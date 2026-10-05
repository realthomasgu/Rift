//! The front page: the apps Rift suggests, in their groups, each with how much room it takes on the
//! drive and whether it needs an account. It is the same list Welcome offers on the first login, out
//! of the one file the image keeps, so the two never say different things.

use iced::widget::{column, row, text};
use iced::{Center, Element, Fill};
use librift::suggested::{self, App};

use crate::catalog;
use crate::theme::Colors;
use crate::ui::{Message, Store, said};
use crate::widgets::{GAP, TEXT_SIZE, action, group, heading, line, note, pressable};

/// The lines `--state` prints: the remotes, what each remote said, and every app listed with its
/// size or that it is installed.
#[must_use]
pub fn lines(state: &Store) -> Vec<String> {
    let mut said = Vec::new();
    if let Err(why) = &state.apps {
        said.push(format!("apps-problem {why}"));
    }
    match state.catalog.as_ref() {
        None => said.push(format!(
            "remotes {}",
            if state.asking { "asking" } else { "unknown" }
        )),
        Some(catalog) => {
            match &catalog.remotes {
                Ok(remotes) if remotes.is_empty() => said.push("remotes none".to_string()),
                Ok(remotes) => said.push(format!("remotes {}", remotes.join(","))),
                Err(why) => said.push(format!("remotes-problem {why}")),
            }
            for (remote, answer) in &catalog.sizes {
                said.push(match answer {
                    Ok(sizes) => format!("sizes {remote} {}", sizes.len()),
                    Err(why) => format!("sizes {remote} problem {why}"),
                });
            }
        }
    }
    for app in catalog::offered(state) {
        let size = if catalog::is_installed(state, &app.id) {
            "installed".to_string()
        } else {
            state
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.size(app))
                .unwrap_or("unknown")
                .to_string()
        };
        said.push(format!("app {} {size}", app.id));
    }
    said
}

/// The page.
pub fn view(state: &Store, look: Colors) -> Element<'_, Message> {
    let mut page = column![note(
        look,
        "These are the apps Rift suggests. They come from Flathub, each runs in a sandbox of its \
         own, and each one's page says what it asks for. Search for anything else.",
    )]
    .spacing(GAP)
    .width(Fill);
    let problem = state.catalog.as_ref().and_then(|catalog| catalog.problem());
    if state.asking {
        page = page.push(note(look, "Asking the remotes how big each app is."));
    } else if let Some(why) = problem {
        page = page.push(
            row![
                text(format!("The remote did not answer. {why}"))
                    .size(TEXT_SIZE)
                    .color(look.error)
                    .width(Fill),
                action(look, "Try again", Some(Message::Ask)),
            ]
            .align_y(Center)
            .spacing(GAP),
        );
    }
    if let Err(why) = &state.apps {
        page = page.push(text(why.as_str()).size(TEXT_SIZE).color(look.error));
    }
    let listed = catalog::offered(state);
    for name in suggested::groups(&listed) {
        let rows = listed
            .iter()
            .filter(|app| app.group == name)
            .map(|app| app_row(state, look, app))
            .collect();
        page = page.push(column![heading(look, name), group(look, rows)].spacing(8));
    }
    page.push(note(
        look,
        "A size is what the app itself takes on the drive. Most apps also need a runtime, which \
         apps share and which comes with the first app that needs it.",
    ))
    .into()
}

/// One app: its name with what it is for under it, and how big it is at the right. The whole row
/// opens the app's own page.
fn app_row<'a>(state: &'a Store, look: Colors, app: &'a App) -> Element<'a, Message> {
    let mut left = column![line(look, &app.name), note(look, &app.about)].spacing(2);
    if app.account {
        left = left.push(note(look, "Needs an account"));
    }
    let beside = if catalog::is_installed(state, &app.id) {
        Some(note(look, "Installed"))
    } else if let Some(work) = state
        .work
        .iter()
        .rev()
        .find(|work| work.id == app.id && work.doing.pending())
    {
        Some(note(look, work.verb()))
    } else {
        state
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.size(app))
            .map(|size| said(look, size.to_string()))
    };
    pressable(
        look,
        left.width(Fill).into(),
        beside,
        false,
        Message::Open(app.id.clone(), app.remote.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::tests::{answered, listed};
    use librift::flatpak::FLATHUB;

    #[test]
    fn the_front_page_says_what_each_app_takes_or_that_it_is_there() {
        let mut state = Store::bare();
        state.apps = Ok(listed());
        state.catalog = Some(Box::new(answered(&[FLATHUB], &["org.gimp.GIMP"])));
        let said = lines(&state);
        assert!(said.contains(&"remotes flathub".to_string()));
        assert!(said.contains(&"sizes flathub 1".to_string()));
        assert!(said.contains(&"app org.videolan.VLC 139.4 MB".to_string()));
        assert!(said.contains(&"app org.gimp.GIMP installed".to_string()));
        // the app from the test's own remote is not listed while that remote is not there
        assert!(!said.iter().any(|line| line.contains("dev.rift.TestEditor")));
    }

    #[test]
    fn a_remote_that_did_not_answer_is_said_once() {
        let mut state = Store::bare();
        state.apps = Ok(listed());
        state.asking = true;
        assert!(lines(&state).contains(&"remotes asking".to_string()));
        state.asking = false;
        assert!(lines(&state).contains(&"remotes unknown".to_string()));
        let mut catalog = answered(&[FLATHUB], &[]);
        catalog.sizes = vec![(FLATHUB.to_string(), Err("No network.".to_string()))];
        state.catalog = Some(Box::new(catalog));
        let said = lines(&state);
        assert!(said.contains(&"sizes flathub problem No network.".to_string()));
        assert!(said.contains(&"app org.videolan.VLC unknown".to_string()));
    }
}
