//! Security keys: a FIDO2 key that opens persist beside its passphrase, never in place of it.
//!
//! systemd-cryptenroll puts the key's credential in a token of its own in the LUKS header, the way
//! it does for a tpm, and cryptsetup's own token plugin uses it in the initrd with nothing in
//! crypttab to say so. A header with no such token is opened with the passphrase as always.
//!
//! Unlike a key sealed to a tpm, a security key is not a property of a machine: it is a thing the
//! owner carries with the drive, so the header is the whole record and nothing is written beside
//! it. Several keys may be enrolled, which is how a person with one on their keyring and one in a
//! drawer owns a security key, and each is the keyslot its own key sits in.
//!
//! Enrolling one is interactive: the key is touched and its pin is typed while cryptenroll waits.
//! That is why `vault enroll-key` runs as root in a terminal and inherits it, rather than
//! answering on the bus the way sealing to a tpm does. Reading the header and taking a key off it
//! need no person, so those two are on the bus and the Owner page uses them.

use std::fs;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use librift::vault::SecurityKey;

/// The type systemd-cryptenroll gives its fido2 token in the header.
const TOKEN: &str = "systemd-fido2";

/// Why a security key could not be enrolled or removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing that speaks FIDO2 is plugged in.
    NoKey,
    /// The passphrase does not open persist.
    Wrong,
    /// That keyslot holds no security key. Holds the keyslot.
    NotAKey(u32),
    /// Anything else, as a sentence.
    Failed(String),
}

impl Refusal {
    /// The sentence a person reads.
    #[must_use]
    pub fn why(&self) -> String {
        match self {
            Refusal::NoKey => librift::vault::NO_SECURITY_KEY.to_string(),
            Refusal::Wrong => "That is not this drive's passphrase.".to_string(),
            Refusal::NotAKey(slot) => librift::vault::not_a_security_key(*slot),
            Refusal::Failed(why) => why.clone(),
        }
    }
}

/// The security keys of one encrypted partition, read and written as root.
#[derive(Debug, Clone)]
pub struct Keys {
    /// The partition persist is on.
    device: PathBuf,
    /// A directory on a tmpfs for the passphrase while systemd-cryptenroll reads it.
    run: PathBuf,
}

impl Keys {
    /// The security keys of the partition at `device`, with the passphrase handed over through
    /// `run`.
    #[must_use]
    pub fn new(device: &Path, run: &Path) -> Keys {
        Keys {
            device: device.to_path_buf(),
            run: run.to_path_buf(),
        }
    }

    /// Which security keys open the drive, lowest keyslot first.
    ///
    /// # Errors
    ///
    /// A sentence when the header could not be read.
    pub fn list(&self) -> Result<Vec<SecurityKey>, String> {
        Ok(keys_in(&self.header()?))
    }

    /// Enrolls the security key that is plugged in, so it opens the drive beside the passphrase.
    ///
    /// Takes the passphrase of a slot that already opens the drive, which is how cryptenroll reads
    /// the volume key, and the terminal, which is where the key is touched and its pin is typed.
    /// `say` gets a line to print before the key is asked for anything.
    ///
    /// # Errors
    ///
    /// [`Refusal::NoKey`] when nothing is plugged in, [`Refusal::Wrong`] when the passphrase does
    /// not open the drive, otherwise a sentence.
    pub fn enroll(&self, passphrase: &str, say: impl FnOnce()) -> Result<(), Refusal> {
        if plugged_in()?.is_empty() {
            return Err(Refusal::NoKey);
        }
        let key = self.key_file(passphrase)?;
        // the passphrase is checked here rather than left to cryptenroll, so a person is not asked
        // to touch their key and only then told the passphrase was wrong
        if !self.passphrase_opens(&key.path)? {
            return Err(Refusal::Wrong);
        }
        say();
        let enrolled = Command::new("systemd-cryptenroll")
            // the terminal is the point: cryptenroll asks for the key's pin on it and waits there
            // for the key to be touched
            .env("SYSTEMD_EMOJI", "0")
            .arg(format!("--unlock-key-file={}", key.path.display()))
            .arg("--fido2-device=auto")
            // a key alone is something you have. its pin is something you know, and the two
            // together are what a security key is for. a key with no pin set falls back to a
            // touch, which systemd says at the time
            .arg("--fido2-with-client-pin=yes")
            .arg("--fido2-with-user-presence=yes")
            .arg("--fido2-with-user-verification=no")
            .arg(&self.device)
            .status()
            .map_err(|e| Refusal::Failed(format!("Could not run systemd-cryptenroll: {e}")))?;
        drop(key);
        if !enrolled.success() {
            return Err(Refusal::Failed(
                "The security key was not added, and the drive is as it was.".to_string(),
            ));
        }
        Ok(())
    }

