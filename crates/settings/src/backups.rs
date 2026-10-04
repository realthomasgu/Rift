//! The Backups page: the snapshots Vault takes of home every hour, and the backups it makes in a
//! folder on another disk.
//!
//! Both halves ask Vault on the system bus, and both are slow enough to ask on a thread of their
//! own: listing the backups mounts the disk and reads rustic's index, and making one reads home.
//! The page asks when it comes up rather than when the window opens, the way the Search page reads
//! the index, and each half answers on its own so a disk that takes a moment does not hold up the
//! snapshots.

use std::thread;

use iced::futures::channel::oneshot;
use iced::widget::{column, text};
use iced::{Element, Fill, Task};
use librift::time;
use librift::vault::{self, Target};

use crate::ai::said;
use crate::ghost;
use crate::theme::Colors;
use crate::ui::{Message, Settings};
use crate::widgets::{GAP, TEXT_SIZE, action, fact, group, heading, note, setting};

/// How many copies there are and when the newest was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Kept {
    /// How many Vault lists.
    pub count: usize,
    /// When the newest was made, in seconds since 1970. None when there are none.
    pub newest: Option<i64>,
}

impl Kept {
    /// What Vault's names come to: they are the times the copies were made.
    fn of(names: &[&str]) -> Self {
        Self {
            count: names.len(),
            newest: names
                .iter()
                .filter_map(|name| vault::snapshot_time(name))
                .max(),
        }
    }

    /// How long ago the newest was made, or nothing when there are none.
    fn ago(self) -> Option<String> {
        self.newest.map(|made| time::ago(time::now() - made))
    }
}

/// What Vault is making for the page right now. Both take a while, and each dims its own button
/// while it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Making {
    /// A snapshot of home.
    pub snapshot: bool,
    /// A backup onto the disk.
    pub backup: bool,
}

/// What the page knows about the backups on the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    /// Where backups go, or why there is nowhere yet.
    pub target: Result<Target, String>,
    /// What is in that folder, or why it could not be read: the disk is not plugged in, or nothing
    /// has been backed up to it yet.
    pub made: Result<Kept, String>,
}

/// Ask Vault for the snapshots of home on a thread of its own.
pub fn read_snapshots() -> Task<Message> {
    on_a_thread(snapshots, |answered| {
        Message::Snapshots(answered.unwrap_or_else(|_| Err(quiet())))
    })
}

/// Ask Vault where backups go and what is there, on a thread of its own.
pub fn read_disk() -> Task<Message> {
    on_a_thread(disk, |answered| {
        Message::Backups(Box::new(answered.unwrap_or_else(|_| Disk {
            target: Err(quiet()),
            made: Err(quiet()),
        })))
    })
}

/// Both halves, which is what the page asks for when it comes up.
pub fn read() -> Task<Message> {
    Task::batch([read_snapshots(), read_disk()])
}

/// Take a snapshot of home now, and read the snapshots again afterwards. The reading is asked for
/// inside the closure, so its thread starts once the snapshot has been taken rather than beside it.
pub fn take() -> Task<Message> {
    on_a_thread(
        || vault::take().map(|_| ()),
        |said| said.unwrap_or_else(|_| Err(quiet())),
    )
    .then(|said| Task::done(Message::Took(said)).chain(read_snapshots()))
}

/// Back up home onto the backup disk now, and read the disk again afterwards.
pub fn back_up() -> Task<Message> {
    on_a_thread(
        || vault::backup().map(|_| ()),
        |said| said.unwrap_or_else(|_| Err(quiet())),
    )
    .then(|said| Task::done(Message::BackedUp(said)).chain(read_disk()))
}

/// Run `ask` on a thread of its own and turn what it answers into a message, or into what the half
/// that follows it takes.
fn on_a_thread<T, O>(
    ask: impl FnOnce() -> T + Send + 'static,
    into: impl Fn(Result<T, oneshot::Canceled>) -> O + Send + 'static,
) -> Task<O>
where
    T: Send + 'static,
    O: Send + 'static,
{
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(ask());
    });
    Task::perform(receiver, into)
}

