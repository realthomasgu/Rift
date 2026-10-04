//! The Unlocking part of the Owner page: what opens the drive besides its passphrase.
//!
//! Two things do. A key sealed to this machine's tpm, which is one machine at a time and is turned
//! on with the drive's passphrase, and any number of security keys, which belong to the drive and
//! work on every machine. Vault answers the owner and root alone for both, so the page asks it as
//! it comes up and after each change, on a thread of its own.
//!
//! Adding a security key is not here. The key is touched and its pin typed while the enrollment
//! waits, and neither can happen on a page, so the row says which command does it.

use std::thread;

use iced::futures::channel::oneshot;
use iced::widget::{column, row};
use iced::{Center, Element, Fill, Task};
use librift::vault::{self, AutoUnlock, SecurityKey};

use crate::ai::said;
use crate::ghost;
use crate::theme::Colors;
use crate::ui::{Message, Settings};
use crate::widgets::{action, fact, field, focus, group, heading, note, setting};

/// The field the drive's passphrase is typed into.
const PASSPHRASE_FIELD: &str = "unlocking-passphrase";

/// What Vault says opens the drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// What the drive does at boot on this machine.
    pub auto: AutoUnlock,
    /// Whether this machine has a tpm at all.
    pub has_tpm: bool,
    /// The security keys that open the drive, lowest keyslot first.
    pub keys: Vec<Key>,
}

/// One security key as the page draws it. The keyslot is its name, because systemd's token holds
/// none, and the sentences are kept here because a row borrows its label for as long as it is
/// drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    /// The keyslot it opens, which is what removes it.
    pub slot: u32,
    /// Its name on the page, `Keyslot 2`.
    pub name: String,
    /// What it asks for at boot, under the name.
    pub asks: String,
}

impl Key {
    /// One of Vault's keys as a row.
    #[must_use]
    pub fn new(key: SecurityKey) -> Key {
        Key {
            slot: key.slot,
            name: format!("Keyslot {}", key.slot),
            asks: format!("It asks for {} at boot.", key.asks()),
        }
    }
}

/// What is typed in this part of the page, and whether a change is on its way to Vault.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Form {
    /// The drive's passphrase, while the field for it is open.
    pub passphrase: Option<String>,
    /// Whether a change is on its way, which holds the buttons until Vault answers.
    pub busy: bool,
}

/// What the owner asked for in this part of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// Open the passphrase field, or close it without a change.
    Open(bool),
    /// The passphrase field was typed into.
    Typed(String),
    /// Seal a key to this machine's tpm with what is in the field.
    TurnOn,
    /// Wipe the key sealed to a tpm, which needs no passphrase.
    TurnOff,
    /// Take the security key in this keyslot off the drive.
    Remove(u32),
    /// How a change went. The page asks Vault again after it.
    Done(Result<(), String>),
}

/// Ask Vault what opens the drive, on a thread of its own. Both answers come from one thread, so
/// the page draws them together rather than a row at a time.
pub fn read() -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(asked_of_vault());
    });
    Task::perform(receiver, |answered| {
        Message::Unlocking(answered.unwrap_or_else(|_| Err(STOPPED.to_string())))
    })
}

/// The two questions, in order. Either one failing is the part of the page failing: they are the
/// same drive.
fn asked_of_vault() -> Result<Picture, String> {
    let (auto, has_tpm) = vault::auto_unlock()?;
    Ok(Picture {
        auto,
        has_tpm,
        keys: vault::security_keys()?.into_iter().map(Key::new).collect(),
    })
}

/// Do what the owner asked. A change goes to Vault on a thread of its own, and when it has
/// answered the page asks again, so it shows what the drive holds rather than what was pressed.
pub fn asked(state: &mut Settings, asked: Asked) -> Task<Message> {
    match asked {
        Asked::Open(open) => {
            state.unlocking_form.passphrase = open.then(String::new);
            state.problem = None;
            if open {
                return focus(PASSPHRASE_FIELD);
            }
        }
        Asked::Typed(typed) => {
            if state.unlocking_form.passphrase.is_some() {
                state.unlocking_form.passphrase = Some(typed);
                state.problem = None;
            }
        }
        Asked::TurnOn => return turn_on(state),
        Asked::TurnOff => return change(state, || vault::set_auto_unlock(false, "")),
        Asked::Remove(slot) => return change(state, move || vault::remove_security_key(slot)),
        Asked::Done(done) => {
            state.unlocking_form.busy = false;
            match done {
                Ok(()) => {
                    state.unlocking_form.passphrase = None;
                    state.problem = None;
                }
                Err(why) => state.problem = Some(why),
            }
            return read();
        }
    }
    Task::none()
}