    /// Takes the security key in `slot` off the drive.
    ///
    /// # Errors
    ///
    /// [`Refusal::NotAKey`] when that keyslot holds no security key, otherwise a sentence.
    pub fn remove(&self, slot: u32) -> Result<(), Refusal> {
        let header = self.header().map_err(Refusal::Failed)?;
        if !keys_in(&header).iter().any(|key| key.slot == slot) {
            return Err(Refusal::NotAKey(slot));
        }
        let wiped = Command::new("systemd-cryptenroll")
            .stdin(Stdio::null())
            .arg(format!("--wipe-slot={slot}"))
            .arg(&self.device)
            .output()
            .map_err(|e| Refusal::Failed(format!("Could not run systemd-cryptenroll: {e}")))?;
        if !wiped.status.success() {
            return Err(Refusal::Failed(format!(
                "Could not take the security key in keyslot {slot} off the drive: {}",
                String::from_utf8_lossy(&wiped.stderr).trim()
            )));
        }
        // a token whose last keyslot is gone opens nothing, and whether libcryptsetup takes it out
        // of the header itself is its business. this makes sure the header says what is true
        self.forget_orphans()
    }

    /// The header of persist as json.
    fn header(&self) -> Result<String, String> {
        let dumped = Command::new("cryptsetup")
            .stdin(Stdio::null())
            .args(["luksDump", "--dump-json-metadata"])
            .arg(&self.device)
            .output()
            .map_err(|e| format!("Could not run cryptsetup: {e}"))?;
        if !dumped.status.success() {
            return Err(format!(
                "Could not read the header of {}: {}",
                self.device.display(),
                String::from_utf8_lossy(&dumped.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&dumped.stdout).into_owned())
    }

    /// Whether the bytes in the file at `path` open persist.
    fn passphrase_opens(&self, path: &Path) -> Result<bool, Refusal> {
        let tested = Command::new("cryptsetup")
            .stdin(Stdio::null())
            .arg("open")
            .arg("--test-passphrase")
            .arg("--key-file")
            .arg(path)
            .arg(&self.device)
            .output()
            .map_err(|e| Refusal::Failed(format!("Could not run cryptsetup: {e}")))?;
        Ok(tested.status.success())
    }

    /// Takes out any security key token the header still holds that opens no keyslot.
    fn forget_orphans(&self) -> Result<(), Refusal> {
        let header = self.header().map_err(Refusal::Failed)?;
        for id in orphans_in(&header) {
            let removed = Command::new("cryptsetup")
                .stdin(Stdio::null())
                .args(["token", "remove", "--token-id"])
                .arg(id.to_string())
                .arg(&self.device)
                .output()
                .map_err(|e| Refusal::Failed(format!("Could not run cryptsetup: {e}")))?;
            if !removed.status.success() {
                return Err(Refusal::Failed(format!(
                    "The security key is off the drive, but token {id} is still in the header: {}",
                    String::from_utf8_lossy(&removed.stderr).trim()
                )));
            }
        }
        Ok(())
    }

    /// The passphrase in a file on a tmpfs, root's alone, as the exact bytes persist was made
    /// with. cryptenroll takes it there and not on a pipe.
    fn key_file(&self, passphrase: &str) -> Result<KeyFile, Refusal> {
        fs::create_dir_all(&self.run)
            .map_err(|e| Refusal::Failed(format!("Could not make {}: {e}", self.run.display())))?;
        let path = self.run.join("security-key.key");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| Refusal::Failed(format!("Could not make {}: {e}", path.display())))?;
        io::Write::write_all(&mut file, passphrase.as_bytes())
            .map_err(|e| Refusal::Failed(format!("Could not write {}: {e}", path.display())))?;
        Ok(KeyFile { path })
    }
}

/// The device paths systemd-cryptenroll lists, which is what a security key comes up as. It says
/// so on stderr and leaves stdout empty when there is none, and it needs no root to answer.
///
/// An empty list is no key: a build with no FIDO2 support, a list that failed and nothing plugged
/// in all mean the same thing to a person, which is that there is nothing to enroll.
///
/// # Errors
///
/// A sentence when systemd-cryptenroll could not be run at all.
pub fn plugged_in() -> Result<Vec<String>, Refusal> {
    let listed = Command::new("systemd-cryptenroll")
        .stdin(Stdio::null())
        .arg("--fido2-device=list")
        .output()
        .map_err(|e| Refusal::Failed(format!("Could not run systemd-cryptenroll: {e}")))?;
    if !listed.status.success() {
        return Ok(Vec::new());
    }
    Ok(devices_in(&String::from_utf8_lossy(&listed.stdout)))
}

/// The passphrase on a tmpfs while cryptenroll reads it, gone as soon as it is dropped.
struct KeyFile {
    path: PathBuf,
}

impl Drop for KeyFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// The security keys a header dumped as json holds, lowest keyslot first.
fn keys_in(json: &str) -> Vec<SecurityKey> {
    let mut keys: Vec<SecurityKey> = tokens_in(json)
        .filter_map(|(_, token)| {
            Some(SecurityKey {
                slot: first_keyslot(&token)?,
                pin: flag(&token, "fido2-clientPin-required"),
                presence: flag(&token, "fido2-up-required"),
            })
        })
        .collect();
    keys.sort_by_key(|key| key.slot);
    keys
}

/// The ids of the security key tokens a header holds that open no keyslot.
fn orphans_in(json: &str) -> Vec<u32> {
    let mut ids: Vec<u32> = tokens_in(json)
        .filter(|(_, token)| first_keyslot(token).is_none())
        .filter_map(|(id, _)| id.parse().ok())
        .collect();
    ids.sort_unstable();
    ids
}

/// Every security key token in a header dumped as json, by its id.
fn tokens_in(json: &str) -> impl Iterator<Item = (String, serde_json::Value)> {
    let tokens = serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|dumped| match dumped.get("tokens") {
            Some(serde_json::Value::Object(tokens)) => Some(tokens.clone()),
            _ => None,
        })
        .unwrap_or_default();
    tokens
        .into_iter()
        .filter(|(_, token)| token.get("type").and_then(serde_json::Value::as_str) == Some(TOKEN))
}

