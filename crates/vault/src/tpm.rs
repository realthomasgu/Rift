//! Auto-unlock: a key for persist sealed to a machine's tpm, so the drive opens on that machine
//! without its passphrase.
//!
//! systemd-cryptenroll puts the sealed key in a token of its own in the LUKS header, beside the
//! passphrase and never in place of it, and cryptsetup's own token plugin unseals it in the initrd
//! with nothing in crypttab to say so. When there is no tpm, or the tpm cannot unseal what is in
//! the header, the passphrase is asked for the way it always has been.
//!
//! The key is sealed to pcr 7 alone, and one machine holds it at a time. Which machine that is is
//! kept beside it, since the header cannot say: the sealed key is a blob only that machine's tpm
//! can open, and nothing short of trying tells you whose it is.

use std::fs;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Which pcr the key is sealed to. 7 is the firmware's record of the secure boot policy, and the
/// one measurement that does not change when the image is updated. Sealing to 11, which measures
/// the kernel and the initrd, would stop the drive opening at every update until the uki is signed
/// and the policy is signed with it.
const PCRS: &str = "7";

/// The type systemd-cryptenroll gives its tpm2 token in the header.
const TOKEN: &str = "systemd-tpm2";

/// Where the kernel puts the tpms it found.
const TPMS: &str = "/sys/class/tpm";

/// What the drive's auto-unlock is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Nothing is sealed to any tpm. Persist opens with its passphrase and nothing else.
    Off,
    /// A key is sealed to this machine's tpm.
    Here,
    /// A key is sealed to a tpm, and the machine it was sealed on is not this one. Holds that
    /// machine's fingerprint.
    Elsewhere(String),
}

impl State {
    /// The word for the bus: `on`, `off` or `elsewhere`.
    #[must_use]
    pub fn word(&self) -> &'static str {
        match self {
            State::Off => "off",
            State::Here => "on",
            State::Elsewhere(_) => "elsewhere",
        }
    }

    /// Which machine holds the key, or an empty string when no machine does.
    #[must_use]
    pub fn machine(&self) -> &str {
        match self {
            State::Elsewhere(fingerprint) => fingerprint,
            State::Off | State::Here => "",
        }
    }
}

/// Why auto-unlock could not be turned on or off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// This machine is not the owner's: its class is `trusted` or `borrowed`. Holds the class.
    NotOwned(String),
    /// The machine has no tpm.
    NoTpm,
    /// The passphrase does not open persist.
    Wrong(String),
    /// Anything else, as a sentence.
    Failed(String),
}

impl Refusal {
    /// The sentence a person reads.
    #[must_use]
    pub fn why(&self) -> String {
        match self {
            Refusal::NotOwned(class) => librift::vault::not_owned(class),
            Refusal::NoTpm => librift::vault::NO_TPM.to_string(),
            Refusal::Wrong(why) | Refusal::Failed(why) => why.clone(),
        }
    }
}

/// The drive's auto-unlock, read and written as root.
#[derive(Debug, Clone)]
pub struct Sealed {
    /// The partition persist is on.
    device: PathBuf,
    /// The file that says which machine holds the sealed key.
    note: PathBuf,
    /// A directory on a tmpfs for the passphrase while systemd-cryptenroll reads it.
    run: PathBuf,
}

impl Sealed {
    /// Auto-unlock for the partition at `device`, with the note under `state` and the passphrase
    /// handed over through `run`.
    #[must_use]
    pub fn new(device: &Path, state: &Path, run: &Path) -> Sealed {
        Sealed {
            device: device.to_path_buf(),
            note: state.join("auto-unlock"),
            run: run.to_path_buf(),
        }
    }

