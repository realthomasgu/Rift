//! The Updates page: which version each slot of the drive holds, what the next boot would start,
//! and where updates come from.
//!
//! None of that can be read without root. The esp is mounted for root alone and the labels that
//! say which version is in which slot are on the drive itself, so Vault reads the lot and answers
//! it in one call. That call mounts the esp, so the page asks for it when it comes up rather than
//! when the window opens.

use std::fmt::Write as _;
use std::thread;

use iced::futures::channel::oneshot;
use iced::widget::column;
use iced::{Element, Fill, Task};
use librift::update::{Slot, Slots, where_from};
use librift::vault;

use crate::ai::said;
use crate::ghost;
use crate::theme::Colors;
use crate::ui::{Message, Settings};
use crate::widgets::{GAP, fact, group, heading, note, setting};

/// Ask Vault what this drive holds, on a thread of its own: it mounts the esp and reads the
/// drive's partition table, and the window is not waiting for that.
pub fn read() -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(vault::slots());
    });
    Task::perform(receiver, |answered| {
        Message::Slots(answered.unwrap_or_else(|_| Err("Vault did not answer.".to_string())))
    })
}

/// The lines `rift-settings --state` prints about the drive: the version running and the slot it
/// is in, what each slot holds and how many tries its uki has left, where updates come from and
/// the version waiting there.
#[must_use]
pub fn state(state: &Settings) -> Vec<String> {
    let Some(Ok(slots)) = state.slots.as_ref() else {
        return Vec::new();
    };
    let mut lines = vec![
        format!("version {}", or_none(&slots.running)),
        format!("slot {}", slots.running_slot().unwrap_or("none")),
    ];
    for slot in &slots.slots {
        lines.push(format!("slot-{} {}", slot.slot, or_none(&slot.version)));
        lines.push(format!(
            "tries-{} {}",
            slot.slot,
            slot.tries()
                .map_or_else(|| "none".to_string(), |left| left.to_string())
        ));
    }
    lines.push(format!("updates {}", or_none(where_from(&slots.source))));
    lines.push(format!("waiting {}", slots.newer().unwrap_or("none")));
    lines
}

/// A value for the state, or `none` where there is nothing to say.
fn or_none(value: &str) -> &str {
    if value.is_empty() { "none" } else { value }
}

/// The page.
pub fn view(state: &Settings, look: Colors) -> Element<'_, Message> {
    let mut page = column![].spacing(GAP).width(Fill);
    if ghost::on() {
        // reading the slots means mounting the esp and reading the drive's partition table, and a
        // Ghost boot mounts no part of the drive, so Vault is not asked and the page says so. An
        // update cannot be written into the other slot here either (ADR-0084)
        return page
            .push(
                column![
                    heading(look, "Versions on this drive"),
                    group(look, vec![ghost::row(look, "Slots", NO_SLOTS)]),
                    note(look, TWO_SLOTS),
                ]
                .spacing(8),
            )
            .push(
                column![
                    heading(look, "Updates"),
                    group(look, vec![ghost::row(look, "Installing", NO_UPDATE)]),
                ]
                .spacing(8),
            )
            .push(firmware(look))
            .into();
    }
    match state.slots.as_ref() {
        None => page = page.push(note(look, "Asking Vault what this drive holds.")),
        Some(Err(why)) => {
            page = page.push(note(
                look,
                "Vault is not answering, so what this drive holds is not here.",
            ));
            page = page.push(note(look, why));
        }
        Some(Ok(slots)) => {
            page = page.push(on_the_drive(look, slots));
            page = page.push(waiting(look, slots));
        }
    }
    page.push(firmware(look)).into()
}

/// The two slots, with the version in each and what it has left.
fn on_the_drive<'a>(look: Colors, slots: &Slots) -> Element<'a, Message> {
    let rows = slots
        .slots
        .iter()
        .map(|slot| {
            setting(
                look,
                label(&slot.slot),
                None,
                said(look, &holds(slot, &slots.running)),
            )
        })
        .collect();
    column![
        heading(look, "Versions on this drive"),
        group(look, rows),
        note(look, TWO_SLOTS),
        note(look, TRIES),
    ]
    .spacing(8)
    .into()
}

/// Where updates come from and whether one is waiting there.
fn waiting<'a>(look: Colors, slots: &Slots) -> Element<'a, Message> {
    let mut rows = vec![if slots.source.is_empty() {
        fact(
            look,
            "From",
            "Nothing is set up to bring updates in.".to_string(),
        )
    } else {
        setting(look, "From", None, said(look, where_from(&slots.source)))
    }];
    if !slots.source.is_empty() {
        rows.push(setting(
            look,
            "Waiting",
            None,
            said(
                look,
                &slots.newer().map_or_else(
                    || "Nothing newer than this drive holds".to_string(),
                    ToString::to_string,
                ),
            ),
        ));
    }
    column![
        heading(look, "Updates"),
        group(look, rows),
        note(look, INSTALLING)
    ]
    .spacing(8)
    .into()
}

