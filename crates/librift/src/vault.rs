//! Vault from a client's side: the snapshots Timeline keeps of home, the backups on the backup
//! disk and where they go, making them, restoring a file from one, the boot style on the drive's
//! esp, which only root can write, whether this machine's tpm opens the drive by itself, and which
//! security keys open it.

use std::path::{Path, PathBuf};
#[cfg(feature = "bus")]
use std::time::Duration;

#[cfg(feature = "bus")]
use crate::boot::Style;
#[cfg(feature = "bus")]
use crate::update::{Answer, Slots};
#[cfg(feature = "bus")]
use crate::{Component, bus};

/// How long taking a snapshot may take. The hourly one can hold the lock for a moment.
#[cfg(feature = "bus")]
const TAKE_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a restore may take. A big file is copied in full.
#[cfg(feature = "bus")]
const RESTORE_TIMEOUT: Duration = Duration::from_secs(1800);

/// How long listing the backups may take. Vault mounts the disk and rustic reads the index.
#[cfg(feature = "bus")]
const BACKUPS_TIMEOUT: Duration = Duration::from_secs(600);

/// How long a backup may take. The first one of a full home reads all of it.
#[cfg(feature = "bus")]
const BACKUP_TIMEOUT: Duration = Duration::from_secs(24 * 3600);

/// How long reading or writing the boot style may take. Vault mounts the esp for it.
#[cfg(feature = "bus")]
const BOOT_STYLE_TIMEOUT: Duration = Duration::from_secs(60);

/// How long reading the two slots may take. Vault mounts the esp and reads the drive's table.
#[cfg(feature = "bus")]
const SLOTS_TIMEOUT: Duration = Duration::from_secs(60);

/// How long sealing a key to the tpm may take. cryptenroll reads the volume key out of a slot,
/// which is argon2id over the passphrase, and then talks to the tpm.
#[cfg(feature = "bus")]
const SEAL_TIMEOUT: Duration = Duration::from_secs(300);

/// How long reading or removing a security key may take. Both are header edits; removing one
/// rewrites the metadata and takes its lock.
#[cfg(feature = "bus")]
const KEYS_TIMEOUT: Duration = Duration::from_secs(60);

const HOUR: i64 = 3600;
const DAY: i64 = 24 * HOUR;

/// Where Vault keeps the snapshots of home, one folder each, named by the time it was taken.
pub const SNAPSHOTS: &str = "/persist/@snapshots/home";

/// Where the subvolume the snapshots are of is mounted, so a path under it has a place in each of
/// them.
pub const HOME: &str = "/home";

/// Where `path`, somewhere under [`HOME`], lies inside the snapshot named `at`. `None` when the
/// path is not under home, when it has a `.` or a `..` in it, or when the name is not a
/// snapshot's.
#[must_use]
pub fn in_snapshot(at: &str, path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    snapshot_time(at)?;
    let rest = path.strip_prefix(HOME).ok()?;
    if rest
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return None;
    }
    Some(Path::new(SNAPSHOTS).join(at).join(rest))
}

/// The first 8 digits of a backup's id, which is what people see and type.
#[must_use]
pub fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

/// Seconds since 1970 as the name of a snapshot taken then: `2026-09-12T14:00:03Z`.
#[must_use]
pub fn snapshot_name(secs: i64) -> String {
    let (year, month, day) = civil(secs.div_euclid(DAY));
    let rest = secs.rem_euclid(DAY);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / HOUR,
        rest % HOUR / 60,
        rest % 60
    )
}

/// The time a snapshot name stands for in seconds since 1970, or `None` when the name is not one.
/// Only the exact form [`snapshot_name`] writes counts, so every time has one name.
#[must_use]
pub fn snapshot_time(name: &str) -> Option<i64> {
    let bytes = name.as_bytes();
    let shape = [
        (4, b'-'),
        (7, b'-'),
        (10, b'T'),
        (13, b':'),
        (16, b':'),
        (19, b'Z'),
    ];
    if bytes.len() != 20 || shape.iter().any(|&(at, c)| bytes[at] != c) {
        return None;
    }
    let number = |from: usize, to: usize| {
        name.get(from..to)?.bytes().try_fold(0, |n: i64, digit| {
            if digit.is_ascii_digit() {
                Some(n * 10 + i64::from(digit - b'0'))
            } else {
                None
            }
        })
    };
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    let valid = (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour < 24
        && minute < 60
        && second < 60;
    valid.then(|| days_from_civil(year, month, day) * DAY + hour * HOUR + minute * 60 + second)
}

/// Days since 1970-01-01 of a date, the era arithmetic from Howard Hinnant's notes.
pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day since 1970-01-01.
pub(crate) fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The error Vault refuses a restore with when the file at the path has changed.
pub const CHANGED: &str = "org.freedesktop.DBus.Error.FileExists";

/// What a restore did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restored {
    /// Nothing was at the path, the copy is there now.
    Restored,
    /// A different file was at the path and the copy took its place.
    Replaced,
    /// The file at the path already had the same bytes.
    Unchanged,
}

