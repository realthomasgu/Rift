//! One app's own page: what it is called, the line about it, how big it is, what it runs on, and the
//! whole list of what it asks for, which is the only thing that says what installing it allows.
//! Install and Remove are here, with how far they have got.
//!
//! The remote is asked on a thread of its own as the page opens, twice over: once for what it says
//! about the app, and once for its metadata, where the permissions are written.

use std::thread;

use iced::futures::channel::oneshot;
use iced::widget::{column, row, space, text};
use iced::{Center, Element, Fill, Task};
use librift::flatpak::{self, About};
use librift::permissions::{self, Permission};

use crate::jobs::{self, Job};
use crate::page::Page;
use crate::theme::Colors;
use crate::ui::{Message, Store, said};
use crate::widgets::{GAP, TEXT_SIZE, action, fact, group, heading, note, primary, progress};

/// The app the page is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// Its id.
    pub id: String,
    /// The remote it comes from, which is empty for an app whose origin flatpak did not say.
    pub remote: String,
    /// Its name, as the page that opened it knows it.
    pub name: String,
    /// The line about it, as the page that opened it knows it. The remote's own line replaces it
    /// once the remote has answered.
    pub summary: String,
    /// The page to go back to.
    pub from: Page,
    /// What the remote says about it, once it has said.
    pub about: Option<Result<About, String>>,
    /// What it asks for, once the remote has said.
    pub asks: Option<Result<Vec<Permission>, String>>,
}

/// What the remote answered about one app.
pub type Answer = (
    String,
    Result<About, String>,
    Result<Vec<Permission>, String>,
);

/// Open an app's page and ask its remote about it.
pub fn open(state: &mut Store, id: &str, remote: &str) -> Task<Message> {
    let from = if state.page == Page::App {
        state.shown.as_ref().map_or(Page::Apps, |shown| shown.from)
    } else {
        state.page
    };
    let (name, summary) = known(state, id);
    state.shown = Some(Shown {
        id: id.to_string(),
        remote: remote.to_string(),
        name,
        summary,
        from,
        about: None,
        asks: None,
    });
    state.page = Page::App;
    ask(state)
}

/// What the Store already knows an app by, before its remote has said anything: the suggested list
/// first, then a row of a search, then the name it is installed under, and the last part of its id
/// when nothing else says.
fn known(state: &Store, id: &str) -> (String, String) {
    if let Some(app) = crate::catalog::offered(state)
        .into_iter()
        .find(|app| app.id == id)
    {
        return (app.name.clone(), app.about.clone());
    }
    if let Some(found) = state
        .found
        .as_ref()
        .and_then(|answer| answer.as_ref().ok())
        .and_then(|found| found.iter().find(|one| one.id == id))
    {
        return (found.name.clone(), found.summary.clone());
    }
    let listed = state
        .catalog
        .as_ref()
        .and_then(|catalog| catalog.installed.iter().find(|one| one.id == id));
    (
        listed.map_or_else(|| flatpak::tail(id), |one| one.name.clone()),
        String::new(),
    )
}

/// Ask the remote what it says about the app and what the app asks for.
pub fn ask(state: &mut Store) -> Task<Message> {
    let Some(shown) = &state.shown else {
        return Task::none();
    };
    let (id, remote) = (shown.id.clone(), shown.remote.clone());
    #[cfg(debug_assertions)]
    if state.pretend {
        let name = shown.name.clone();
        let (about, asks) = crate::pretend::about(&id, &name);
        answered(state, (id, Ok(about), Ok(asks)));
        return Task::none();
    }
    if remote.is_empty() {
        return Task::none();
    }
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let about = flatpak::about(&remote, &id);
        let asks = flatpak::metadata(&remote, &id).map(|metadata| permissions::of(&metadata));
        let _ = sender.send((id, about, asks));
    });
    Task::perform(receiver, |answered| {
        Message::About(Box::new(answered.unwrap_or_else(|_| {
            (
                String::new(),
                Err("The question to the remote stopped before it was answered.".to_string()),
                Err("The question to the remote stopped before it was answered.".to_string()),
            )
        })))
    })
}

