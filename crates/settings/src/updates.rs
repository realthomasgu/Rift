//! The Updates page: which version each slot of the drive holds, what the next boot would start,
//! where updates come from, and the button that installs the one that is waiting.
//!
//! None of that can be read without root. The esp is mounted for root alone and the labels that
//! say which version is in which slot are on the drive itself, so Vault reads the lot and answers
//! it in one call. That call mounts the esp, so the page asks for it when it comes up rather than
//! when the window opens.
//!
//! The install is Vault's too: it writes the slot that is not running, which nothing but flashing
//! and cloning has done before (ADR-0089). Both calls take a while, so each runs on a thread of
//! its own, and they run one after another rather than beside each other, since both mount the
//! esp.

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
use crate::widgets::{GAP, action, fact, group, heading, note, setting};

/// Ask Vault what this drive holds and what an update would do, one after the other: both mount
/// the esp, and the window is not waiting for either.
pub fn read() -> Task<Message> {
    // the second task is built inside the closure, so its thread starts once the first has
    // answered rather than beside it
    held().then(|said| Task::done(said).chain(waiting_version()))
}

/// Ask Vault what this drive holds, on a thread of its own: it mounts the esp and reads the
/// drive's partition table.
fn held() -> Task<Message> {
    on_a_thread(vault::slots, Message::Slots)
}

/// Ask Vault what an update would do, on a thread of its own. On a drive that was flashed and
/// never updated this reads the whole running partition to work out what it already holds, so it
/// is the slower of the two.
fn waiting_version() -> Task<Message> {
    on_a_thread(vault::next_version, Message::Next)
}

/// Install the version that is waiting, then read both halves again: the slots have changed and so
/// has what is waiting.
pub fn install() -> Task<Message> {
    on_a_thread(vault::update, Message::Installed).then(|said| Task::done(said).chain(read()))
}

/// Run `ask` on a thread of its own and turn what it answers into a message.
fn on_a_thread<T: Send + 'static>(
    ask: impl FnOnce() -> Result<T, String> + Send + 'static,
    into: impl Fn(Result<T, String>) -> Message + Send + 'static,
) -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(ask());
    });
    Task::perform(receiver, move |answered| {
        into(answered.unwrap_or_else(|_| Err("Vault did not answer.".to_string())))
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
    if let Some(Ok(plan)) = state.next.as_ref() {
        lines.push(format!("next {}", or_none(&plan.version)));
        lines.push(format!("fetch {}", plan.fetch));
        lines.push(format!("total {}", plan.total));
    }
    lines.push(format!(
        "installing {}",
        if state.installing { "on" } else { "off" }
    ));
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
            page = page.push(waiting(look, state, slots));
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

/// Where updates come from, whether one is waiting there, what it would cost and the button that
/// installs it.
fn waiting<'a>(look: Colors, state: &Settings, slots: &Slots) -> Element<'a, Message> {
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
        rows.extend(costs(look, state));
        rows.push(installing(look, state, slots));
    }
    column![
        heading(look, "Updates"),
        group(look, rows),
        note(look, ANOTHER_SLOT)
    ]
    .spacing(8)
    .into()
}

/// What the update waiting costs, once Vault has worked it out. A sentence as a value, since it is
/// too long to sit at the right end of a row.
fn costs<'a>(look: Colors, state: &Settings) -> Option<Element<'a, Message>> {
    match state.next.as_ref()? {
        Ok(plan) if plan.waiting() => Some(fact(look, "Size", plan.line())),
        Ok(_) => None,
        Err(why) => Some(fact(look, "Size", why.clone())),
    }
}

/// The row that installs: the button, and what is happening under it.
fn installing<'a>(look: Colors, state: &Settings, slots: &Slots) -> Element<'a, Message> {
    let waiting = slots.newer().is_some();
    let under = match (state.installing, waiting, state.next.is_some()) {
        (true, _, _) => WRITING,
        (false, false, _) => NOTHING,
        (false, true, false) => WORKING,
        (false, true, true) => INSTALLED,
    };
    setting(
        look,
        "Install it",
        Some(under),
        action(
            look,
            "Install",
            (waiting && !state.installing).then_some(Message::Install),
        ),
    )
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
/// What installing one does, under the button.
const INSTALLED: &str = "Only the parts this drive does not have already are fetched.";
/// What the row says while Vault writes the slot.
const WRITING: &str = "Writing the other slot. This takes a while, and nothing of the version you \
                       are on now is touched.";
/// And when there is nothing to install, or nothing worked out yet.
const NOTHING: &str = "There is nothing newer in the folder updates come from.";
const WORKING: &str = "Working out how much of it this drive already has.";
/// What the group says under it, however it is drawn.
const ANOTHER_SLOT: &str = "An update is written into the slot that is not running, so the version \
                            you are on now is left alone, and the next boot starts the new one.";
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
                "installing off",
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
                "installing off",
            ]
        );
    }

    #[test]
    fn what_an_update_would_cost_is_in_the_state_once_vault_has_worked_it_out() {
        let mut held = settings(Some(Ok(drive())));
        held.next = Some(Ok(librift::update::Plan {
            version: "0.3.0".to_string(),
            running: "0.1.0".to_string(),
            running_slot: "a".to_string(),
            slot: "b".to_string(),
            from: "/var/lib/rift/updates".to_string(),
            total: 6_015_943_552,
            fetch: 521_248_768,
        }));
        held.installing = true;
        assert_eq!(
            state(&held)[8..],
            [
                "next 0.3.0",
                "fetch 521248768",
                "total 6015943552",
                "installing on",
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
        for sentence in [
            TWO_SLOTS,
            TRIES,
            ANOTHER_SLOT,
            INSTALLED,
            WRITING,
            WORKING,
            NOTHING,
            FIRMWARE,
        ] {
            assert!(sentence.ends_with('.'), "{sentence}");
            assert!(sentence.is_ascii(), "{sentence}");
        }
    }
}