/// Why a restore did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The file at the path has changed since the snapshot. Holds Vault's sentence. Asking again
    /// with `replace` overwrites it.
    Changed(String),
    /// Anything else, as a sentence.
    Other(String),
}

/// Where backups go: a folder on a file system, and the file system's uuid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The folder, from the root of that file system.
    pub folder: String,
    /// The file system's uuid, as `/dev/disk/by-uuid` names it.
    pub disk: String,
}

/// What the outcome string `Restore` returned means.
#[must_use]
pub fn read(outcome: &str) -> Option<Restored> {
    match outcome {
        "restored" => Some(Restored::Restored),
        "replaced" => Some(Restored::Replaced),
        "unchanged" => Some(Restored::Unchanged),
        _ => None,
    }
}

/// The snapshots of home, oldest first.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, or Vault could not read them.
#[cfg(feature = "bus")]
pub fn list() -> Result<Vec<String>, String> {
    let vault = Component::Vault;
    let connection = bus::connect(bus::PROPERTY_TIMEOUT)?;
    let proxy = bus::proxy(&connection, vault)?;
    proxy.call("List", &()).map_err(|e| bus::sentence(vault, e))
}

/// Takes a snapshot of home now and returns its name.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, or the snapshot could not be taken.
#[cfg(feature = "bus")]
pub fn take() -> Result<String, String> {
    let vault = Component::Vault;
    let connection = bus::connect(TAKE_TIMEOUT)?;
    let proxy = bus::proxy(&connection, vault)?;
    proxy.call("Take", &()).map_err(|e| bus::sentence(vault, e))
}

/// Restores `path`, a full path to a file in home, from `snapshot`. Without `replace` a file that
/// changed since the snapshot is left as it is. Returns what happened and the path written.
///
/// # Errors
///
/// [`Refusal::Changed`] when the file changed and `replace` is false, otherwise a sentence.
#[cfg(feature = "bus")]
pub fn restore(snapshot: &str, path: &str, replace: bool) -> Result<(Restored, String), Refusal> {
    restore_with("Restore", snapshot, path, replace)
}

/// The backups on the backup disk, oldest first: each one's id and when it was made.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, there is no backup disk, or it is not plugged in.
#[cfg(feature = "bus")]
pub fn backups() -> Result<Vec<(String, String)>, String> {
    let vault = Component::Vault;
    let connection = bus::connect(BACKUPS_TIMEOUT)?;
    let proxy = bus::proxy(&connection, vault)?;
    proxy
        .call("Backups", &())
        .map_err(|e| bus::sentence(vault, e))
}

/// Where backups go, once a folder on a disk has been chosen.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, or no folder has been chosen yet.
#[cfg(feature = "bus")]
pub fn target() -> Result<Target, String> {
    let vault = Component::Vault;
    let connection = bus::connect(bus::PROPERTY_TIMEOUT)?;
    let proxy = bus::proxy(&connection, vault)?;
    let (folder, disk): (String, String) = proxy
        .call("Target", &())
        .map_err(|e| bus::sentence(vault, e))?;
    Ok(Target { folder, disk })
}

/// What the drive's two slots hold, which version is running, where updates come from and the
/// versions waiting there. All of it is root's to read, so Vault reads it.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, or the drive could not be read.
#[cfg(feature = "bus")]
pub fn slots() -> Result<Slots, String> {
    let vault = Component::Vault;
    let connection = bus::connect(SLOTS_TIMEOUT)?;
    let proxy = bus::proxy(&connection, vault)?;
    let answer: Answer = proxy
        .call("Slots", &())
        .map_err(|e| bus::sentence(vault, e))?;
    Ok(Slots::from_answer(answer))
}