/// What the page says when the thread went away before it answered.
fn quiet() -> String {
    "Vault did not answer.".to_string()
}

/// The snapshots of home, as the page counts them.
fn snapshots() -> Result<Kept, String> {
    vault::list().map(|names| {
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        Kept::of(&names)
    })
}

/// Where backups go and what is there. The folder is asked for first, because a machine with no
/// backup disk chosen has nothing to list and says so in one sentence rather than two.
fn disk() -> Disk {
    let target = vault::target();
    let made = match &target {
        Ok(_) => vault::backups().map(|backups| {
            let times: Vec<&str> = backups.iter().map(|(_, time)| time.as_str()).collect();
            Kept::of(&times)
        }),
        Err(why) => Err(why.clone()),
    };
    Disk { target, made }
}

/// The lines `rift-settings --state` prints about backups: how many snapshots there are and how
/// long ago the newest was taken, the same for the backups on the disk, where they go, and whether
/// one of the two is being made now.
#[must_use]
pub fn state(state: &Settings) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(Ok(kept)) = state.snapshots.as_ref() {
        lines.push(format!("snapshots {}", kept.count));
        lines.push(format!(
            "snapshot {}",
            kept.ago().unwrap_or_else(|| "none".to_string())
        ));
    }
    if let Some(disk) = state.disk.as_deref() {
        lines.push(format!(
            "backups {}",
            match &disk.made {
                Ok(kept) => kept.count.to_string(),
                Err(_) => "none".to_string(),
            }
        ));
        if let Some(ago) = disk.made.as_ref().ok().and_then(|kept| kept.ago()) {
            lines.push(format!("backup {ago}"));
        }
        lines.push(format!(
            "backup-folder {}",
            match &disk.target {
                Ok(target) => target.folder.clone(),
                Err(_) => "none".to_string(),
            }
        ));
    }
    lines.push(format!("taking {}", on_or_off(state.making.snapshot)));
    lines.push(format!("backing {}", on_or_off(state.making.backup)));
    lines
}

fn on_or_off(doing: bool) -> &'static str {
    if doing { "on" } else { "off" }
}

/// The page.
pub fn view(state: &Settings, look: Colors) -> Element<'_, Message> {
    let mut page = column![].spacing(GAP).width(Fill);
    page = page.push(timeline(state, look));
    page = page.push(on_a_disk(state, look));
    if let Some(why) = &state.problem {
        page = page.push(text(why).size(TEXT_SIZE).color(look.error));
    }
    page.push(note(look, RESTORING)).into()
}

/// The snapshots of home: how many there are, how long ago the newest was taken, and the button
/// that takes one now.
fn timeline(state: &Settings, look: Colors) -> Element<'_, Message> {
    if ghost::on() {
        // the snapshots are subvolumes of persist, which stays locked, so there is nothing to
        // count and nothing to take one of: home here is memory (ADR-0084)
        return column![
            heading(look, "Snapshots of your home"),
            group(look, vec![ghost::row(look, "Snapshots", NO_SNAPSHOTS)]),
            note(look, ON_THIS_DRIVE),
        ]
        .spacing(8)
        .into();
    }
    let mut rows = Vec::new();
    match state.snapshots.as_ref() {
        None => rows.push(fact(look, "Snapshots", "Asking Vault.".to_string())),
        Some(Err(why)) => rows.push(fact(look, "Snapshots", why.clone())),
        Some(Ok(kept)) => {
            rows.push(setting(
                look,
                "Snapshots",
                None,
                said(look, &kept.count.to_string()),
            ));
            if let Some(ago) = kept.ago() {
                rows.push(setting(look, "Newest", None, said(look, &ago)));
            }
        }
    }
    rows.push(setting(
        look,
        "Take a snapshot now",
        Some(if state.making.snapshot {
            "Taking a snapshot of your home folder."
        } else {
            "Vault takes one every hour and keeps the first of each hour, each day and each week."
        }),
        action(
            look,
            "Take",
            (!state.making.snapshot).then_some(Message::Snapshot),
        ),
    ));
    column![
        heading(look, "Snapshots of your home"),
        group(look, rows),
        note(look, ON_THIS_DRIVE)
    ]
    .spacing(8)
    .into()
}

