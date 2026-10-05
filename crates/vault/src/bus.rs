//! Vault on the system bus: `dev.rift.Vault` at `/dev/rift/Vault`.
//!
//! `List` returns the snapshots of home, oldest first. `Take` takes one now and runs the retention
//! rules. `Restore` copies one file back from a snapshot as the account that asked, and refuses
//! with `org.freedesktop.DBus.Error.FileExists` when the file there has changed, unless it is told
//! to replace it. `Backups`, `Backup` and `RestoreBackup` do the same with the backups on the
//! backup disk, and `Target` says which folder on which disk they go to. `BootStyle` and
//! `SetBootStyle` read and write the word on the esp that says how the next boot looks, which only
//! root can reach. `Slots` says what the drive's two slots hold, `NextVersion` what an update
//! would do, and `Update` installs it into the slot that is not running, which writes a partition
//! of the drive and so answers the owner and root alone.
//! `Owner`, `SetOwnerName` and `SetOwnerPassword` read and change the owner's name
//! and password, and answer the owner and root alone. `AutoUnlock` and `SetAutoUnlock` say whether
//! this machine's tpm holds a key for persist and seal one to it or wipe it, and answer the owner
//! and root alone too. `SecurityKeys` and `RemoveSecurityKey` say which security keys open persist
//! and take one off, and answer the owner and root alone as well. Adding one is not here: it needs
//! a terminal, so `vault enroll-key` does it as root. `Exchange` says whether the drive has an
//! exchange partition, which is read off its table, and `MountExchange` mounts it; both answer the
//! owner and root alone too.
//!
//! In a Ghost boot persist stays locked and nothing of the drive is mounted, so every one of these
//! but `Owner`, `Exchange` and `MountExchange` refuses with the one sentence the mode says
//! (ADR-0084). The refusal is here rather than in each page and command, so there is one place that
//! decides what the drive does not do. The two about the exchange partition answer there because
//! the mode's promise is that Rift writes nothing to the drive, not that the owner may not: reading
//! the table writes nothing, and the mount happens when they ask for it (ADR-0085).

use std::path::PathBuf;
use std::sync::Arc;

use librift::Component;
use librift::boot::Style;
use zbus::fdo;
use zbus::message::Header;

use crate::backup::Backups;
use crate::boot::Esp;
use crate::exchange::Exchange;
use crate::keys::{Keys, Refusal as KeyRefusal};
use crate::owner::{self, Owner, Refusal};
use crate::restore::{self, Account, Outcome, Problem};
use crate::slots::Drive;
use crate::timeline::{self, Timeline};
use crate::tpm::{Refusal as SealRefusal, Sealed};
use crate::update::Updater;

/// The object that answers on the bus.
pub struct Vault {
    timeline: Arc<Timeline>,
    backups: Arc<Backups>,
    home: Arc<PathBuf>,
    esp: Arc<Esp>,
    drive: Arc<Drive>,
    owner: Arc<Owner>,
    sealed: Arc<Sealed>,
    keys: Arc<Keys>,
    exchange: Arc<Exchange>,
    updater: Arc<Updater>,
}

impl Vault {
    /// The three the two update methods need, cloned for the thread they run on.
    fn drive_esp_updater(&self) -> (Arc<Drive>, Arc<Esp>, Arc<Updater>) {
        (
            Arc::clone(&self.drive),
            Arc::clone(&self.esp),
            Arc::clone(&self.updater),
        )
    }
}

/// What a method answers with when this boot is a Ghost one: the one sentence the mode says, as
/// the error, so the page or the command that asked prints it and says nothing untrue.
///
/// Persist stays locked in a Ghost boot and nothing of the drive is mounted, which is the whole of
/// what the mode promises (ADR-0082), so every method that reads or writes what the drive keeps
/// refuses here instead of answering with the image's own defaults or mounting the esp to look.
/// `Owner` is the one read that stays: the session has a name and a password of its own, the
/// image's, and those are the truth about the session (ADR-0084).
fn in_ghost_mode(what: &str) -> fdo::Error {
    fdo::Error::Failed(librift::ghost::cannot(what))
}

