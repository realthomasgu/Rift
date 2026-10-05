//! The dialogs: a name for a new folder, a new name for a file, the question before a name is
//! replaced, the question before anything is deleted for good, the question before an application
//! in one file is run, what is known about what is selected, and the passphrase of a locked disk. Each stands in the middle of its window with the
//! rest of the window dimmed behind it, the way GNOME's dialogs do: a title, one sentence or a
//! field, and the answers along the bottom.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use iced::widget::{column, text};
use iced::{Element, Fill, window};
use librift::files::check_name;
use rift_ui::theme::Colors;
use rift_ui::widgets::{TEXT_SIZE, action, destructive, dialog, fact, primary, wide_field};

use crate::props::Facts;
use crate::ui::Message;

/// A dialog that is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    /// A name for a new folder.
    NewFolder {
        /// What is typed.
        name: String,
    },
    /// A new name for a file or a folder.
    Rename {
        /// Its name now.
        from: OsString,
        /// The folder it is in, which is the window's folder unless a search found it under one
        /// of its own.
        inside: PathBuf,
        /// What is typed.
        name: String,
        /// Whether it is a folder.
        folder: bool,
    },
    /// Delete these for good, asked with Shift and Delete, or because they are somewhere with no
    /// trash.
    Delete {
        /// What is deleted.
        paths: Vec<PathBuf>,
        /// Whether they are somewhere with no trash.
        trashless: bool,
    },
    /// Delete these in the trash for good.
    Forget {
        /// Where each of them lies in the trash it is in.
        files: Vec<PathBuf>,
        /// The names they had, for the question.
        labels: Vec<String>,
    },
    /// Empty the trash.
    Empty,
    /// What is known about what is selected.
    Properties(Box<Facts>),
    /// The passphrase of a locked disk, asked for before it is unlocked.
    Unlock {
        /// What names the disk on the bus.
        drive: String,
        /// What the sidebar calls it.
        name: String,
        /// What is typed, which is never printed anywhere.
        secret: String,
        /// Why the last try did not open it.
        problem: Option<String>,
        /// Whether udisks is being asked at the moment.
        working: bool,
    },
    /// An `AppImage`, which is an application in one file, before it is run. Nothing of the system
    /// has checked it and it runs with everything the owner can reach, so it is asked for once,
    /// the way a downloaded program is asked for on Windows and macOS.
    Run {
        /// The file itself.
        path: PathBuf,
    },
    /// A copy, a move or a file brought back from a moment whose name is already taken in the
    /// folder it is going to.
    Replace {
        /// What is being copied, moved or brought back.
        from: Vec<PathBuf>,
        /// Where to.
        into: PathBuf,
        /// Which of the three it is.
        putting: Putting,
        /// The names that are taken there.
        names: Vec<String>,
    },
}

/// What is going into the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Putting {
    /// A copy of what is on the clipboard.
    Copy,
    /// What is on the clipboard, moved.
    Move,
    /// A file out of a moment in the Timeline, which the question names.
    Bring(String),
}