/// The backups on another disk: where they go, what is there, and the button that makes one.
fn on_a_disk(state: &Settings, look: Colors) -> Element<'_, Message> {
    if ghost::on() {
        // the folder backups go to and the password that encrypts them are both on persist, so
        // there is nothing to list and nothing to back up to
        return column![
            heading(look, "Backups on a disk"),
            group(look, vec![ghost::row(look, "Folder", NO_BACKUPS)]),
        ]
        .spacing(8)
        .into();
    }
    let mut rows = Vec::new();
    match state.disk.as_deref() {
        None => rows.push(fact(look, "Folder", "Asking Vault.".to_string())),
        Some(disk) => rows = disk_rows(state, look, disk),
    }
    column![
        heading(look, "Backups on a disk"),
        group(look, rows),
        note(look, CHOOSING),
    ]
    .spacing(8)
    .into()
}

/// The rows of the disk once Vault has answered: the folder and the disk it is on, what is in it,
/// and the button. A machine with no folder chosen has nothing to back up to, so it says that and
/// no more.
fn disk_rows<'a>(state: &'a Settings, look: Colors, disk: &'a Disk) -> Vec<Element<'a, Message>> {
    let target = match &disk.target {
        Err(why) => return vec![fact(look, "Folder", why.clone())],
        Ok(target) => target,
    };
    let mut rows = vec![
        setting(look, "Folder", None, said(look, &target.folder)),
        setting(look, "Disk", None, said(look, &target.disk)),
    ];
    match &disk.made {
        Err(why) => rows.push(fact(look, "Backups", why.clone())),
        Ok(kept) => {
            rows.push(setting(
                look,
                "Backups",
                None,
                said(look, &kept.count.to_string()),
            ));
            if let Some(ago) = kept.ago() {
                rows.push(setting(look, "Newest", None, said(look, &ago)));
            }
        }
    }
    rows.push(setting(
        look,
        "Back up home now",
        Some(if state.making.backup {
            "Copying what is new or changed onto the disk."
        } else {
            "Everything in your home folder is copied onto the disk, encrypted."
        }),
        action(
            look,
            "Back up",
            (!state.making.backup).then_some(Message::BackUp),
        ),
    ));
    rows
}

/// What a Ghost boot cannot do with the snapshots and the backups.
const NO_SNAPSHOTS: &str = "Timeline snapshots cannot be reached";
const NO_BACKUPS: &str = "Backups cannot be reached";

/// What a snapshot is and is not.
const ON_THIS_DRIVE: &str = "Snapshots sit beside your home folder on this drive, so they bring \
                             back a file you changed or deleted. A drive that is lost or broken \
                             takes them with it, which is what the backups below are for.";
/// How the disk backups go to is chosen today.
const CHOOSING: &str = "Choosing the disk is sudo vault target <folder> from a terminal, which \
                        prints the password of the backups once. Vault mounts that disk itself \
                        whenever it is plugged in.";