/// What each refusal says it cannot do. The sentence a person reads is this and the mode's.
const SNAPSHOTS: &str = "Timeline snapshots cannot be reached";
const TAKE: &str = "A snapshot cannot be taken";
const RESTORE: &str = "A file cannot be restored from a snapshot";
const BACKUPS: &str = "Backups cannot be reached";
const BACKUP: &str = "A backup cannot be made";
const RESTORE_BACKUP: &str = "A file cannot be restored from a backup";
const BOOT_STYLE: &str = "How the next boot looks cannot be read";
const SET_BOOT_STYLE: &str = "How the next boot looks cannot be changed";
const SLOTS: &str = "What this drive holds cannot be read";
const NEXT_VERSION: &str = "What is waiting to be installed cannot be read";
const UPDATE: &str = "An update cannot be installed";
const OWNER_NAME: &str = "The owner's name cannot be changed";
const OWNER_PASSWORD: &str = "The password cannot be changed";
const UNLOCKING: &str = "What opens the drive cannot be read";
const SET_UNLOCKING: &str = "What opens the drive cannot be changed";
const REMOVE_KEY: &str = "A security key cannot be taken off the drive";

/// What the owner and root alone may do, which finishes the sentence anyone else is refused with.
const THE_OWNER: &str = "see or change the owner's name and password";
const THE_EXCHANGE: &str = "mount the drive's exchange partition";
const THE_UPDATE: &str = "install an update, which writes a partition of the drive";

#[zbus::interface(name = "dev.rift.Vault")]
impl Vault {
    /// Every snapshot of home, oldest first.
    async fn list(&self) -> fdo::Result<Vec<String>> {
        if librift::ghost::on() {
            return Err(in_ghost_mode(SNAPSHOTS));
        }
        let timeline = Arc::clone(&self.timeline);
        blocking::unblock(move || timeline.list())
            .await
            .map_err(|e| fdo::Error::Failed(format!("Could not read the snapshots: {e}")))
    }

    /// Takes a snapshot of home now and returns its name.
    async fn take(&self) -> fdo::Result<String> {
        if librift::ghost::on() {
            return Err(in_ghost_mode(TAKE));
        }
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
        if librift::ghost::on() {
            return Err(in_ghost_mode(RESTORE));
        }
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
        if librift::ghost::on() {
            return Err(in_ghost_mode(BACKUPS));
        }
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
        if librift::ghost::on() {
            return Err(in_ghost_mode(BACKUPS));
        }
        let backups = Arc::clone(&self.backups);
        let target = blocking::unblock(move || backups.target())
            .await
            .map_err(fdo::Error::Failed)?;
        Ok((target.folder, target.disk))
    }

    /// Backs up home now and returns the backup's id and when it was made.
    #[zbus(out_args("id", "time"))]
    async fn backup(&self) -> fdo::Result<(String, String)> {
        if librift::ghost::on() {
            return Err(in_ghost_mode(BACKUP));
        }
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
        if librift::ghost::on() {
            return Err(in_ghost_mode(RESTORE_BACKUP));
        }
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
        if librift::ghost::on() {
            return Err(in_ghost_mode(BOOT_STYLE));
        }
        let esp = Arc::clone(&self.esp);
        blocking::unblock(move || esp.style())
            .await
            .map(|style| style.word().to_string())
            .map_err(fdo::Error::Failed)
    }