/// The remote has answered. An answer about an app that is no longer the one shown is dropped.
pub fn answered(state: &mut Store, answer: Answer) {
    let (id, about, asks) = answer;
    let Some(shown) = state.shown.as_mut() else {
        return;
    };
    if shown.id != id {
        return;
    }
    if let Ok(about) = &about {
        if !about.name.is_empty() {
            shown.name.clone_from(&about.name);
        }
        if !about.summary.is_empty() {
            shown.summary.clone_from(&about.summary);
        }
    }
    shown.about = Some(about);
    shown.asks = Some(asks);
}

/// Install the app the page is showing, or take it off.
pub fn press(state: &mut Store, take: bool) -> Task<Message> {
    let Some(shown) = &state.shown else {
        return Task::none();
    };
    let (id, called, remote) = (shown.id.clone(), shown.name.clone(), shown.remote.clone());
    if crate::catalog::is_busy(state, &id) || (!take && remote.is_empty()) {
        return Task::none();
    }
    if take != crate::catalog::is_installed(state, &id) {
        return Task::none();
    }
    state.work.push(jobs::Work {
        id: id.clone(),
        name: called,
        remove: take,
        doing: jobs::Doing::Waiting,
    });
    jobs::start(
        state.queue(),
        Job {
            id,
            remote,
            remove: take,
        },
    )
}

/// The lines `--state` prints about the app that is shown.
#[must_use]
pub fn lines(state: &Store) -> Vec<String> {
    let Some(shown) = &state.shown else {
        return vec!["shown none".to_string()];
    };
    let mut said = vec![
        format!("shown {}", shown.id),
        format!("name {}", shown.name),
        format!("remote {}", words(&shown.remote)),
        format!("summary {}", words(&shown.summary)),
        format!(
            "installed {}",
            if crate::catalog::is_installed(state, &shown.id) {
                "yes"
            } else {
                "no"
            }
        ),
    ];
    match shown.about.as_ref() {
        None => said.push("size unknown".to_string()),
        Some(Err(why)) => said.push(format!("size-problem {why}")),
        Some(Ok(about)) => {
            said.push(format!("size {}", words(&about.installed)));
            said.push(format!("download {}", words(&about.download)));
            said.push(format!("runtime {}", words(&about.runtime)));
            if !about.version.is_empty() {
                said.push(format!("version {}", about.version));
            }
            if !about.licence.is_empty() {
                said.push(format!("licence {}", about.licence));
            }
        }
    }
    match shown.asks.as_ref() {
        None => said.push("permissions unknown".to_string()),
        Some(Err(why)) => said.push(format!("permissions-problem {why}")),
        Some(Ok(asked)) => {
            said.push(format!("permissions {}", asked.len()));
            for one in asked {
                said.push(format!("permission {}", one.said));
            }
        }
    }
    said
}

/// A value for a state line, which says nothing in one word when there is nothing.
fn words(value: &str) -> &str {
    if value.trim().is_empty() {
        "none"
    } else {
        value.trim()
    }
}

/// The page.
pub fn view(state: &Store, look: Colors) -> Element<'_, Message> {
    let Some(shown) = &state.shown else {
        return note(look, "No app is open.");
    };
    let installed = crate::catalog::is_installed(state, &shown.id);
    let busy = state
        .work
        .iter()
        .rev()
        .find(|work| work.id == shown.id && work.doing.pending());
    let button: Element<'_, Message> = if let Some(work) = busy {
        row![
            said(look, format!("{} it.", work.verb())),
            space().width(GAP),
            progress(
                look,
                match work.doing {
                    jobs::Doing::Running(percent) => percent,
                    _ => 0,
                }
            ),
        ]
        .align_y(Center)
        .into()
    } else if installed {
        action(look, "Remove", Some(Message::Remove))
    } else if shown.remote.is_empty() {
        note(look, "There is no remote to install it from.")
    } else {
        primary(look, "Install", Some(Message::Install))
    };
    // the name is in the header bar over the page, which is where every other window of the
    // session puts the name of what is open
    let mut page = column![].spacing(GAP).width(Fill);
    if shown.summary.is_empty() {
        page = page.push(note(look, "The remote says nothing about what it is."));
    } else {
        page = page.push(note(look, &shown.summary));
    }
    page = page.push(row![button, space().width(Fill)].align_y(Center));
    if let Some(work) = state
        .work
        .iter()
        .rev()
        .find(|work| work.id == shown.id && !work.doing.pending())
        && let jobs::Doing::Failed(why) = &work.doing
    {
        page = page.push(text(why.as_str()).size(TEXT_SIZE).color(look.error));
    }
    page = page.push(facts(state, look, shown));
    page = page.push(asks(look, shown));
    page.into()
}