/// Firmware, which is the machine's own and not the drive's.
fn firmware<'a>(look: Colors) -> Element<'a, Message> {
    column![heading(look, "Firmware"), note(look, FIRMWARE)]
        .spacing(8)
        .into()
}

/// The name of a slot's row.
fn label(slot: &str) -> &'static str {
    match slot {
        "a" => "Slot A",
        "b" => "Slot B",
        _ => "Slot",
    }
}

/// What a slot's row says: the version in it, whether it is the one running, and how many tries
/// systemd-boot has left to start it.
fn holds(slot: &Slot, running: &str) -> String {
    if !slot.filled() {
        return "Empty".to_string();
    }
    let mut said = slot.version.clone();
    if slot.version == running {
        said.push_str(", running now");
    }
    match slot.tries() {
        Some(1) => said.push_str(", 1 try left"),
        Some(left) => {
            let _ = write!(said, ", {left} tries left");
        }
        None => {}
    }
    if slot.uki.is_empty() {
        said.push_str(", with nothing on the boot partition to start it");
    }
    said
}

/// What the two slots are for.
const TWO_SLOTS: &str = "The drive keeps two versions. An update is written into the slot that is \
                         not running, so the version you are on now stays where it is.";
/// What the tries mean, and what happens after the last one.
const TRIES: &str = "The next boot starts the newest version that still has a try left. A new \
                     version gets three, and a start that does not reach the desktop takes one \
                     off, so a version that will not run gives way to the one in the other slot.";
/// How an update is installed today, and why the page does not do it.
const INSTALLING: &str = "Installing an update is sudo systemd-sysupdate from a terminal for now. \
                          Settings will do it once updates come from a channel with a signature \
                          this drive knows.";
/// What a Ghost boot cannot do with the drive's slots and an update.
const NO_SLOTS: &str = "What this drive holds cannot be read";
const NO_UPDATE: &str = "An update cannot be brought in or installed";

/// Firmware, which fwupd looks after.
const FIRMWARE: &str = "Firmware is not in Settings yet. fwupdmgr get-updates says what the makers \
                        of this machine have published, and sudo fwupdmgr update installs it.";

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(name: &str, version: &str, uki: &str) -> Slot {
        Slot {
            slot: name.to_string(),
            version: version.to_string(),
            uki: uki.to_string(),
        }
    }

    fn drive() -> Slots {
        Slots {
            running: "0.1.0".to_string(),
            slots: vec![
                slot("a", "0.1.0", "rift_0.1.0.efi"),
                slot("b", "0.2.0", "rift_0.2.0+3-0.efi"),
            ],
            source: "file:///var/lib/rift/updates/".to_string(),
            waiting: vec!["0.2.0".to_string()],
        }
    }

    fn settings(slots: Option<Result<Slots, String>>) -> Settings {
        let mut state = Settings::bare();
        state.slots = slots;
        state
    }

    #[test]
    fn a_row_says_what_its_slot_holds() {
        let held = drive();
        assert_eq!(holds(&held.slots[0], &held.running), "0.1.0, running now");
        assert_eq!(holds(&held.slots[1], &held.running), "0.2.0, 3 tries left");
        assert_eq!(
            holds(&slot("b", "0.2.0", "rift_0.2.0+1-2.efi"), "0.1.0"),
            "0.2.0, 1 try left"
        );
        assert_eq!(holds(&slot("b", "", ""), "0.1.0"), "Empty");
        assert_eq!(
            holds(&slot("b", "0.2.0", ""), "0.1.0"),
            "0.2.0, with nothing on the boot partition to start it"
        );
        assert_eq!((label("a"), label("b")), ("Slot A", "Slot B"));
    }

    #[test]
    fn the_state_says_what_is_where_and_what_is_waiting() {
        assert_eq!(
            state(&settings(Some(Ok(drive())))),
            [
                "version 0.1.0",
                "slot a",
                "slot-a 0.1.0",
                "tries-a none",
                "slot-b 0.2.0",
                "tries-b 3",
                "updates /var/lib/rift/updates",
                // what is waiting is the version already in slot b
                "waiting none",
            ]
        );
    }

    #[test]
    fn an_empty_slot_and_a_new_version_are_in_the_state() {
        let mut held = drive();
        held.slots[1] = slot("b", "", "");
        held.waiting = vec!["0.2.0".to_string(), "0.3.0".to_string()];
        assert_eq!(
            state(&settings(Some(Ok(held))))[4..],
            [
                "slot-b none",
                "tries-b none",
                "updates /var/lib/rift/updates",
                "waiting 0.3.0",
            ]
        );
    }

    #[test]
    fn nothing_is_said_until_vault_has_answered() {
        assert!(state(&settings(None)).is_empty());
        assert!(state(&settings(Some(Err("Vault is not running.".to_string())))).is_empty());
    }

    #[test]
    fn the_sentences_are_sentences() {
        for sentence in [TWO_SLOTS, TRIES, INSTALLING, FIRMWARE] {
            assert!(sentence.ends_with('.'), "{sentence}");
            assert!(sentence.is_ascii(), "{sentence}");
        }
    }
}
