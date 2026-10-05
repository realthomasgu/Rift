//! The search: the words in the field, looked for in the appstream data of every remote the
//! installations have. flatpak reads the data it already has and fetches what it has not, so the
//! first search of a remote waits for that download; it happens on a thread of its own either way.
//!
//! A search runs half a second after the last letter, or at once on Enter. The words are counted as
//! they are typed, and an answer to words that are no longer the ones in the field is thrown away.

use std::thread;
use std::time::Duration;

use iced::futures::channel::oneshot;
use iced::widget::{column, row, text};
use iced::{Center, Element, Fill, Task};
use librift::flatpak::{self, Found};

use crate::page::Page;
use crate::theme::Colors;
use crate::ui::{Message, Store, said};
use crate::widgets::{GAP, TEXT_SIZE, action, group, line, note, pressable};

/// How long after the last letter the search runs.
const WAIT: Duration = Duration::from_millis(500);

/// How few letters are not worth a search.
const SHORTEST: usize = 2;

/// What a search found, or why it could not be made.
pub type Answer = Result<Vec<Found>, String>;

/// Something was typed in the field. The page follows the field: words enough to search for show
/// what they find, and an empty field is the front page again.
pub fn typed(state: &mut Store, words: String) -> Task<Message> {
    state.words = words;
    state.typed = state.typed.wrapping_add(1);
    let when = state.typed;
    if state.words.trim().len() < SHORTEST {
        state.found = None;
        state.searching = false;
        if state.page == Page::Found {
            state.page = Page::Apps;
        }
        return Task::none();
    }
    state.page = Page::Found;
    // the letter after this one is usually a moment away, so the search waits for the words to
    // settle
    Task::perform(async move { thread::sleep(WAIT) }, move |()| {
        Message::Waited(when)
    })
}

/// Look for what is in the field now.
pub fn find(state: &mut Store) -> Task<Message> {
    let words = state.words.trim().to_string();
    if words.len() < SHORTEST {
        return Task::none();
    }
    state.page = Page::Found;
    #[cfg(debug_assertions)]
    if state.pretend {
        state.found = Some(Ok(crate::pretend::found(&words)));
        return Task::none();
    }
    state.searching = true;
    let when = state.typed;
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(flatpak::search(&words));
    });
    Task::perform(receiver, move |answered| {
        Message::Searched(
            when,
            Box::new(answered.unwrap_or_else(|_| {
                Err("The search stopped before flatpak answered it.".to_string())
            })),
        )
    })
}

/// The wait after a letter is over: search, unless another letter has been typed since.
pub fn waited(state: &mut Store, when: u64) -> Task<Message> {
    if when == state.typed {
        return find(state);
    }
    Task::none()
}

/// An answer from flatpak. One to words that are no longer in the field is thrown away.
pub fn searched(state: &mut Store, when: u64, answer: Answer) {
    if when != state.typed {
        return;
    }
    state.searching = false;
    state.found = Some(answer);
}

/// The lines `--state` prints: what is in the field, whether flatpak is being asked, how many rows
/// there are and the id and the name of each.
#[must_use]
pub fn lines(state: &Store) -> Vec<String> {
    let mut said = vec![
        format!(
            "words {}",
            if state.words.trim().is_empty() {
                "none"
            } else {
                state.words.trim()
            }
        ),
        format!("searching {}", if state.searching { "yes" } else { "no" }),
    ];
    match state.found.as_ref() {
        None => said.push("rows unknown".to_string()),
        Some(Err(why)) => said.push(format!("rows-problem {why}")),
        Some(Ok(found)) => {
            said.push(format!("rows {}", found.len()));
            for one in found {
                said.push(format!("row {} {}", one.id, one.name));
            }
        }
    }
    said
}