/// What the remote says about the app, a row each.
fn facts<'a>(state: &'a Store, look: Colors, shown: &'a Shown) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    match shown.about.as_ref() {
        None if shown.remote.is_empty() => {}
        None => rows.push(fact(look, "Size", "Asking the remote".to_string())),
        Some(Err(why)) => rows.push(fact(look, "Size", why.clone())),
        Some(Ok(about)) => {
            let size = state
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.on(&shown.remote, &shown.id))
                .unwrap_or(about.installed.as_str());
            if !size.is_empty() {
                rows.push(fact(look, "Size", size.to_string()));
            }
            if !about.download.is_empty() {
                rows.push(fact(look, "To download", about.download.clone()));
            }
            if !about.version.is_empty() {
                rows.push(fact(look, "Version", about.version.clone()));
            }
            if !about.licence.is_empty() {
                rows.push(fact(look, "Licence", about.licence.clone()));
            }
            if !about.runtime.is_empty() {
                rows.push(fact(look, "Runs on", about.runtime.clone()));
            }
        }
    }
    rows.push(fact(look, "Id", shown.id.clone()));
    if !shown.remote.is_empty() {
        rows.push(fact(look, "From", shown.remote.clone()));
    }
    column![heading(look, "What it is"), group(look, rows)]
        .spacing(8)
        .into()
}

/// What the app asks for, a line each, the wide ones first.
fn asks<'a>(look: Colors, shown: &'a Shown) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    match shown.asks.as_ref() {
        None if shown.remote.is_empty() => {
            rows.push(sentence(look, "There is no remote to ask.".to_string()));
        }
        None => rows.push(sentence(look, "Asking the remote.".to_string())),
        Some(Err(why)) => rows.push(sentence(look, why.clone())),
        Some(Ok(asked)) if asked.is_empty() => rows.push(sentence(
            look,
            "Nothing. It cannot reach the network, your files, or anything else of yours."
                .to_string(),
        )),
        Some(Ok(asked)) => {
            for one in asked {
                rows.push(sentence(look, one.said.clone()));
            }
        }
    }
    let wide = shown
        .asks
        .as_ref()
        .and_then(|asked| asked.as_ref().ok())
        .is_some_and(|asked| asked.iter().any(|one| one.wide));
    let under = if wide {
        "The ones that reach your own files or the machine itself are at the top. An app in a \
         sandbox has what it asks for and nothing else: a file you hand it, a printer and the \
         camera are asked for as it runs and are not on this list."
    } else {
        "An app in a sandbox has what it asks for and nothing else: a file you hand it, a printer \
         and the camera are asked for as it runs and are not on this list."
    };
    column![
        heading(look, "What it asks for"),
        group(look, rows),
        note(look, under),
    ]
    .spacing(8)
    .into()
}