/// The first keyslot a token opens, when it opens one.
fn first_keyslot(token: &serde_json::Value) -> Option<u32> {
    token
        .get("keyslots")?
        .as_array()?
        .iter()
        .filter_map(|slot| slot.as_str()?.parse().ok())
        .min()
}

/// One of a token's booleans, false when it is not there.
fn flag(token: &serde_json::Value, name: &str) -> bool {
    token
        .get(name)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// The device paths in what systemd-cryptenroll lists. Its table puts the path first, and a run
/// that found nothing prints a sentence with no path in it.
fn devices_in(listed: &str) -> Vec<String> {
    listed
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|word| word.starts_with("/dev/"))
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_KEYS: &str = r#"{
      "keyslots": { "0": { "type": "luks2" }, "1": { "type": "luks2" }, "3": { "type": "luks2" } },
      "tokens": {
        "0": {
          "type": "systemd-fido2",
          "keyslots": ["1"],
          "fido2-rp": "io.systemd.cryptsetup",
          "fido2-clientPin-required": true,
          "fido2-up-required": true,
          "fido2-uv-required": false
        },
        "1": { "type": "systemd-tpm2", "keyslots": ["2"], "tpm2-pcrs": [7] },
        "2": {
          "type": "systemd-fido2",
          "keyslots": ["3"],
          "fido2-clientPin-required": false,
          "fido2-up-required": true
        }
      },
      "segments": {}
    }"#;

    const PASSPHRASE_ONLY: &str = r#"{
      "keyslots": { "0": { "type": "luks2" } },
      "tokens": {},
      "segments": {}
    }"#;

    #[test]
    fn a_security_key_is_the_token_cryptenroll_writes() {
        assert_eq!(
            keys_in(TWO_KEYS),
            [
                SecurityKey {
                    slot: 1,
                    pin: true,
                    presence: true
                },
                SecurityKey {
                    slot: 3,
                    pin: false,
                    presence: true
                }
            ]
        );
        assert!(keys_in(PASSPHRASE_ONLY).is_empty());
        assert!(keys_in("not json").is_empty());
        // the tpm half's token is not one of these, and this one is not its
        let tpm = TWO_KEYS.replace("systemd-fido2", "systemd-luks2-nonsense");
        assert!(keys_in(&tpm).is_empty());
    }

    #[test]
    fn a_token_that_opens_nothing_is_an_orphan() {
        assert!(orphans_in(TWO_KEYS).is_empty());
        let wiped = TWO_KEYS.replace(r#""keyslots": ["3"]"#, r#""keyslots": []"#);
        assert_eq!(orphans_in(&wiped), [2]);
        assert_eq!(keys_in(&wiped).len(), 1);
        assert!(orphans_in(PASSPHRASE_ONLY).is_empty());
    }

    #[test]
    fn a_listed_device_is_a_path() {
        // what the table looks like with a key in it
        let listed = "/dev/hidraw0 Yubico YubiKey OTP+FIDO+CCID\n";
        assert_eq!(devices_in(listed), ["/dev/hidraw0"]);
        // and with none, whichever way it says so
        assert!(devices_in("No FIDO2 devices found.\n").is_empty());
        assert!(devices_in("").is_empty());
        assert!(devices_in("PATH MANUFACTURER PRODUCT\n").is_empty());
    }

    #[test]
    fn a_refusal_says_what_to_do_about_it() {
        assert!(Refusal::NoKey.why().contains("Plug one in"));
        assert!(Refusal::Wrong.why().contains("passphrase"));
        assert!(Refusal::NotAKey(4).why().contains("Keyslot 4"));
        assert_eq!(Refusal::Failed("nope".into()).why(), "nope");
    }
}