/// Backs up home now. Returns the backup's id and when it was made.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, or the backup could not be made.
#[cfg(feature = "bus")]
pub fn backup() -> Result<(String, String), String> {
    let vault = Component::Vault;
    let connection = bus::connect(BACKUP_TIMEOUT)?;
    let proxy = bus::proxy(&connection, vault)?;
    proxy
        .call("Backup", &())
        .map_err(|e| bus::sentence(vault, e))
}

/// Restores `path`, a full path to a file in home, from the backup with the id `backup`, the way
/// [`restore`] does from a snapshot.
///
/// # Errors
///
/// [`Refusal::Changed`] when the file changed and `replace` is false, otherwise a sentence.
#[cfg(feature = "bus")]
pub fn restore_backup(
    backup: &str,
    path: &str,
    replace: bool,
) -> Result<(Restored, String), Refusal> {
    restore_with("RestoreBackup", backup, path, replace)
}

/// The boot style on the esp of the drive this system started from.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, or the esp could not be read.
#[cfg(feature = "bus")]
pub fn boot_style() -> Result<Style, String> {
    let vault = Component::Vault;
    let connection = bus::connect(BOOT_STYLE_TIMEOUT)?;
    let proxy = bus::proxy(&connection, vault)?;
    let word: String = proxy
        .call("BootStyle", &())
        .map_err(|e| bus::sentence(vault, e))?;
    Ok(Style::from_setting(&word))
}

/// Writes the boot style onto that esp, where the initrd reads it before the next boot's splash.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, or the esp could not be written.
#[cfg(feature = "bus")]
pub fn set_boot_style(style: Style) -> Result<(), String> {
    let vault = Component::Vault;
    let connection = bus::connect(BOOT_STYLE_TIMEOUT)?;
    let proxy = bus::proxy(&connection, vault)?;
    proxy
        .call("SetBootStyle", &(style.word(),))
        .map_err(|e| bus::sentence(vault, e))
}

#[cfg(feature = "bus")]
fn restore_with(
    method: &str,
    from: &str,
    path: &str,
    replace: bool,
) -> Result<(Restored, String), Refusal> {
    let vault = Component::Vault;
    let connection = bus::connect(RESTORE_TIMEOUT).map_err(Refusal::Other)?;
    let proxy = bus::proxy(&connection, vault).map_err(Refusal::Other)?;
    let (outcome, written): (String, String) =
        proxy
            .call(method, &(from, path, replace))
            .map_err(|e| match changed(&e) {
                Some(why) => Refusal::Changed(why),
                None => Refusal::Other(bus::sentence(vault, e)),
            })?;
    let restored = read(&outcome).ok_or_else(|| {
        Refusal::Other(format!(
            "Vault said \"{outcome}\", which this program does not understand."
        ))
    })?;
    Ok((restored, written))
}

