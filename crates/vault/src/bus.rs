//! Vault on the system bus: `dev.rift.Vault` at `/dev/rift/Vault`.
//!
//! `List` returns the snapshots of home, oldest first. `Take` takes one now and runs the retention
//! rules. `Restore` copies one file back from a snapshot as the account that asked, and refuses
//! with `org.freedesktop.DBus.Error.FileExists` when the file there has changed, unless it is told
//! to replace it. `Backups`, `Backup` and `RestoreBackup` do the same with the backups on the
//! backup disk, and `Target` says which folder on which disk they go to. `BootStyle` and
//! `SetBootStyle` read and write the word on the esp that says how the next boot looks, which only
//! root can reach. `Owner`, `SetOwnerName` and `SetOwnerPassword` read and change the owner's name
//! and password, and answer the owner and root alone. `AutoUnlock` and `SetAutoUnlock` say whether
//! this machine's tpm holds a key for persist and seal one to it or wipe it, and answer the owner
//! and root alone too.

use std::path::PathBuf;
use std::sync::Arc;

use librift::Component;
use librift::boot::Style;
use zbus::fdo;
use zbus::message::Header;

use crate::backup::Backups;
use crate::boot::Esp;
use crate::owner::{self, Owner, Refusal};
use crate::restore::{self, Account, Outcome, Problem};
use crate::slots::Drive;
use crate::timeline::{self, Timeline};
use crate::tpm::{Refusal as SealRefusal, Sealed};

/// The object that answers on the bus.
pub struct Vault {
    timeline: Arc<Timeline>,
    backups: Arc<Backups>,
    home: Arc<PathBuf>,
    esp: Arc<Esp>,
    drive: Arc<Drive>,
    owner: Arc<Owner>,
    sealed: Arc<Sealed>,
}

#[zbus::interface(name = "dev.rift.Vault")]
impl Vault {
    /// Every snapshot of home, oldest first.
    async fn list(&self) -> fdo::Result<Vec<String>> {
        let timeline = Arc::clone(&self.timeline);
        blocking::unblock(move || timeline.list())
            .await
            .map_err(|e| fdo::Error::Failed(format!("Could not read the snapshots: {e}")))
    }

    /// Takes a snapshot of home now and returns its name.
    async fn take(&self) -> fdo::Result<String> {
        let timeline = Arc::clone(&self.timeline);
        let (name, dropped) = blocking::unblock(move || timeline.take())
            .await
            .map_err(fdo::Error::Failed)?;
        println!("vault: took snapshot {name} for the bus");
        for old in dropped {
            println!("vault: dropped snapshot {old}");
        }
        Ok(name)
    }

    /// Restores the file at `path` from `snapshot`: `restored`, `replaced` or `unchanged`, and
    /// the path written.
    #[zbus(out_args("outcome", "path"))]
    async fn restore(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        snapshot: String,
        path: String,
        replace: bool,
    ) -> fdo::Result<(String, String)> {
        let account = caller(&header, connection).await?;
        let timeline = Arc::clone(&self.timeline);
        let home = Arc::clone(&self.home);
        let done = blocking::unblock(move || {
            restore::restore(
                &timeline.snapshots,
                &home,
                &snapshot,
                &path,
                replace,
                account,
            )
        })
        .await;
        answer(done, account)
    }

    /// Every backup on the backup disk, oldest first: its id and when it was made.
    async fn backups(&self) -> fdo::Result<Vec<(String, String)>> {
        let backups = Arc::clone(&self.backups);
        let listed = blocking::unblock(move || backups.list())
            .await
            .map_err(fdo::Error::Failed)?;
        Ok(listed
            .into_iter()
            .map(|made| (made.id, timeline::name_of(made.time)))
            .collect())
    }

    /// Where backups go: the folder on the backup disk, and the uuid of the file system it is on.
    #[zbus(out_args("folder", "disk"))]
    async fn target(&self) -> fdo::Result<(String, String)> {
        let backups = Arc::clone(&self.backups);
        let target = blocking::unblock(move || backups.target())
            .await
            .map_err(fdo::Error::Failed)?;
        Ok((target.folder, target.disk))
    }