/// Where a file comes back from a snapshot, and how one comes back from a backup.
const RESTORING: &str = "Timeline in Files shows a folder as it was at any of these snapshots and \
                         puts a file back. A file from a backup is rift backup restore, from a \
                         terminal.";

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Target {
        Target {
            folder: "/Rift".to_string(),
            disk: "5f0e1c7a-8d2b-4c1e-9a3f-6b7d8e9f0a1b".to_string(),
        }
    }

    fn settings(snapshots: Option<Result<Kept, String>>, disk: Option<Disk>) -> Settings {
        let mut state = Settings::bare();
        state.snapshots = snapshots;
        state.disk = disk.map(Box::new);
        state
    }

    #[test]
    fn the_names_vault_lists_come_to_a_count_and_a_time() {
        let kept = Kept::of(&[
            "2026-09-15T09:00:00Z",
            "2026-09-15T11:48:00Z",
            "2026-09-15T10:00:00Z",
        ]);
        assert_eq!(kept.count, 3);
        assert_eq!(kept.newest, vault::snapshot_time("2026-09-15T11:48:00Z"));
        // a name that is not a time is still one of them, and none of them is a time here
        let odd = Kept::of(&["keep-this", "2026-09-15T09:00:00Z"]);
        assert_eq!(odd.count, 2);
        assert_eq!(odd.newest, vault::snapshot_time("2026-09-15T09:00:00Z"));
        assert_eq!(Kept::of(&["keep-this"]).newest, None);
        assert_eq!(Kept::of(&[]), Kept::default());
        assert_eq!(Kept::of(&[]).ago(), None);
    }

    #[test]
    fn the_state_says_how_many_there_are_and_how_long_ago() {
        let kept = settings(
            Some(Ok(Kept {
                count: 7,
                newest: Some(time::now() - 12 * 60 - 5),
            })),
            Some(Disk {
                target: Ok(target()),
                made: Ok(Kept {
                    count: 2,
                    newest: Some(time::now() - 3 * 3600),
                }),
            }),
        );
        assert_eq!(
            state(&kept),
            [
                "snapshots 7",
                "snapshot 12 minutes ago",
                "backups 2",
                "backup 3 hours ago",
                "backup-folder /Rift",
                "taking off",
                "backing off",
            ]
        );
    }

    #[test]
    fn a_drive_with_no_backup_disk_says_none() {
        let kept = settings(
            Some(Ok(Kept::default())),
            Some(Disk {
                target: Err("There is no backup disk yet.".to_string()),
                made: Err("There is no backup disk yet.".to_string()),
            }),
        );
        assert_eq!(
            state(&kept),
            [
                "snapshots 0",
                "snapshot none",
                "backups none",
                "backup-folder none",
                "taking off",
                "backing off",
            ]
        );
    }

    #[test]
    fn nothing_is_said_about_a_half_that_has_not_answered() {
        let asking = settings(None, None);
        assert_eq!(state(&asking), ["taking off", "backing off"]);
        // and a failure is on the page, not in the state
        let broken = settings(Some(Err("Vault is not running.".to_string())), None);
        assert_eq!(state(&broken), ["taking off", "backing off"]);
    }

    #[test]
    fn a_disk_with_nothing_backed_up_to_it_yet_says_none_of_them() {
        let kept = settings(
            None,
            Some(Disk {
                target: Ok(target()),
                made: Ok(Kept::default()),
            }),
        );
        assert_eq!(state(&kept)[..2], ["backups 0", "backup-folder /Rift"]);
    }

    #[test]
    fn a_disk_that_is_not_plugged_in_still_says_where_the_backups_go() {
        let kept = settings(
            None,
            Some(Disk {
                target: Ok(target()),
                made: Err("The backup disk is not plugged in.".to_string()),
            }),
        );
        assert_eq!(state(&kept)[..2], ["backups none", "backup-folder /Rift"]);
    }

    #[test]
    fn what_is_being_made_now_is_in_the_state() {
        let mut kept = settings(Some(Ok(Kept::default())), None);
        kept.making.snapshot = true;
        assert_eq!(state(&kept)[2..], ["taking on", "backing off"]);
        kept.making.backup = true;
        assert_eq!(state(&kept)[2..], ["taking on", "backing on"]);
    }

    #[test]
    fn the_sentences_are_sentences() {
        for sentence in [ON_THIS_DRIVE, CHOOSING, RESTORING] {
            assert!(sentence.ends_with('.'), "{sentence}");
            assert!(sentence.is_ascii(), "{sentence}");
        }
    }
}