/// Vault's sentence, when the error is the one for a file that changed.
#[cfg(feature = "bus")]
fn changed(error: &zbus::Error) -> Option<String> {
    match error {
        zbus::Error::MethodError(name, detail, _) if name.as_str() == CHANGED => {
            Some(detail.clone().unwrap_or_default())
        }
        zbus::Error::FDO(error) => match error.as_ref() {
            zbus::fdo::Error::FileExists(detail) => Some(detail.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// What a drive's auto-unlock is on the machine that asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoUnlock {
    /// No machine's tpm holds a key for this drive. It opens with its passphrase and nothing else.
    Off,
    /// This machine's tpm holds one, so this machine opens it without the passphrase.
    Here,
    /// A key is sealed to a tpm and the machine it was sealed on is not this one. Holds that
    /// machine's fingerprint, which is empty when nothing wrote it down.
    Elsewhere(String),
}

impl AutoUnlock {
    /// Whether this machine opens the drive by itself.
    #[must_use]
    pub fn here(&self) -> bool {
        matches!(self, AutoUnlock::Here)
    }
}

/// Why a machine whose class is not `owned` may not hold a key for the drive. Vault refuses with
/// it, and the command says it before asking for a passphrase that would be refused.
#[must_use]
pub fn not_owned(class: &str) -> String {
    format!(
        "This drive only opens by itself on a machine the owner has said is theirs. This one is \
         {class}. Run rift host set class owned first."
    )
}

/// Why a machine with no tpm cannot hold a key for the drive.
pub const NO_TPM: &str = "This machine has no tpm, so there is nothing to seal a key to.";

/// What the word Vault answers with means.
#[must_use]
pub fn auto_unlock_state(word: &str, machine: &str) -> Option<AutoUnlock> {
    match word {
        "off" => Some(AutoUnlock::Off),
        "on" => Some(AutoUnlock::Here),
        "elsewhere" => Some(AutoUnlock::Elsewhere(machine.to_string())),
        _ => None,
    }
}

/// Whether this machine's tpm holds a key for persist, and whether the machine has a tpm at all.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, when the caller is neither the owner nor root,
/// or when the header could not be read.
#[cfg(feature = "bus")]
pub fn auto_unlock() -> Result<(AutoUnlock, bool), String> {
    let vault = Component::Vault;
    let connection = bus::connect(bus::PROPERTY_TIMEOUT)?;
    let proxy = bus::proxy(&connection, vault)?;
    let (word, machine, has_tpm): (String, String, bool) = proxy
        .call("AutoUnlock", &())
        .map_err(|e| bus::sentence(vault, e))?;
    let state = auto_unlock_state(&word, &machine)
        .ok_or_else(|| format!("Vault said \"{word}\", which this program does not understand."))?;
    Ok((state, has_tpm))
}

/// Seals a key for persist to this machine's tpm, or wipes the one that is sealed. Sealing takes
/// the drive's passphrase; wiping takes none.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, when this machine is not the owner's, when it
/// has no tpm, or when the passphrase does not open the drive.
#[cfg(feature = "bus")]
pub fn set_auto_unlock(on: bool, passphrase: &str) -> Result<(), String> {
    let vault = Component::Vault;
    let connection = bus::connect(SEAL_TIMEOUT)?;
    bus::proxy(&connection, vault)?
        .call("SetAutoUnlock", &(on, passphrase))
        .map_err(|e| bus::sentence(vault, e))
}

/// One security key that opens the drive, as the header holds it.
///
/// A key is not a property of a machine: it is a thing the owner carries with the drive, and the
/// header is the only place that says a key was enrolled. Which key it is has no name, because
/// systemd's token holds none, so a key is the keyslot its own key sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityKey {
    /// The keyslot the key opens, which is what removes it.
    pub slot: u32,
    /// Whether the key asks for its pin at boot.
    pub pin: bool,
    /// Whether the key has to be touched at boot.
    pub presence: bool,
}

impl SecurityKey {
    /// What the key asks for at boot, as the end of a sentence.
    #[must_use]
    pub fn asks(&self) -> &'static str {
        match (self.pin, self.presence) {
            (true, true) => "its PIN and a touch",
            (true, false) => "its PIN",
            (false, true) => "a touch",
            (false, false) => "nothing",
        }
    }
}

/// Why there is no security key to enroll. Both halves are needed: the first says what is wrong
/// and the second what to do about it.
pub const NO_SECURITY_KEY: &str = "No security key is plugged in, so there is nothing to \
                                   enroll. Plug one in and run it again.";

/// Why a keyslot cannot be removed as a security key.
#[must_use]
pub fn not_a_security_key(slot: u32) -> String {
    format!(
        "Keyslot {slot} does not hold a security key. rift host keys lists the keyslots that do."
    )
}

/// What the keys the drive holds read as: one line each, or one sentence when there are none.
#[must_use]
pub fn keys_read_as(keys: &[SecurityKey]) -> Vec<String> {
    if keys.is_empty() {
        return vec!["No security key opens this drive.".to_string()];
    }
    keys.iter()
        .map(|key| {
            format!(
                "Keyslot {}, which asks for {} at boot.",
                key.slot,
                key.asks()
            )
        })
        .collect()
}

/// Which security keys open the drive, oldest keyslot first.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, when the caller is neither the owner nor root,
/// or when the header could not be read.
#[cfg(feature = "bus")]
pub fn security_keys() -> Result<Vec<SecurityKey>, String> {
    let vault = Component::Vault;
    let connection = bus::connect(KEYS_TIMEOUT)?;
    let keys: Vec<(u32, bool, bool)> = bus::proxy(&connection, vault)?
        .call("SecurityKeys", &())
        .map_err(|e| bus::sentence(vault, e))?;
    Ok(keys
        .into_iter()
        .map(|(slot, pin, presence)| SecurityKey {
            slot,
            pin,
            presence,
        })
        .collect())
}

/// Takes the security key in `slot` off the drive. The passphrase slot is never one of these, so
/// the drive still opens with it.
///
/// # Errors
///
/// A sentence when the bus or Vault is not there, when the caller is neither the owner nor root,
/// or when that keyslot holds no security key.
#[cfg(feature = "bus")]
pub fn remove_security_key(slot: u32) -> Result<(), String> {
    let vault = Component::Vault;
    let connection = bus::connect(KEYS_TIMEOUT)?;
    bus::proxy(&connection, vault)?
        .call("RemoveSecurityKey", &(slot))
        .map_err(|e| bus::sentence(vault, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_security_key_says_what_it_asks_for() {
        let key = |pin, presence| SecurityKey {
            slot: 2,
            pin,
            presence,
        };
        assert_eq!(key(true, true).asks(), "its PIN and a touch");
        assert_eq!(key(true, false).asks(), "its PIN");
        assert_eq!(key(false, true).asks(), "a touch");
        assert_eq!(key(false, false).asks(), "nothing");
        assert_eq!(keys_read_as(&[]), ["No security key opens this drive."]);
        assert_eq!(
            keys_read_as(&[
                key(true, true),
                SecurityKey {
                    slot: 3,
                    pin: false,
                    presence: true
                }
            ]),
            [
                "Keyslot 2, which asks for its PIN and a touch at boot.",
                "Keyslot 3, which asks for a touch at boot."
            ]
        );
        assert!(not_a_security_key(7).contains("Keyslot 7"));
        assert!(not_a_security_key(7).contains("rift host keys"));
    }

    #[test]
    fn an_auto_unlock_word_reads_as_a_state() {
        assert_eq!(auto_unlock_state("off", ""), Some(AutoUnlock::Off));
        assert_eq!(auto_unlock_state("on", ""), Some(AutoUnlock::Here));
        assert_eq!(
            auto_unlock_state("elsewhere", "5297c0f65d6a"),
            Some(AutoUnlock::Elsewhere("5297c0f65d6a".into()))
        );
        assert_eq!(auto_unlock_state("sealed", ""), None);
        assert!(AutoUnlock::Here.here());
        assert!(!AutoUnlock::Off.here());
        assert!(!AutoUnlock::Elsewhere(String::new()).here());
    }

    #[test]
    fn outcomes_are_read_by_name() {
        assert_eq!(read("restored"), Some(Restored::Restored));
        assert_eq!(read("replaced"), Some(Restored::Replaced));
        assert_eq!(read("unchanged"), Some(Restored::Unchanged));
        assert_eq!(read("deleted"), None);
        assert_eq!(read(""), None);
    }

    #[test]
    fn a_path_in_home_has_a_place_in_every_snapshot() {
        let at = "2026-09-12T14:00:03Z";
        assert_eq!(
            in_snapshot(at, Path::new("/home/rift/Documents")),
            Some(PathBuf::from(
                "/persist/@snapshots/home/2026-09-12T14:00:03Z/rift/Documents"
            ))
        );
        assert_eq!(
            in_snapshot(at, Path::new("/home")),
            Some(PathBuf::from(
                "/persist/@snapshots/home/2026-09-12T14:00:03Z"
            ))
        );
        assert_eq!(in_snapshot(at, Path::new("/etc/passwd")), None);
        assert_eq!(in_snapshot(at, Path::new("/home/rift/../root")), None);
        assert_eq!(in_snapshot("yesterday", Path::new("/home/rift")), None);
    }

    #[test]
    fn a_backup_is_shown_by_its_first_eight_digits() {
        assert_eq!(
            short("e863e83c77b4f162be953870ddaaf7ff70f92ab02e129fa01b3752e0495b489d"),
            "e863e83c"
        );
        assert_eq!(short("e863"), "e863");
    }

    #[cfg(feature = "bus")]
    #[test]
    fn only_file_exists_is_a_changed_file() {
        let exists = zbus::Error::FDO(Box::new(zbus::fdo::Error::FileExists(
            "/home/rift/notes.txt has changed since this snapshot.".into(),
        )));
        assert_eq!(
            changed(&exists).as_deref(),
            Some("/home/rift/notes.txt has changed since this snapshot.")
        );
        let missing = zbus::Error::FDO(Box::new(zbus::fdo::Error::FileNotFound(
            "There is no snapshot 2026-09-12T14:00:03Z.".into(),
        )));
        assert_eq!(changed(&missing), None);
    }
}