    /// Backs up home now and returns the backup's id and when it was made.
    #[zbus(out_args("id", "time"))]
    async fn backup(&self) -> fdo::Result<(String, String)> {
        let backups = Arc::clone(&self.backups);
        let made = blocking::unblock(move || backups.back_up())
            .await
            .map_err(fdo::Error::Failed)?;
        let time = timeline::name_of(made.time);
        println!("vault: backed up home as {} at {time} for the bus", made.id);
        Ok((made.id, time))
    }

    /// Restores the file at `path` from the backup with the id `backup`, the way `Restore` does
    /// from a snapshot.
    #[zbus(out_args("outcome", "path"))]
    async fn restore_backup(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        backup: String,
        path: String,
        replace: bool,
    ) -> fdo::Result<(String, String)> {
        let account = caller(&header, connection).await?;
        let backups = Arc::clone(&self.backups);
        let home = Arc::clone(&self.home);
        let done =
            blocking::unblock(move || backups.restore(&home, &backup, &path, replace, account))
                .await;
        answer(done, account)
    }

    /// How the next boot of this drive looks: `text` or `graphical`.
    async fn boot_style(&self) -> fdo::Result<String> {
        let esp = Arc::clone(&self.esp);
        blocking::unblock(move || esp.style())
            .await
            .map(|style| style.word().to_string())
            .map_err(fdo::Error::Failed)
    }

    /// Writes how the next boot of this drive looks. The word is `text` or `graphical`.
    async fn set_boot_style(&self, style: String) -> fdo::Result<()> {
        let wanted = Style::from_setting(&style);
        if wanted.word() != style.trim() {
            return Err(fdo::Error::InvalidArgs(format!(
                "\"{}\" is not a boot style. It is text or graphical.",
                style.trim()
            )));
        }
        let esp = Arc::clone(&self.esp);
        blocking::unblock(move || esp.set_style(wanted))
            .await
            .map_err(fdo::Error::Failed)?;
        println!("vault: the next boot of this drive is {}", wanted.word());
        Ok(())
    }