    /// Writes how the next boot of this drive looks. The word is `text` or `graphical`.
    async fn set_boot_style(&self, style: String) -> fdo::Result<()> {
        if librift::ghost::on() {
            return Err(in_ghost_mode(SET_BOOT_STYLE));
        }
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

    /// Whether this drive has an exchange partition: the plain one Windows, macOS and Linux can
    /// all read, which a drive is written with or without.
    ///
    /// An ordinary boot mounts it before anyone logs in, so Files finds it in the mount table and
    /// never asks this. A Ghost boot mounts nothing of the drive, so the sidebar needs to be told
    /// the partition is there before it can offer it, and the only place that is written down is
    /// the drive's partition table, which is root's to read. Reading it writes nothing, which is
    /// why this answers in a Ghost boot (ADR-0085).
    #[zbus(out_args("there"))]
    async fn exchange(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<bool> {
        owner_or_root(&header, connection, &self.owner, THE_EXCHANGE).await?;
        let exchange = Arc::clone(&self.exchange);
        blocking::unblock(move || exchange.there())
            .await
            .map_err(fdo::Error::Failed)
    }

    /// Mounts the exchange partition and says where it went, which is what a press on it in Files
    /// calls. Mounting one that is mounted already says where it is and changes nothing.
    ///
    /// A unit of its own does the mount: this service answers the bus inside a mount namespace of
    /// its own, where a mount would be invisible to the rest of the machine. In a Ghost boot this
    /// is the one thing that mounts any part of the drive, and it happens because the owner asked.
    #[zbus(out_args("folder"))]
    async fn mount_exchange(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<String> {
        owner_or_root(&header, connection, &self.owner, THE_EXCHANGE).await?;
        let exchange = Arc::clone(&self.exchange);
        blocking::unblock(move || {
            if !exchange.there()? {
                return Err("This drive has no exchange partition.".to_string());
            }
            crate::exchange::mount_now()
        })
        .await
        .map_err(fdo::Error::Failed)
    }

    /// The owner: the account they log in to, the name the lock screen greets them by, and
    /// whether the password is still the one every drive starts with.
    #[zbus(out_args("user", "name", "image_password"))]
    async fn owner(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<(String, String, bool)> {
        owner_or_root(&header, connection, &self.owner, THE_OWNER).await?;
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
        if librift::ghost::on() {
            return Err(in_ghost_mode(OWNER_NAME));
        }
        let uid = owner_or_root(&header, connection, &self.owner, THE_OWNER).await?;
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
        if librift::ghost::on() {
            return Err(in_ghost_mode(OWNER_PASSWORD));
        }
        let uid = owner_or_root(&header, connection, &self.owner, THE_OWNER).await?;
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
        if librift::ghost::on() {
            return Err(in_ghost_mode(SLOTS));
        }
        let drive = Arc::clone(&self.drive);
        let esp = Arc::clone(&self.esp);
        blocking::unblock(move || drive.slots(&esp))
            .await
            .map(|slots| slots.answer())
            .map_err(fdo::Error::Failed)
    }

    /// What an update would do, with nothing written: the version waiting where updates come from,
    /// the version running and its slot, the slot the new one would go into, where it comes from,
    /// how many bytes it is and how many of them this drive does not have yet.
    ///
    /// Working that out means reading the index of the slot this drive runs, and a drive that was
    /// flashed and never updated has none kept, so the first call on such a drive reads the whole
    /// running partition to make one. It is kept after that.
    #[zbus(out_args("version", "running", "running_slot", "slot", "from", "total", "fetch"))]
    async fn next_version(&self) -> fdo::Result<librift::update::Waiting> {
        if librift::ghost::on() {
            return Err(in_ghost_mode(NEXT_VERSION));
        }
        let (drive, esp, updater) = self.drive_esp_updater();
        blocking::unblock(move || {
            let slots = drive.slots(&esp)?;
            updater.next(&slots, &esp).map(|plan| plan.answer())
        })
        .await
        .map_err(fdo::Error::Failed)
    }

    /// Installs the version waiting where updates come from into the slot that is not running: its
    /// store, the verity tree over it, the names of the two partitions and its uki on the esp.
    /// Returns the version, the slot it went into, the bytes fetched and the bytes taken from the
    /// slot this drive runs.
    ///
    /// It writes a partition of the drive, which nothing but flashing and cloning has done before,
    /// so it only ever writes the slot that is not running, and the uki that makes that slot
    /// bootable goes on last, after the root hash of the tree it made matches the published one.
    #[zbus(out_args("version", "slot", "fetched", "seeded"))]
    async fn update(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<librift::update::Installed> {
        if librift::ghost::on() {
            return Err(in_ghost_mode(UPDATE));
        }
        owner_or_root(&header, connection, &self.owner, THE_UPDATE).await?;
        let (drive, esp, updater) = self.drive_esp_updater();
        blocking::unblock(move || {
            let slots = drive.slots(&esp)?;
            updater.write(&slots, &esp, &mut |line| println!("vault: {line}"))
        })
        .await
        .map(|written| {
            println!(
                "vault: version {} is in slot {}",
                written.version, written.slot
            );
            written.answer()
        })
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
        if librift::ghost::on() {
            return Err(in_ghost_mode(UNLOCKING));
        }
        owner_or_root(&header, connection, &self.owner, THE_OWNER).await?;
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
        if librift::ghost::on() {
            return Err(in_ghost_mode(SET_UNLOCKING));
        }
        let uid = owner_or_root(&header, connection, &self.owner, THE_OWNER).await?;
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

    /// Which security keys open the drive: the keyslot each one opens, whether it asks for its pin
    /// at boot and whether it has to be touched. Adding one is not on the bus, because the key is
    /// touched and its pin typed while the enrollment waits.
    #[zbus(out_args("keys"))]
    async fn security_keys(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<Vec<(u32, bool, bool)>> {
        if librift::ghost::on() {
            return Err(in_ghost_mode(UNLOCKING));
        }
        owner_or_root(&header, connection, &self.owner, THE_OWNER).await?;
        let keys = Arc::clone(&self.keys);
        blocking::unblock(move || keys.list())
            .await
            .map(|keys| {
                keys.into_iter()
                    .map(|key| (key.slot, key.pin, key.presence))
                    .collect()
            })
            .map_err(fdo::Error::Failed)
    }

    /// Takes the security key in a keyslot off the drive. The passphrase slot is never one of
    /// these, so the drive still opens with it.
    async fn remove_security_key(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        slot: u32,
    ) -> fdo::Result<()> {
        if librift::ghost::on() {
            return Err(in_ghost_mode(REMOVE_KEY));
        }
        let uid = owner_or_root(&header, connection, &self.owner, THE_OWNER).await?;
        let keys = Arc::clone(&self.keys);
        blocking::unblock(move || keys.remove(slot))
            .await
            .map_err(|refusal| match refusal {
                KeyRefusal::NotAKey(_) => fdo::Error::InvalidArgs(refusal.why()),
                other => fdo::Error::Failed(other.why()),
            })?;
        println!("vault: the security key in keyslot {slot} is off this drive, by uid {uid}");
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

/// The owner and root may do `what`, and no one else: not Rift's own services, which run as
/// accounts of their own. The uid of the sender, when it may. `what` finishes the sentence anyone
/// else is refused with, so it reads as what was asked for: "see or change the owner's name and
/// password".
async fn owner_or_root(
    header: &Header<'_>,
    connection: &zbus::Connection,
    owner: &Owner,
    what: &str,
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
    Err(fdo::Error::AccessDenied(format!(
        "Only the owner and root may {what}."
    )))
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

/// What the service answers questions about, which `main` builds from where everything is.
pub struct Parts {
    pub timeline: Timeline,
    pub backups: Backups,
    pub home: PathBuf,
    pub esp: Esp,
    pub drive: Drive,
    pub sealed: Sealed,
    pub keys: Keys,
    pub updater: Updater,
}

/// Takes the name and answers until the process is stopped.
///
/// # Errors
///
/// When the system bus is not there, or another process already owns the name.
pub fn serve(parts: Parts) -> zbus::Result<()> {
    parts.backups.clear();
    let component = Component::Vault;
    let vault = Vault {
        timeline: Arc::new(parts.timeline),
        backups: Arc::new(parts.backups),
        home: Arc::new(parts.home),
        esp: Arc::new(parts.esp),
        drive: Arc::new(parts.drive),
        owner: Arc::new(Owner::system()),
        sealed: Arc::new(parts.sealed),
        keys: Arc::new(parts.keys),
        updater: Arc::new(parts.updater),
        // the one part built here rather than passed in, the way the owner is: every place it
        // looks is the running drive's own and there is nothing for a caller to choose
        exchange: Arc::new(Exchange::default()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_names_the_mode_and_reads_as_a_sentence() {
        let refusals = [
            SNAPSHOTS,
            TAKE,
            RESTORE,
            NEXT_VERSION,
            UPDATE,
            BACKUPS,
            BACKUP,
            RESTORE_BACKUP,
            BOOT_STYLE,
            SET_BOOT_STYLE,
            SLOTS,
            OWNER_NAME,
            OWNER_PASSWORD,
            UNLOCKING,
            SET_UNLOCKING,
            REMOVE_KEY,
        ];
        for what in refusals {
            let said = librift::ghost::cannot(what);
            assert!(said.starts_with(what), "{said}");
            assert!(said.contains(librift::ghost::NAME), "{said}");
            assert!(said.ends_with('.'), "{said}");
            assert!(said.is_ascii(), "{said}");
            // what cannot be done, not an error: no full stop of its own and no path in it
            assert!(!what.ends_with('.'), "{what}");
            assert!(!what.contains('/'), "{what}");
        }
    }

    #[test]
    fn what_only_the_owner_may_do_finishes_the_sentence() {
        for what in [THE_OWNER, THE_EXCHANGE] {
            let said = format!("Only the owner and root may {what}.");
            assert!(said.is_ascii(), "{said}");
            assert!(!what.ends_with('.'), "{what}");
            // a verb, so the sentence reads as one thing: not a noun and not a capital
            assert!(
                what.starts_with(|first: char| first.is_ascii_lowercase()),
                "{what}"
            );
        }
        assert_eq!(
            format!("Only the owner and root may {THE_EXCHANGE}."),
            "Only the owner and root may mount the drive's exchange partition."
        );
    }
}