/// Seal a key to this machine's tpm with the passphrase in the field.
fn turn_on(state: &mut Settings) -> Task<Message> {
    let Some(passphrase) = state.unlocking_form.passphrase.clone() else {
        return Task::none();
    };
    if passphrase.is_empty() {
        state.problem = Some(EMPTY.to_string());
        return Task::none();
    }
    change(state, move || vault::set_auto_unlock(true, &passphrase))
}

/// Ask Vault for a change on a thread of its own, and say how it went.
fn change(
    state: &mut Settings,
    change: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Task<Message> {
    if state.unlocking_form.busy {
        return Task::none();
    }
    state.problem = None;
    state.unlocking_form.busy = true;
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(change());
    });
    Task::perform(receiver, |done| {
        Message::Unlock(Asked::Done(
            done.unwrap_or_else(|_| Err(STOPPED.to_string())),
        ))
    })
}

/// The lines `rift-settings --state` prints, once Vault has answered: what opens the drive at boot
/// and how many security keys it takes. `unlocking none` is Vault not answering.
#[must_use]
pub fn state(state: &Settings) -> Vec<String> {
    match &state.unlocking {
        None => Vec::new(),
        Some(Err(_)) => vec!["unlocking none".to_string()],
        Some(Ok(drive)) => {
            let mut lines = vec![
                format!("unlocking-auto {}", word(&drive.auto)),
                format!("unlocking-tpm {}", if drive.has_tpm { "yes" } else { "no" }),
                format!("unlocking-keys {}", drive.keys.len()),
            ];
            for key in &drive.keys {
                lines.push(format!("unlocking-key {}", key.slot));
            }
            lines
        }
    }
}

/// The word `--state` prints for what the drive does at boot.
fn word(auto: &AutoUnlock) -> &'static str {
    match auto {
        AutoUnlock::Off => "off",
        AutoUnlock::Here => "on",
        AutoUnlock::Elsewhere(_) => "elsewhere",
    }
}

/// What `rift-settings --set` asks of this part of the page: `auto-unlock <passphrase>` opens the
/// field, types the passphrase and seals a key, `auto-unlock off` wipes the one there is, and
/// `remove-key <keyslot>` takes a security key off. The steps go in that order, which a batch does
/// not promise.
pub fn named(name: &str, value: &str) -> Task<Message> {
    steps(name, value)
        .into_iter()
        .map(|asked| Task::done(Message::Unlock(asked)))
        .reduce(Task::chain)
        .unwrap_or_else(Task::none)
}