impl Dialog {
    /// The word `--state` prints for it.
    #[must_use]
    pub const fn word(&self) -> &'static str {
        match self {
            Self::NewFolder { .. } => "new-folder",
            Self::Rename { .. } => "rename",
            Self::Delete {
                trashless: false, ..
            } => "delete",
            Self::Delete {
                trashless: true, ..
            } => "no-trash",
            Self::Forget { .. } => "forget",
            Self::Empty => "empty",
            Self::Properties(_) => "properties",
            Self::Run { .. } => "run",
            Self::Unlock { .. } => "unlock",
            Self::Replace { .. } => "replace",
        }
    }

    /// What is typed in its field, when it has one.
    #[must_use]
    pub fn typed(&self) -> Option<&str> {
        match self {
            Self::NewFolder { name } | Self::Rename { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Change what is typed in its field. A passphrase is kept apart from the names, since
    /// `--state` prints what is typed in a field and a passphrase belongs nowhere but the dialog.
    pub fn type_in(&mut self, typed: String) {
        match self {
            Self::NewFolder { name } | Self::Rename { name, .. } => *name = typed,
            Self::Unlock {
                secret, problem, ..
            } => {
                *secret = typed;
                *problem = None;
            }
            _ => {}
        }
    }

    /// What is wrong with what was typed, as the line under the field: `folder` is the window's
    /// folder, and a rename looks in the folder the file itself is in. For a passphrase it is what
    /// udisks said. Nothing for a dialog with no field, and nothing for an empty name, which only
    /// dims the button.
    #[must_use]
    pub fn problem(&self, folder: &Path) -> Option<String> {
        let (name, from, folder) = match self {
            Self::NewFolder { name } => (name, None, folder),
            Self::Rename {
                name, from, inside, ..
            } => (name, Some(from.as_os_str()), inside.as_path()),
            Self::Unlock { problem, .. } => return problem.clone(),
            _ => return None,
        };
        name_problem(folder, name, from)
    }

    /// Whether its default button can be pressed.
    #[must_use]
    pub fn ready(&self, folder: &Path) -> bool {
        if let Self::Unlock {
            secret, working, ..
        } = self
        {
            return !secret.is_empty() && !working;
        }
        match self.typed() {
            Some(name) => check_name(name).is_ok() && self.problem(folder).is_none(),
            None => true,
        }
    }
}

/// What is wrong with a name for something new in `folder`, or for `from` there under a new name.
/// Nothing when it can be used, and nothing for an empty one.
#[must_use]
pub fn name_problem(folder: &Path, name: &str, from: Option<&OsStr>) -> Option<String> {
    if name.trim().is_empty() {
        return None;
    }
    if let Err(why) = check_name(name) {
        return Some(why.to_string());
    }
    if from == Some(OsStr::new(name)) {
        return None;
    }
    fs::symlink_metadata(folder.join(name))
        .is_ok()
        .then(|| format!("Something called {name} is already here."))
}

/// What the dialog for a locked disk says over its field.
const WHY_LOCKED: &str = "This disk is encrypted. Rift can see it and cannot read it yet.";

/// The id of the field of the dialog in window `number`. Each window has its own, since an
/// operation on a field reaches every window.
#[must_use]
pub fn field_id(number: usize) -> String {
    format!("files-dialog-{number}")
}

/// The dialog as it is drawn.
#[must_use]
pub fn view<'a>(
    shown: &'a Dialog,
    look: Colors,
    id: window::Id,
    number: usize,
    folder: &Path,
) -> Element<'a, Message> {
    let cancel = action(look, "Cancel", Some(Message::Cancel(id)));
    let ready = shown.ready(folder).then_some(Message::Confirm(id));
    match shown {
        Dialog::NewFolder { name } | Dialog::Rename { name, .. } => {
            let (title, button) = match shown {
                Dialog::Rename { folder: true, .. } => ("Rename folder", "Rename"),
                Dialog::Rename { .. } => ("Rename file", "Rename"),
                _ => ("New folder", "Create"),
            };
            let field = wide_field(
                look,
                "Name",
                name,
                field_id(number),
                move |typed| Message::Typed(id, typed),
                Message::Confirm(id),
            );
            let under: Element<'a, Message> = match shown.problem(folder) {
                Some(why) => text(why).size(TEXT_SIZE).color(look.error).into(),
                None if name.starts_with('.') => text("A name that starts with a dot is hidden.")
                    .size(TEXT_SIZE)
                    .color(look.dim)
                    .into(),
                None => text(" ").size(TEXT_SIZE).into(),
            };
            dialog(
                look,
                title.to_string(),
                vec![field, under],
                vec![cancel, primary(look, button, ready)],
            )
        }
        Dialog::Delete { paths, trashless } => deleting(look, paths, *trashless, cancel, ready),
        Dialog::Run { path } => running(look, path, cancel, ready),
        Dialog::Properties(facts) => properties(look, id, facts),
        Dialog::Unlock {
            name,
            secret,
            problem,
            working,
            ..
        } => unlocking(
            look,
            id,
            number,
            (name, secret, problem.as_deref(), *working),
            cancel,
            ready,
        ),
        Dialog::Replace {
            into,
            putting,
            names,
            ..
        } => replacing(look, id, into, putting, names, cancel, ready),
        Dialog::Forget { labels, .. } => {
            let said = if labels.len() == 1 {
                "It cannot be brought back."
            } else {
                "They cannot be brought back."
            };
            question(
                look,
                labels,
                said,
                cancel,
                destructive(look, "Delete", ready),
            )
        }
        Dialog::Empty => dialog(
            look,
            "Empty the trash?".to_string(),
            vec![sentence(look, "Everything in it is deleted for good.")],
            vec![cancel, destructive(look, "Empty trash", ready)],
        ),
    }
}

/// What is known about what is selected, a row for each fact.
fn properties<'a>(look: Colors, id: window::Id, facts: &Facts) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    for (label, said) in facts.rows() {
        rows.push(fact(look, label, said));
    }
    dialog(
        look,
        "Properties".to_string(),
        vec![column(rows).width(Fill).into()],
        vec![primary(look, "Close", Some(Message::Cancel(id)))],
    )
}

/// The passphrase of a locked disk: one sentence about what the disk is, the field, and the line
/// under it when the last passphrase did not open it.
fn unlocking<'a>(
    look: Colors,
    id: window::Id,
    number: usize,
    disk: (&'a str, &'a str, Option<&'a str>, bool),
    cancel: Element<'a, Message>,
    ready: Option<Message>,
) -> Element<'a, Message> {
    let (name, secret, problem, working) = disk;
    let field = rift_ui::widgets::wide_secret(
        look,
        "Passphrase",
        secret,
        field_id(number),
        move |typed| Message::Typed(id, typed),
        Message::Confirm(id),
    );
    let under: Element<'a, Message> = match problem {
        Some(why) => text(why).size(TEXT_SIZE).color(look.error).into(),
        None => text(" ").size(TEXT_SIZE).into(),
    };
    dialog(
        look,
        format!("Unlock {name}"),
        vec![
            text(WHY_LOCKED).size(TEXT_SIZE).color(look.text).into(),
            field,
            under,
        ],
        vec![
            cancel,
            primary(look, if working { "Unlocking" } else { "Unlock" }, ready),
        ],
    )
}

