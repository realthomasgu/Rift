//! The apps that are installed, as flatpak lists them: the name, the room it takes and the remote
//! it came from, with Remove on each row. The apps of the image are not here. They are part of the
//! system and cannot be taken off one at a time.

use iced::widget::{column, row, space, text};
use iced::{Center, Element, Fill};
use librift::flatpak::Listed;

use crate::jobs;
use crate::theme::Colors;
use crate::ui::{Message, Store, said};
use crate::widgets::{GAP, TEXT_SIZE, action, group, line, note, progress};

/// The lines `--state` prints: how many apps are installed and each one's id.
#[must_use]
pub fn lines(state: &Store) -> Vec<String> {
    let Some(catalog) = state.catalog.as_ref() else {
        return vec!["installs unknown".to_string()];
    };
    let mut said = vec![format!("installs {}", catalog.installed.len())];
    for one in &catalog.installed {
        said.push(format!("install {} {}", one.id, one.name));
    }
    said
}

/// The page.
pub fn view(state: &Store, look: Colors) -> Element<'_, Message> {
    let mut page = column![note(
        look,
        "The apps you have installed. The browser, the terminal and the rest of what the drive \
         came with are part of the system and are not listed here.",
    )]
    .spacing(GAP)
    .width(Fill);
    let Some(catalog) = state.catalog.as_ref() else {
        return page.push(note(look, "Asking flatpak.")).into();
    };
    if let Err(why) = &catalog.remotes {
        page = page.push(text(why.as_str()).size(TEXT_SIZE).color(look.error));
    }
    if catalog.installed.is_empty() {
        return page
            .push(note(look, "Nothing is installed from a remote yet."))
            .into();
    }
    let rows = catalog
        .installed
        .iter()
        .map(|one| app_row(state, look, one))
        .collect();
    page.push(group(look, rows)).into()
}

/// One row: the name with the id under it, and either how far a remove has got or the room it takes
/// with the button that takes it off. The row itself opens the app's own page.
fn app_row<'a>(state: &'a Store, look: Colors, one: &'a Listed) -> Element<'a, Message> {
    let left = column![line(look, &one.name), note(look, &one.id)].spacing(2);
    let busy = state
        .work
        .iter()
        .rev()
        .find(|work| work.id == one.id && work.doing.pending());
    let beside: Element<'a, Message> = if let Some(work) = busy {
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
    } else {
        let mut beside = row![].align_y(Center).spacing(GAP);
        if !one.size.is_empty() {
            beside = beside.push(said(look, one.size.clone()));
        }
        beside
            .push(action(
                look,
                "Remove",
                Some(Message::Take(one.id.clone(), one.name.clone())),
            ))
            .into()
    };
    crate::widgets::pressable(
        look,
        left.width(Fill).into(),
        Some(beside),
        false,
        Message::Open(one.id.clone(), one.remote.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::tests::answered;
    use librift::flatpak::FLATHUB;

    #[test]
    fn the_page_lists_what_is_installed() {
        let mut state = Store::bare();
        assert_eq!(lines(&state), ["installs unknown"]);
        state.catalog = Some(Box::new(answered(&[FLATHUB], &[])));
        assert_eq!(lines(&state), ["installs 0"]);
        state.catalog = Some(Box::new(answered(
            &[FLATHUB],
            &["org.videolan.VLC", "dev.rift.TestEditor"],
        )));
        assert_eq!(
            lines(&state),
            [
                "installs 2",
                "install org.videolan.VLC VLC",
                "install dev.rift.TestEditor TestEditor",
            ]
        );
    }
}