/// What one `--set` does to this part of the page, in order.
fn steps(name: &str, value: &str) -> Vec<Asked> {
    match name {
        "auto-unlock" if value.trim() == "off" => vec![Asked::TurnOff],
        "auto-unlock" => vec![
            Asked::Open(true),
            Asked::Typed(value.trim().to_string()),
            Asked::TurnOn,
        ],
        "remove-key" => value
            .trim()
            .parse()
            .map(|slot| vec![Asked::Remove(slot)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The Unlocking part of the page: the passphrase, the tpm and the security keys.
pub fn view(state: &Settings, look: Colors) -> Element<'_, Message> {
    let inside = if ghost::on() {
        // nothing can be sealed to a tpm and no security key can be added or taken off while
        // persist is locked, and the header is not read to say what is there already either: the
        // note that says which machine holds a key is on persist, so the answer would be half of
        // one (ADR-0084)
        vec![ghost::row(look, "Unlocking", NOT_IN_GHOST_MODE)]
    } else {
        match &state.unlocking {
            None => vec![fact(
                look,
                "Unlocking",
                "Asking Vault about the drive.".into(),
            )],
            Some(Err(why)) => vec![fact(look, "Unlocking", why.clone())],
            Some(Ok(drive)) => rows(state, look, drive),
        }
    };
    column![
        heading(look, "Unlocking the drive"),
        group(look, inside),
        note(look, PASSPHRASE),
    ]
    .spacing(8)
    .width(Fill)
    .into()
}

/// The rows, once Vault has said what the drive holds.
fn rows<'a>(state: &'a Settings, look: Colors, drive: &'a Picture) -> Vec<Element<'a, Message>> {
    let busy = state.unlocking_form.busy;
    let mut rows = vec![setting(
        look,
        "Start without the passphrase",
        Some(automatic(drive)),
        match (&drive.auto, drive.has_tpm) {
            (AutoUnlock::Here, _) => action(
                look,
                "Turn off",
                (!busy).then_some(Message::Unlock(Asked::TurnOff)),
            ),
            (_, false) => said(look, "No tpm"),
            // while the field below is open the button is there and dimmed, so the row does not
            // change shape under the hand that pressed it
            _ => action(
                look,
                "Turn on",
                (!busy && state.unlocking_form.passphrase.is_none())
                    .then_some(Message::Unlock(Asked::Open(true))),
            ),
        },
    )];
    if let Some(typed) = &state.unlocking_form.passphrase {
        rows.push(setting(
            look,
            "Drive passphrase",
            Some(if busy { SEALING } else { TYPE_IT }),
            row![
                field(
                    look,
                    "Passphrase",
                    typed,
                    true,
                    PASSPHRASE_FIELD,
                    |typed| Message::Unlock(Asked::Typed(typed)),
                    Message::Unlock(Asked::TurnOn),
                ),
                action(
                    look,
                    "Cancel",
                    (!busy).then_some(Message::Unlock(Asked::Open(false)))
                ),
                action(
                    look,
                    "Turn on",
                    (!busy && !typed.is_empty()).then_some(Message::Unlock(Asked::TurnOn))
                ),
            ]
            .spacing(8)
            .align_y(Center),
        ));
    }
    rows.push(setting(
        look,
        "Security keys",
        Some(KEYS),
        said(look, how_many(drive.keys.len())),
    ));
    for key in &drive.keys {
        rows.push(setting(
            look,
            &key.name,
            Some(&key.asks),
            action(
                look,
                "Remove",
                (!busy).then_some(Message::Unlock(Asked::Remove(key.slot))),
            ),
        ));
    }
    rows
}

/// The sentence under the tpm row, which says what the drive does now.
fn automatic(drive: &Picture) -> &'static str {
    match (&drive.auto, drive.has_tpm) {
        (AutoUnlock::Here, _) => HERE,
        (AutoUnlock::Elsewhere(_), true) => ELSEWHERE,
        (AutoUnlock::Elsewhere(_), false) => ELSEWHERE_NO_TPM,
        (AutoUnlock::Off, true) => OFF,
        (AutoUnlock::Off, false) => NO_TPM,
    }
}

/// How many security keys open the drive, as a word.
fn how_many(keys: usize) -> &'static str {
    match keys {
        0 => "None",
        1 => "One",
        2 => "Two",
        3 => "Three",
        _ => "Several",
    }
}

/// What a thread that stopped before it answered says.
const STOPPED: &str = "It stopped before it finished.";
/// What a Ghost boot cannot do about what opens the drive.
const NOT_IN_GHOST_MODE: &str = "What opens the drive cannot be read or changed";
/// Under the tpm row while a key is sealed to this machine.
const HERE: &str = "This machine's tpm holds a key for the drive, so the drive starts here with \
                    nothing typed. Somebody who takes both can start it.";
/// And while the key is on another machine.
const ELSEWHERE: &str = "Another machine's tpm holds a key for the drive. Turning it on here takes \
                         it off that machine.";
/// And when that machine is not this one and this one has no tpm to hold it instead.
const ELSEWHERE_NO_TPM: &str = "Another machine's tpm holds a key for the drive. This machine has \
                                no tpm, so the drive asks for its passphrase here.";
/// And while no machine holds one.
const OFF: &str = "A machine you have said is yours can hold a key for the drive in its tpm and \
                   start it with nothing typed.";
/// And on a machine with no tpm.
const NO_TPM: &str = "This machine has no tpm, so there is nothing to hold a key for the drive.";
/// Beside the passphrase field.
const TYPE_IT: &str = "The key is made from the passphrase that already opens the drive.";
/// And while Vault is sealing it.
const SEALING: &str = "Sealing a key to this machine's tpm.";
/// Under the security keys row.
const KEYS: &str = "A security key opens the drive on every machine, and any number of them can. \
                    The drive's passphrase opens it as well, with a key or without one.";
/// What is said when the field is empty.
const EMPTY: &str = "A key is sealed with the passphrase that already opens the drive, so it \
                     cannot be empty.";
/// Under the whole part: the passphrase itself, and how a security key is added.
const PASSPHRASE: &str = "The drive's passphrase always opens it, here and on every other machine, \
                          and none of this changes that. sudo cryptsetup luksChangeKey \
                          /dev/disk/by-partlabel/persist changes the passphrase in a terminal, and \
                          sudo rift host enroll-key adds a security key there: the key is touched \
                          and its PIN typed while it waits.";

#[cfg(test)]
mod tests {
    use super::*;

    fn key(slot: u32) -> Key {
        Key::new(SecurityKey {
            slot,
            pin: true,
            presence: true,
        })
    }

    fn settings(auto: AutoUnlock, has_tpm: bool, keys: Vec<Key>) -> Settings {
        let mut state = Settings::bare();
        state.unlocking = Some(Ok(Picture {
            auto,
            has_tpm,
            keys,
        }));
        state
    }

    #[test]
    fn the_state_says_what_opens_the_drive() {
        assert!(state(&Settings::bare()).is_empty());
        assert_eq!(
            state(&settings(AutoUnlock::Off, true, Vec::new())),
            [
                "unlocking-auto off",
                "unlocking-tpm yes",
                "unlocking-keys 0"
            ]
        );
        assert_eq!(
            state(&settings(AutoUnlock::Here, true, vec![key(2), key(3)])),
            [
                "unlocking-auto on",
                "unlocking-tpm yes",
                "unlocking-keys 2",
                "unlocking-key 2",
                "unlocking-key 3"
            ]
        );
        assert_eq!(
            state(&settings(
                AutoUnlock::Elsewhere("5297c0f65d6a".into()),
                false,
                Vec::new()
            ))[..2],
            ["unlocking-auto elsewhere", "unlocking-tpm no"]
        );
        let mut unanswered = Settings::bare();
        unanswered.unlocking = Some(Err("Vault is not running.".to_string()));
        assert_eq!(state(&unanswered), ["unlocking none"]);
    }

    #[test]
    fn the_passphrase_field_opens_and_closes() {
        let mut state = settings(AutoUnlock::Off, true, Vec::new());
        let _ = asked(&mut state, Asked::Open(true));
        assert_eq!(state.unlocking_form.passphrase.as_deref(), Some(""));
        // an empty field is said and not sent
        let _ = asked(&mut state, Asked::TurnOn);
        assert!(!state.unlocking_form.busy);
        assert_eq!(state.problem.as_deref(), Some(EMPTY));
        let _ = asked(&mut state, Asked::Typed("rift-test".into()));
        assert_eq!(state.problem, None);
        // closing it forgets what was typed
        let _ = asked(&mut state, Asked::Open(false));
        assert_eq!(state.unlocking_form.passphrase, None);
    }

    #[test]
    fn a_change_that_went_through_closes_the_field() {
        let mut state = settings(AutoUnlock::Off, true, Vec::new());
        state.unlocking_form = Form {
            passphrase: Some("rift-test".into()),
            busy: true,
        };
        let _ = asked(&mut state, Asked::Done(Ok(())));
        assert_eq!(state.unlocking_form, Form::default());
        state.unlocking_form.busy = true;
        let _ = asked(
            &mut state,
            Asked::Done(Err("That is not this drive's passphrase.".into())),
        );
        assert!(!state.unlocking_form.busy);
        assert_eq!(
            state.problem.as_deref(),
            Some("That is not this drive's passphrase.")
        );
    }

    #[test]
    fn a_set_names_the_steps_in_order() {
        // the field is opened, typed into and pressed, in that order, which a batch does not promise
        assert_eq!(
            steps("auto-unlock", " rift-test "),
            [
                Asked::Open(true),
                Asked::Typed("rift-test".into()),
                Asked::TurnOn
            ]
        );
        assert_eq!(steps("auto-unlock", "off"), [Asked::TurnOff]);
        assert_eq!(steps("remove-key", " 2 "), [Asked::Remove(2)]);
        assert!(steps("remove-key", "the one in the drawer").is_empty());
        assert!(steps("nothing", "2").is_empty());
    }

    #[test]
    fn how_many_keys_reads_as_a_word() {
        assert_eq!(how_many(0), "None");
        assert_eq!(how_many(1), "One");
        assert_eq!(how_many(3), "Three");
        assert_eq!(how_many(9), "Several");
    }

    #[test]
    fn the_sentences_are_sentences() {
        for sentence in [
            STOPPED,
            HERE,
            ELSEWHERE,
            ELSEWHERE_NO_TPM,
            OFF,
            NO_TPM,
            TYPE_IT,
            SEALING,
            KEYS,
            EMPTY,
            PASSPHRASE,
        ] {
            assert!(sentence.ends_with('.'), "{sentence}");
            assert!(sentence.is_ascii(), "{sentence}");
        }
    }
}