/// The page.
pub fn view(state: &Store, look: Colors) -> Element<'_, Message> {
    let mut page = column![].spacing(GAP).width(Fill);
    let words = state.words.trim();
    if state.searching {
        return page
            .push(note(
                look,
                "Looking for it. The first search of a remote takes as long as it takes to fetch \
                 what that remote knows about its apps.",
            ))
            .into();
    }
    match state.found.as_ref() {
        None => page = page.push(note(look, "Type what you are looking for.")),
        Some(Err(why)) => {
            page = page.push(
                row![
                    text(format!("The search could not be made. {why}"))
                        .size(TEXT_SIZE)
                        .color(look.error)
                        .width(Fill),
                    action(look, "Try again", Some(Message::Find)),
                ]
                .align_y(Center)
                .spacing(GAP),
            );
        }
        Some(Ok(found)) if found.is_empty() => {
            page = page.push(said(
                look,
                format!("Nothing any remote offers is called {words}."),
            ));
        }
        Some(Ok(found)) => {
            page = page.push(said(
                look,
                format!(
                    "{} for {words}.",
                    if found.len() == 1 {
                        "One app".to_string()
                    } else {
                        format!("{} apps", found.len())
                    }
                ),
            ));
            let rows = found
                .iter()
                .map(|one| found_row(state, look, one))
                .collect();
            page = page.push(group(look, rows));
        }
    }
    page.into()
}

/// One row: the name with the line about it under it, and what the Store knows about it at the
/// right. The whole row opens the app's own page.
fn found_row<'a>(state: &'a Store, look: Colors, one: &'a Found) -> Element<'a, Message> {
    let mut left = column![line(look, &one.name)].spacing(2);
    if !one.summary.is_empty() {
        left = left.push(note(look, &one.summary));
    }
    let beside = if crate::catalog::is_installed(state, &one.id) {
        Some(note(look, "Installed"))
    } else if one.remote == flatpak::FLATHUB || one.remote.is_empty() {
        None
    } else {
        Some(said(look, format!("From {}", one.remote)))
    };
    pressable(
        look,
        left.width(Fill).into(),
        beside,
        false,
        Message::Open(one.id.clone(), one.remote.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::tests::{answered, listed};
    use librift::flatpak::FLATHUB;

    fn one(id: &str, name: &str) -> Found {
        Found {
            id: id.to_string(),
            name: name.to_string(),
            summary: "A plain text editor for the boot test".to_string(),
            version: String::new(),
            remote: "rift-test".to_string(),
        }
    }

    #[test]
    fn the_page_follows_the_field() {
        let mut state = Store::bare();
        state.apps = Ok(listed());
        // one letter is not worth a search, and the front page stays
        let _ = typed(&mut state, "e".to_string());
        assert_eq!(state.page, Page::Apps);
        let _ = typed(&mut state, "editor".to_string());
        assert_eq!(state.page, Page::Found);
        // and clearing the field goes back to the front page
        let _ = typed(&mut state, String::new());
        assert_eq!(state.page, Page::Apps);
        assert!(state.found.is_none());
    }

    #[test]
    fn an_answer_to_words_that_have_changed_is_thrown_away() {
        let mut state = Store::bare();
        let _ = typed(&mut state, "edit".to_string());
        let before = state.typed;
        let _ = typed(&mut state, "editor".to_string());
        state.searching = true;
        searched(
            &mut state,
            before,
            Ok(vec![one("dev.rift.TestEditor", "Rift test editor")]),
        );
        assert!(state.found.is_none() && state.searching);
        let now = state.typed;
        searched(
            &mut state,
            now,
            Ok(vec![one("dev.rift.TestEditor", "Rift test editor")]),
        );
        assert!(!state.searching);
        assert_eq!(
            lines(&state),
            [
                "words editor",
                "searching no",
                "rows 1",
                "row dev.rift.TestEditor Rift test editor",
            ]
        );
    }

    #[test]
    fn a_search_that_could_not_be_made_says_so() {
        let mut state = Store::bare();
        state.apps = Ok(listed());
        state.catalog = Some(Box::new(answered(&[FLATHUB], &[])));
        let _ = typed(&mut state, "editor".to_string());
        let now = state.typed;
        searched(&mut state, now, Err("No network.".to_string()));
        assert_eq!(
            lines(&state),
            ["words editor", "searching no", "rows-problem No network.",]
        );
        // and a field with nothing in it has nothing to say
        let mut empty = Store::bare();
        assert_eq!(
            lines(&empty),
            ["words none", "searching no", "rows unknown"]
        );
        let _ = typed(&mut empty, "  ".to_string());
        assert_eq!(empty.words.trim(), "");
    }
}