/// One line inside a group.
fn sentence<'a>(look: Colors, words: String) -> Element<'a, Message> {
    iced::widget::container(text(words).size(TEXT_SIZE).color(look.text))
        .width(Fill)
        .padding([8, 12])
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::tests::{answered as catalog, listed};
    use librift::flatpak::FLATHUB;

    const METADATA: &str = "[Application]\nname=dev.rift.TestEditor\n\n[Context]\n\
         shared=network;\nsockets=wayland;\nfilesystems=home;\n";

    fn remote_said(installed: &str) -> About {
        About {
            id: "dev.rift.TestEditor".to_string(),
            name: "Rift test editor".to_string(),
            summary: "A plain text editor for the boot test".to_string(),
            installed: installed.to_string(),
            download: "566 bytes".to_string(),
            runtime: "dev.rift.TestPlatform/x86_64/test".to_string(),
            ..About::default()
        }
    }

    fn opened() -> Store {
        let mut state = Store::bare();
        state.apps = Ok(listed());
        state.catalog = Some(Box::new(catalog(&[FLATHUB, "rift-test"], &[])));
        let _ = open(&mut state, "dev.rift.TestEditor", "rift-test");
        state
    }

    #[test]
    fn the_page_knows_the_app_by_the_suggested_list_until_the_remote_answers() {
        let mut state = opened();
        assert_eq!(state.page, Page::App);
        let shown = state.shown.as_ref().unwrap();
        assert_eq!(shown.name, "Rift test editor");
        assert_eq!(shown.summary, "The test's");
        assert_eq!(shown.from, Page::Apps);
        assert!(lines(&state).contains(&"permissions unknown".to_string()));
        answered(
            &mut state,
            (
                "dev.rift.TestEditor".to_string(),
                Ok(remote_said("2.0 kB")),
                Ok(permissions::of(METADATA)),
            ),
        );
        let said = lines(&state);
        assert!(said.contains(&"shown dev.rift.TestEditor".to_string()));
        assert!(said.contains(&"name Rift test editor".to_string()));
        assert!(said.contains(&"summary A plain text editor for the boot test".to_string()));
        assert!(said.contains(&"size 2.0 kB".to_string()));
        assert!(said.contains(&"download 566 bytes".to_string()));
        assert!(said.contains(&"permissions 3".to_string()));
        assert!(said.contains(&"permission Reads and writes your home folder".to_string()));
        assert!(said.contains(&"installed no".to_string()));
    }

    #[test]
    fn an_answer_about_another_app_is_dropped() {
        let mut state = opened();
        answered(
            &mut state,
            (
                "org.videolan.VLC".to_string(),
                Ok(remote_said("139.4 MB")),
                Ok(Vec::new()),
            ),
        );
        assert!(state.shown.as_ref().unwrap().about.is_none());
        assert!(lines(&state).contains(&"size unknown".to_string()));
    }

    #[test]
    fn install_asks_once_and_remove_only_what_is_installed() {
        let mut state = opened();
        let _ = press(&mut state, false);
        assert_eq!(state.work.len(), 1);
        assert!(!state.work[0].remove);
        // twice over is still one job, and there is nothing to remove yet
        let _ = press(&mut state, false);
        let _ = press(&mut state, true);
        assert_eq!(state.work.len(), 1);
        // once it is installed, Remove is the one that does something
        state.work[0].doing = jobs::Doing::Done;
        crate::catalog::settled(&mut state, "dev.rift.TestEditor", false);
        let _ = press(&mut state, false);
        assert_eq!(state.work.len(), 1);
        let _ = press(&mut state, true);
        assert_eq!(state.work.len(), 2);
        assert!(state.work[1].remove);
        assert!(lines(&state).contains(&"installed yes".to_string()));
    }

    #[test]
    fn an_app_with_no_remote_is_shown_without_one() {
        let mut state = Store::bare();
        state.catalog = Some(Box::new(catalog(&[FLATHUB], &["org.example.Gone"])));
        let _ = open(&mut state, "org.example.Gone", "");
        let said = lines(&state);
        assert!(said.contains(&"name Gone".to_string()));
        assert!(said.contains(&"remote none".to_string()));
        assert!(said.contains(&"summary none".to_string()));
        assert!(said.contains(&"size unknown".to_string()));
        assert!(said.contains(&"installed yes".to_string()));
        // nothing can be installed from nowhere, and it can still be taken off
        let _ = press(&mut state, false);
        assert!(state.work.is_empty());
        let _ = press(&mut state, true);
        assert_eq!(state.work.len(), 1);
    }
}