    /// What the drive's auto-unlock is, for the machine whose fingerprint is `fingerprint`.
    ///
    /// # Errors
    ///
    /// A sentence when the header could not be read.
    pub fn state(&self, fingerprint: &str) -> Result<State, String> {
        if !self.header_has_a_sealed_key()? {
            return Ok(State::Off);
        }
        let held = self.held()?;
        if held.as_deref() == Some(fingerprint) {
            Ok(State::Here)
        } else {
            // a key is in the header and the note does not name this machine. it was sealed
            // somewhere else, or the note was lost with the machine that wrote it
            Ok(State::Elsewhere(held.unwrap_or_default()))
        }
    }

    /// Seals a key for persist to this machine's tpm, so it opens without the passphrase here.
    /// Takes the passphrase of a slot that already opens it, which is how cryptenroll reads the
    /// volume key. A key sealed to another machine before is wiped: one machine at a time.
    ///
    /// # Errors
    ///
    /// [`Refusal::NoTpm`] when the machine has none, [`Refusal::Wrong`] when the passphrase does
    /// not open persist, otherwise a sentence.
    pub fn turn_on(&self, passphrase: &str, fingerprint: &str) -> Result<(), Refusal> {
        if !machine_has_a_tpm() {
            return Err(Refusal::NoTpm);
        }
        let key = self.key_file(passphrase)?;
        let enrolled = Command::new("systemd-cryptenroll")
            // nothing here may ever stop to ask a person something: the passphrase is in the key
            // file and a prompt with no answer would hang the bus call
            .stdin(Stdio::null())
            .arg(format!("--unlock-key-file={}", key.path.display()))
            .arg("--tpm2-device=auto")
            .arg(format!("--tpm2-pcrs={PCRS}"))
            // the new slot is excluded from the wipe, so this replaces the machine that held it
            .arg("--wipe-slot=tpm2")
            .arg(&self.device)
            .output()
            .map_err(|e| Refusal::Failed(format!("Could not run systemd-cryptenroll: {e}")))?;
        drop(key);
        if !enrolled.status.success() {
            let said = String::from_utf8_lossy(&enrolled.stderr).trim().to_string();
            // cryptenroll says this when no slot took the passphrase it was given
            if said.contains("Failed to unlock") || said.contains("No passphrase") {
                return Err(Refusal::Wrong(
                    "That is not this drive's passphrase.".to_string(),
                ));
            }
            return Err(Refusal::Failed(format!(
                "Could not seal a key to this machine's tpm: {said}"
            )));
        }
        self.keep(fingerprint)
            .map_err(|e| Refusal::Failed(format!("The key is sealed but {e}")))
    }

    /// Wipes the sealed key, whichever machine it was sealed on, so the drive asks for its
    /// passphrase again everywhere.
    ///
    /// # Errors
    ///
    /// A sentence when the header could not be written.
    pub fn turn_off(&self) -> Result<(), String> {
        if self.header_has_a_sealed_key()? {
            let wiped = Command::new("systemd-cryptenroll")
                .stdin(Stdio::null())
                .arg("--wipe-slot=tpm2")
                .arg(&self.device)
                .output()
                .map_err(|e| format!("Could not run systemd-cryptenroll: {e}"))?;
            if !wiped.status.success() {
                return Err(format!(
                    "Could not wipe the sealed key: {}",
                    String::from_utf8_lossy(&wiped.stderr).trim()
                ));
            }
        }
        match fs::remove_file(&self.note) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!(
                "Could not forget which machine held the key: {e} at {}",
                self.note.display()
            )),
        }
    }

    /// Whether the header holds a key sealed to a tpm.
    fn header_has_a_sealed_key(&self) -> Result<bool, String> {
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
        let text = String::from_utf8_lossy(&dumped.stdout);
        Ok(sealed_tokens(&text) > 0)
    }

    /// The fingerprint of the machine the note names, when there is one.
    fn held(&self) -> Result<Option<String>, String> {
        match fs::read_to_string(&self.note) {
            Ok(text) => Ok(Some(text.trim().to_string()).filter(|held| !held.is_empty())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("Could not read {}: {e}", self.note.display())),
        }
    }

    /// Writes down which machine holds the sealed key.
    fn keep(&self, fingerprint: &str) -> Result<(), String> {
        if let Some(folder) = self.note.parent() {
            fs::create_dir_all(folder)
                .map_err(|e| format!("could not make {}: {e}", folder.display()))?;
        }
        fs::write(&self.note, format!("{fingerprint}\n"))
            .map_err(|e| format!("could not write {}: {e}", self.note.display()))
    }

    /// The passphrase in a file on a tmpfs, root's alone, as the exact bytes persist was made
    /// with. cryptenroll takes it there and not on a pipe.
    fn key_file(&self, passphrase: &str) -> Result<KeyFile, Refusal> {
        fs::create_dir_all(&self.run)
            .map_err(|e| Refusal::Failed(format!("Could not make {}: {e}", self.run.display())))?;
        let path = self.run.join("auto-unlock.key");
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

/// Whether this machine has a tpm at all.
#[must_use]
pub fn machine_has_a_tpm() -> bool {
    fs::read_dir(TPMS).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("tpm"))
        })
    })
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