/// The question before a name in the folder is replaced. Keeping both is the default answer, the
/// way nothing in Rift is ever written over without being asked; Replace puts what is there in
/// the trash first, so it can still be brought back. A file out of a moment is the same question,
/// with the moment it comes from in it.
fn replacing<'a>(
    look: Colors,
    id: window::Id,
    into: &Path,
    putting: &Putting,
    names: &[String],
    cancel: Element<'a, Message>,
    ready: Option<Message>,
) -> Element<'a, Message> {
    let title = match names {
        [one] => format!("Replace {one}?"),
        more => format!("Replace {} items?", more.len()),
    };
    let what = if names.len() == 1 {
        "It is"
    } else {
        "They are"
    };
    let taking = match putting {
        Putting::Copy => "what is copied".to_string(),
        Putting::Move => "what is moved".to_string(),
        Putting::Bring(moment) => format!("the copy from {moment}"),
    };
    let said = format!(
        "{what} in {} already. What is there goes to the trash, and {taking} takes the name.",
        librift::files::shown(into)
    );
    let body: Element<'a, Message> = text(said).size(TEXT_SIZE).color(look.text).into();
    dialog(
        look,
        title,
        vec![body],
        vec![
            cancel,
            action(look, "Replace", Some(Message::Replace(id))),
            primary(look, "Keep both", ready),
        ],
    )
}

/// The question before something is deleted for good: its name, or how many there are.
fn question<'a>(
    look: Colors,
    names: &[String],
    said: &'a str,
    cancel: Element<'a, Message>,
    delete: Element<'a, Message>,
) -> Element<'a, Message> {
    let title = match names {
        [one] => format!("Delete {one} for good?"),
        _ => format!("Delete {} items for good?", names.len()),
    };
    dialog(
        look,
        title,
        vec![sentence(look, said)],
        vec![cancel, delete],
    )
}

/// The question before files are deleted for good: their names, and that they do not come back.
fn deleting<'a>(
    look: Colors,
    paths: &[PathBuf],
    trashless: bool,
    cancel: Element<'a, Message>,
    ready: Option<Message>,
) -> Element<'a, Message> {
    let names: Vec<String> = paths
        .iter()
        .map(|path| {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        })
        .collect();
    let said = match (names.len() == 1, trashless) {
        (true, false) => "It is not moved to the trash and cannot be brought back.",
        (false, false) => "They are not moved to the trash and cannot be brought back.",
        (true, true) => "It is on a disk with no trash, so it cannot be brought back.",
        (false, true) => "They are on a disk with no trash, so they cannot be brought back.",
    };
    question(
        look,
        &names,
        said,
        cancel,
        destructive(look, "Delete", ready),
    )
}

/// The question before an application in one file is run: what it is, and what running it allows.
fn running<'a>(
    look: Colors,
    path: &Path,
    cancel: Element<'a, Message>,
    ready: Option<Message>,
) -> Element<'a, Message> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    dialog(
        look,
        format!("Run {name}?"),
        vec![sentence(
            look,
            "This is an application in one file. It did not come from the Store, nothing has \
             checked it, and it can read and change everything you can.",
        )],
        vec![cancel, primary(look, "Run", ready)],
    )
}

fn sentence(look: Colors, said: &str) -> Element<'_, Message> {
    text(said).size(TEXT_SIZE).color(look.text).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_checked_against_the_folder() {
        let folder = std::env::temp_dir().join(format!("files-dialogs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("Notes")).unwrap();
        assert_eq!(name_problem(&folder, "", None), None);
        assert_eq!(name_problem(&folder, "Plans", None), None);
        assert_eq!(
            name_problem(&folder, "Notes", None),
            Some("Something called Notes is already here.".to_string())
        );
        // a rename to the name it has is no problem, only nothing to do
        assert_eq!(
            name_problem(&folder, "Notes", Some(OsStr::new("Notes"))),
            None
        );
        assert_eq!(
            name_problem(&folder, "a/b", None),
            Some("A name cannot have a slash in it.".to_string())
        );
        let new = Dialog::NewFolder {
            name: String::new(),
        };
        assert!(!new.ready(&folder));
        let named = Dialog::NewFolder {
            name: "Plans".to_string(),
        };
        assert!(named.ready(&folder));
        assert!(Dialog::Empty.ready(&folder));
        let _ = fs::remove_dir_all(&folder);
    }
}