    /// The owner: the account they log in to, the name the lock screen greets them by, and
    /// whether the password is still the one every drive starts with.
    #[zbus(out_args("user", "name", "image_password"))]
    async fn owner(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<(String, String, bool)> {
        owner_or_root(&header, connection, &self.owner).await?;
        let owner = Arc::clone(&self.owner);
        blocking::unblock(move || {
            let account = owner.account()?;
            Ok((account.user, account.name, owner.image_password()?))
        })
        .await
        .map_err(fdo::Error::Failed)
    }

    /// Gives the owner a new name, now and at every boot after.
    async fn set_owner_name(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        name: String,
    ) -> fdo::Result<()> {
        let uid = owner_or_root(&header, connection, &self.owner).await?;
        let owner = Arc::clone(&self.owner);
        let chosen = name.trim().to_string();
        blocking::unblock(move || owner.keep_name(&name).map(|()| owner::apply_now()))
            .await
            .map_err(refused)?
            .map_err(fdo::Error::Failed)?;
        println!("vault: the owner is called {chosen}, set by uid {uid}");
        Ok(())
    }

    /// Gives the owner a new password, now and at every boot after, once `current` has been
    /// checked against the one they have.
    async fn set_owner_password(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        current: String,
        new: String,
    ) -> fdo::Result<()> {
        let uid = owner_or_root(&header, connection, &self.owner).await?;
        let owner = Arc::clone(&self.owner);
        let done = blocking::unblock(move || {
            owner
                .keep_password(&current, &new)
                .map(|()| owner::apply_now())
        })
        .await;
        match done {
            Err(Refusal::Wrong(why)) => {
                println!("vault: a wrong current password from uid {uid}");
                Err(fdo::Error::AccessDenied(why))
            }
            other => {
                other.map_err(refused)?.map_err(fdo::Error::Failed)?;
                println!("vault: the owner has a new password, set by uid {uid}");
                Ok(())
            }
        }
    }

    /// What the drive's two slots hold: the version running, the slot, the version and the name
    /// of the uki on the esp for each slot, where updates come from, and the versions waiting
    /// there. A uki's name carries the boots systemd-boot has left to try of it.
    #[zbus(out_args("running", "slots", "source", "waiting"))]
    async fn slots(&self) -> fdo::Result<librift::update::Answer> {
        let drive = Arc::clone(&self.drive);
        let esp = Arc::clone(&self.esp);
        blocking::unblock(move || drive.slots(&esp))
            .await
            .map(|slots| slots.answer())
            .map_err(fdo::Error::Failed)
    }

    /// Whether this machine's tpm holds a key for persist: `on` here, `elsewhere` when the key was
    /// sealed on another machine, `off` when no machine holds one. The second value is that other
    /// machine's fingerprint, and the third says whether this machine has a tpm at all.
    #[zbus(out_args("state", "machine", "has_tpm"))]
    async fn auto_unlock(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<(String, String, bool)> {
        owner_or_root(&header, connection, &self.owner).await?;
        let fingerprint = this_machine().await?;
        let sealed = Arc::clone(&self.sealed);
        blocking::unblock(move || {
            let state = sealed.state(&fingerprint)?;
            Ok((
                state.word().to_string(),
                state.machine().to_string(),
                crate::tpm::machine_has_a_tpm(),
            ))
        })
        .await
        .map_err(fdo::Error::Failed)
    }

    /// Seals a key for persist to this machine's tpm, or wipes the one that is sealed. Sealing one
    /// takes the drive's passphrase, which is how the volume key is read, and only happens on a
    /// machine the owner has said is theirs. The passphrase slot is never touched either way.
    async fn set_auto_unlock(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        on: bool,
        passphrase: String,
    ) -> fdo::Result<()> {
        let uid = owner_or_root(&header, connection, &self.owner).await?;
        let sealed = Arc::clone(&self.sealed);
        if !on {
            blocking::unblock(move || sealed.turn_off())
                .await
                .map_err(fdo::Error::Failed)?;
            println!("vault: this drive asks for its passphrase again, set by uid {uid}");
            return Ok(());
        }
        let class = host_class().await?;
        if class != "owned" {
            return Err(sealing_refused(&SealRefusal::NotOwned(class)));
        }
        let fingerprint = this_machine().await?;
        let short = fingerprint.get(..12).unwrap_or(&fingerprint).to_string();
        blocking::unblock(move || sealed.turn_on(&passphrase, &fingerprint))
            .await
            .map_err(|refusal| {
                if matches!(refusal, SealRefusal::Wrong(_)) {
                    println!("vault: a wrong drive passphrase from uid {uid}");
                }
                sealing_refused(&refusal)
            })?;
        println!("vault: this drive opens by itself on machine {short}, set by uid {uid}");
        Ok(())
    }
}

/// The fingerprint of the machine this is running on, which Orbit is the one to say.
async fn this_machine() -> fdo::Result<String> {
    orbit_says(|host| host.fingerprint).await
}

/// This machine's class: `owned`, `trusted` or `borrowed`.
async fn host_class() -> fdo::Result<String> {
    orbit_says(|host| host.class).await
}

/// One thing Orbit remembers about this machine. Orbit owns the host profile, so Vault asks it
/// rather than reading the file behind its back.
async fn orbit_says(pick: fn(librift::orbit::Host) -> String) -> fdo::Result<String> {
    blocking::unblock(move || librift::orbit::host().map(pick))
        .await
        .map_err(|why| {
            fdo::Error::Failed(format!("Orbit could not say what this machine is: {why}"))
        })
}

/// The error a sealing was refused with, of the kind that says why.
fn sealing_refused(refusal: &SealRefusal) -> fdo::Error {
    let why = refusal.why();
    match refusal {
        SealRefusal::NotOwned(_) | SealRefusal::Wrong(_) => fdo::Error::AccessDenied(why),
        SealRefusal::NoTpm => fdo::Error::NotSupported(why),
        SealRefusal::Failed(_) => fdo::Error::Failed(why),
    }
}

/// The account that sent the message: its uid from the bus, its group from the password file.
async fn caller(header: &Header<'_>, connection: &zbus::Connection) -> fdo::Result<Account> {
    let sender = header
        .sender()
        .ok_or_else(|| fdo::Error::Failed("The request came without a sender.".into()))?
        .to_owned();
    let uid = fdo::DBusProxy::new(connection)
        .await?
        .get_connection_unix_user(sender.into())
        .await?;
    let passwd = std::fs::read_to_string("/etc/passwd")
        .map_err(|e| fdo::Error::Failed(format!("Could not read the password file: {e}")))?;
    let gid = restore::group_of(&passwd, uid).ok_or_else(|| {
        fdo::Error::AccessDenied(format!("Vault does not know the account with uid {uid}."))
    })?;
    Ok(Account { uid, gid })
}

/// The owner and root may read and change the owner's name and password, and no one else: not
/// Rift's own services, which run as accounts of their own. The uid of the sender, when it may.
async fn owner_or_root(
    header: &Header<'_>,
    connection: &zbus::Connection,
    owner: &Owner,
) -> fdo::Result<u32> {
    let sender = header
        .sender()
        .ok_or_else(|| fdo::Error::Failed("The request came without a sender.".into()))?
        .to_owned();
    let uid = fdo::DBusProxy::new(connection)
        .await?
        .get_connection_unix_user(sender.into())
        .await?;
    if uid == 0 || owner.account().is_ok_and(|account| account.uid == uid) {
        return Ok(uid);
    }
    Err(fdo::Error::AccessDenied(
        "Only the owner and root may see or change the owner's name and password.".into(),
    ))
}

/// The error a change to the owner was refused with, of the kind that says why.
fn refused(refusal: Refusal) -> fdo::Error {
    match refusal {
        Refusal::Wrong(why) => fdo::Error::AccessDenied(why),
        Refusal::Invalid(why) => fdo::Error::InvalidArgs(why),
        Refusal::Failed(why) => fdo::Error::Failed(why),
    }
}

/// The reply to a restore, with a problem as the error that says what kind it is.
fn answer(
    done: Result<(Outcome, PathBuf), Problem>,
    account: Account,
) -> fdo::Result<(String, String)> {
    let (outcome, written) = done.map_err(|problem| match problem {
        Problem::Invalid(s) => fdo::Error::InvalidArgs(s),
        Problem::Missing(s) => fdo::Error::FileNotFound(s),
        Problem::Changed(s) => fdo::Error::FileExists(s),
        Problem::Failed(s) => fdo::Error::Failed(s),
    })?;
    println!(
        "vault: {} {} for uid {}",
        outcome.name(),
        written.display(),
        account.uid
    );
    Ok((outcome.name().to_string(), written.display().to_string()))
}

/// Takes the name and answers until the process is stopped.
///
/// # Errors
///
/// When the system bus is not there, or another process already owns the name.
pub fn serve(
    timeline: Timeline,
    backups: Backups,
    home: PathBuf,
    esp: Esp,
    drive: Drive,
    sealed: Sealed,
) -> zbus::Result<()> {
    backups.clear();
    let component = Component::Vault;
    let vault = Vault {
        timeline: Arc::new(timeline),
        backups: Arc::new(backups),
        home: Arc::new(home),
        esp: Arc::new(esp),
        drive: Arc::new(drive),
        owner: Arc::new(Owner::system()),
        sealed: Arc::new(sealed),
    };
    let _connection = zbus::blocking::connection::Builder::system()?
        .name(component.dbus_name())?
        .serve_at(component.dbus_path(), vault)?
        .build()?;
    // the connection runs on its own threads; this one has nothing left to do
    loop {
        std::thread::park();
    }
}