/// How many tokens in a header dumped as json are keys sealed to a tpm.
fn sealed_tokens(json: &str) -> usize {
    let Ok(dumped) = serde_json::from_str::<serde_json::Value>(json) else {
        return 0;
    };
    let Some(tokens) = dumped.get("tokens").and_then(serde_json::Value::as_object) else {
        return 0;
    };
    tokens
        .values()
        .filter(|token| token.get("type").and_then(serde_json::Value::as_str) == Some(TOKEN))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = r#"{
      "keyslots": { "0": { "type": "luks2" }, "1": { "type": "luks2" } },
      "tokens": {
        "0": { "type": "systemd-tpm2", "keyslots": ["1"], "tpm2-pcrs": [7] }
      },
      "segments": {}
    }"#;

    const PASSPHRASE_ONLY: &str = r#"{
      "keyslots": { "0": { "type": "luks2" } },
      "tokens": {},
      "segments": {}
    }"#;

    #[test]
    fn a_sealed_key_is_the_token_cryptenroll_writes() {
        assert_eq!(sealed_tokens(HEADER), 1);
        assert_eq!(sealed_tokens(PASSPHRASE_ONLY), 0);
        assert_eq!(sealed_tokens("not json"), 0);
        let fido2 = HEADER.replace("systemd-tpm2", "systemd-fido2");
        assert_eq!(sealed_tokens(&fido2), 0);
    }

    #[test]
    fn the_note_says_whose_machine_the_key_is_on() {
        let work = std::env::temp_dir().join(format!("vault-tpm-{}", std::process::id()));
        let state = work.join("state");
        fs::create_dir_all(&state).unwrap();
        let sealed = Sealed::new(Path::new("/dev/null"), &state, &work.join("run"));
        assert_eq!(sealed.held().unwrap(), None);
        sealed.keep("5297c0f65d6a").unwrap();
        assert_eq!(sealed.held().unwrap().as_deref(), Some("5297c0f65d6a"));
        fs::write(sealed.note.clone(), "  \n").unwrap();
        assert_eq!(sealed.held().unwrap(), None);
        fs::remove_dir_all(&work).unwrap();
    }

    #[test]
    fn a_state_reads_as_a_word_and_a_machine() {
        assert_eq!(State::Off.word(), "off");
        assert_eq!(State::Here.word(), "on");
        let other = State::Elsewhere("5297c0f65d6a".into());
        assert_eq!(other.word(), "elsewhere");
        assert_eq!(other.machine(), "5297c0f65d6a");
        assert_eq!(State::Here.machine(), "");
    }

    #[test]
    fn a_refusal_says_what_to_do_about_it() {
        let borrowed = Refusal::NotOwned("borrowed".into()).why();
        assert!(borrowed.contains("borrowed"));
        assert!(borrowed.contains("rift host set class owned"));
        assert!(Refusal::NoTpm.why().contains("no tpm"));
        assert_eq!(Refusal::Wrong("nope".into()).why(), "nope");
    }
}
