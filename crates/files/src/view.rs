//! How a window is drawn: the header bar along the top with the way back and forward, the path of
//! the folder and the search field, the places and the drives down the left, the list beside them,
//! and over the list whatever stands there for a moment: the progress of a job, a toast, what is
//! selected, a menu or a dialog. A bar under the header says what the window is showing when it is
//! not simply the folder: a moment in the Timeline, or a search by meaning.

use std::path::{Path, PathBuf};

use iced::widget::{button, column, container, mouse_area, opaque, pin, row, space, stack, text};
use iced::{Border, Center, Color, Element, Fill, Length, Theme, window};
use librift::drives::{self, Volume};

use crate::actions::PATIENCE;
use crate::browser::{Browser, Location};
use crate::dialogs;
use crate::icons;
use crate::jobs::Job;
use crate::list::{self, HEADS, path_id, search_id};
use crate::menus;
use crate::theme::Colors;
use crate::timeline;
use crate::ui::{Act, Files, Message};
use crate::widgets::{
    BOLD, TEXT_SIZE, action, fill, menu, primary, progress, scroll, shade, toast, tool, wide_field,
};

/// How wide the sidebar is.
pub const SIDEBAR: f32 = 208.0;
/// How tall the header bar is. It is the title bar of the window, which the app draws itself.
pub const HEADER: f32 = 45.0;
/// Where the first row of the list is from the top of the window, under the bar when there is one.
pub const LIST_TOP: f32 = HEADER + 1.0 + HEADS + 1.0;
/// How tall a row of the sidebar is.
const SIDE_ROW: f32 = 34.0;
/// How much of a row the eject button takes at its right end.
const EJECT: f32 = 30.0;
/// How many parts of a path the path bar shows before it leaves out the middle.
const CRUMBS: usize = 5;
/// How tall the bar under the header is.
const BAR: f32 = 40.0;

/// A window.
pub fn window(state: &Files, id: window::Id) -> Element<'_, Message> {
    let Some(browser) = state.windows.get(&id) else {
        return space().into();
    };
    let look = state.colors();
    let listing = container(list::view(state, id, browser, look))
        .width(Fill)
        .height(Fill)
        .style(move |_: &Theme| fill(look.view));
    // the bar stands over the list, so the places down the left run from the header to the bottom
    let beside: Element<'_, Message> = match bar(state, id, browser, look) {
        Some(bar) => column![bar, listing].into(),
        None => listing.into(),
    };
    let body = row![sidebar(state, id, browser, look), beside].height(Fill);
    let mut layers: Vec<Element<'_, Message>> =
        vec![column![header(browser, id, look), body].into()];
    if let Some(bottom) = bottom(state, id, browser, look) {
        layers.push(
            container(bottom)
                .width(Fill)
                .height(Fill)
                .padding(iced::Padding {
                    left: SIDEBAR + 16.0,
                    ..iced::Padding::new(16.0)
                })
                .align_bottom(Fill)
                .center_x(Fill)
                .into(),
        );
    }
    if let Some(summary) = summary(browser, look) {
        layers.push(
            container(summary)
                .width(Fill)
                .height(Fill)
                .padding(12)
                .align_bottom(Fill)
                .align_right(Fill)
                .into(),
        );
    }
    if let Some(open) = &browser.menu {
        // a press anywhere else closes the menu, the way a popover goes
        layers.push(
            mouse_area(space().width(Fill).height(Fill))
                .on_press(Message::CloseMenu(id))
                .on_right_press(Message::CloseMenu(id))
                .into(),
        );
        let items = menus::items(state, browser, id, open);
        layers.push(pin(menu(look, items)).x(open.at.x).y(open.at.y).into());
    }
    if let Some(shown) = &browser.dialog {
        let folder = browser
            .location
            .folder()
            .map_or_else(|| PathBuf::from("/"), Path::to_path_buf);
        layers.push(opaque(shade()));
        layers.push(
            container(dialogs::view(shown, look, id, browser.number, &folder))
                .width(Fill)
                .height(Fill)
                .center(Fill)
                .into(),
        );
    }
    stack(layers).into()
}

/// The title bar the app draws for itself: the app's name over the sidebar, back and forward, the
/// path of the folder, the menu, and the close button at the right end, the way GNOME's Files lays
/// its own out.
fn header(browser: &Browser, id: window::Id, look: Colors) -> Element<'_, Message> {
    let name = container(text("Files").size(TEXT_SIZE).font(BOLD).color(look.text))
        .width(Length::Fixed(SIDEBAR - 8.0))
        .padding([0, 6])
        .center_y(Fill);
    let back = tool(
        look,
        "go-previous-symbolic",
        (!browser.back.is_empty()).then_some(Message::Back(id)),
    );
    let forward = tool(
        look,
        "go-next-symbolic",
        (!browser.forward.is_empty()).then_some(Message::Forward(id)),
    );
    let close = button(icons::symbolic(look.text, "window-close-symbolic", 16.0))
        .padding(7)
        .on_press(Message::Do(id, Act::Close))
        .style(move |_: &Theme, status| button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => look.hover,
                    _ => look.button,
                }
                .into(),
            ),
            text_color: look.text,
            border: Border {
                radius: 15.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        });
    let line = container(space().height(1.0).width(Fill)).style(move |_: &Theme| fill(look.line));
    let mut tools = row![name, back, forward, path_bar(browser, id, look)]
        .align_y(Center)
        .spacing(4)
        .padding([0, 8]);
    // the trash is not a folder to walk, so it has nothing to search
    if browser.location.place().is_some() {
        tools = tools.push(tool(
            look,
            "edit-find-symbolic",
            Some(Message::Do(id, Act::Search)),
        ));
    }
    // only home is snapshotted, so only a folder in it has a Timeline to show
    if browser.location.about().is_some_and(timeline::covers) {
        tools = tools.push(tool(
            look,
            "document-open-recent-symbolic",
            Some(Message::Do(id, Act::Timeline)),
        ));
    }
    column![
        container(
            tools
                .push(tool(
                    look,
                    "open-menu-symbolic",
                    Some(Message::MainMenu(id))
                ))
                .push(close),
        )
        .width(Fill)
        .height(Length::Fixed(HEADER))
        .style(move |_: &Theme| fill(look.header)),
        line,
    ]
    .into()
}

/// The path of the folder as a button for each folder on the way to it, the last in bold, in a box
/// of its own. A press between them, or Ctrl and L, makes it a field to type a path in.
fn path_bar(browser: &Browser, id: window::Id, look: Colors) -> Element<'_, Message> {
    if let Some(query) = &browser.search {
        return container(wide_field(
            look,
            "Search this folder",
            &query.words,
            search_id(browser.number),
            move |typed| Message::SearchTyped(id, typed),
            Message::SearchEntered(id),
        ))
        .width(Fill)
        .padding([0, 6])
        .into();
    }
    if let Some(typed) = &browser.typing {
        return container(wide_field(
            look,
            "A folder to go to",
            typed,
            path_id(browser.number),
            move |typed| Message::PathTyped(id, typed),
            Message::PathEntered(id),
        ))
        .width(Fill)
        .padding([0, 6])
        .into();
    }
    let mut crumbs = row![].spacing(0).align_y(Center);
    match &browser.location {
        Location::Trash => {
            crumbs = crumbs.push(crumb(look, "Trash".to_string(), true, None));
        }
        Location::Folder(path) | Location::Moment { folder: path, .. } => {
            let parts = parts(path);
            let count = parts.len();
            let hidden_to = count.saturating_sub(CRUMBS - 2);
            for (at, (label, place)) in parts.into_iter().enumerate() {
                // a deep folder shows where it starts and the last few folders on the way, with a
                // button for the last of the ones left out between them
                if count > CRUMBS && at > 0 && at < hidden_to {
                    if at + 1 == hidden_to {
                        crumbs = crumbs.push(slash(look));
                        crumbs = crumbs.push(tool(
                            look,
                            "pan-start-symbolic",
                            Some(Message::Go(id, at_the_same_time(&browser.location, place))),
                        ));
                    }
                    continue;
                }
                if at > 0 {
                    crumbs = crumbs.push(slash(look));
                }
                let last = at + 1 == count;
                crumbs = crumbs.push(crumb(
                    look,
                    label,
                    last,
                    Some(Message::Go(id, at_the_same_time(&browser.location, place))),
                ));
            }
        }
    }
    let boxed = container(crumbs.padding([0, 4]))
        .width(Fill)
        .height(Length::Fixed(32.0))
        .center_y(Length::Fixed(32.0))
        .style(move |_: &Theme| container::Style {
            background: Some(look.field.into()),
            border: Border {
                color: look.edge,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..container::Style::default()
        });
    container(mouse_area(boxed).on_press(Message::Do(id, Act::Location)))
        .width(Fill)
        .padding([0, 6])
        .into()
}

/// The folders on the way to a folder, with their names: Home and what is under it, or the root
/// and what is under that.
fn parts(path: &Path) -> Vec<(String, PathBuf)> {
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from);
    let (mut parts, start) = match home.as_deref().filter(|home| path.starts_with(home)) {
        Some(home) => (
            vec![("Home".to_string(), home.to_path_buf())],
            home.to_path_buf(),
        ),
        None => (
            vec![("/".to_string(), PathBuf::from("/"))],
            PathBuf::from("/"),
        ),
    };
    let mut place = start.clone();
    if let Ok(rest) = path.strip_prefix(&start) {
        for part in rest.components() {
            place.push(part);
            parts.push((
                part.as_os_str().to_string_lossy().into_owned(),
                place.clone(),
            ));
        }
    }
    parts
}

/// A folder on the way to this one, as the window would show it: as it is now, or as it was at
/// the moment the window is showing.
fn at_the_same_time(location: &Location, folder: PathBuf) -> Location {
    match location.at() {
        Some(at) => Location::Moment {
            at: at.to_string(),
            folder,
        },
        None => Location::Folder(folder),
    }
}

/// How far the first row of the list is from the top of the window: further down when a bar stands
/// over it, and further up in the grid, which has no headings over it.
#[must_use]
pub fn list_top(state: &Files, browser: &Browser) -> f32 {
    let top = if state.options.grid {
        HEADER + 1.0
    } else {
        LIST_TOP
    };
    if bar_words(state, browser).is_some() {
        top + BAR + 1.0
    } else {
        top
    }
}

/// What the bar says, when there is one to draw.
fn bar_words(state: &Files, browser: &Browser) -> Option<String> {
    if let Some(at) = browser.location.at() {
        return Some(moment_words(state, at));
    }
    let query = browser.search.as_ref()?;
    match (&query.problem, query.meaning) {
        (Some(why), _) => Some(why.clone()),
        (None, true) => Some("Closest in meaning first.".to_string()),
        (None, false) => None,
    }
}

/// The bar under the header bar, when the window is showing something other than the folder as it
/// is: a moment in the Timeline, with the way through the moments and back to now, or a sentence
/// about a search by meaning.
fn bar<'a>(
    state: &'a Files,
    id: window::Id,
    browser: &'a Browser,
    look: Colors,
) -> Option<Element<'a, Message>> {
    let said = bar_words(state, browser)?;
    let buttons = if browser.location.at().is_some() {
        moment_buttons(state, id, browser, look)
    } else {
        vec![action(
            look,
            "Back to the folder",
            Some(Message::Escape(id)),
        )]
    };
    let mut inside = row![
        container(
            text(said)
                .size(TEXT_SIZE)
                .color(look.text)
                .wrapping(text::Wrapping::None)
        )
        .width(Fill)
        .clip(true)
    ]
    .align_y(Center)
    .spacing(8)
    .padding([0, 12]);
    for button in buttons {
        inside = inside.push(button);
    }
    Some(
        column![
            container(inside)
                .width(Fill)
                .height(Length::Fixed(BAR))
                .style(move |_: &Theme| fill(look.side)),
            container(space().height(1.0).width(Fill)).style(move |_: &Theme| fill(look.line)),
        ]
        .into(),
    )
}

/// What the bar says about a moment: the day and the time the snapshot was taken.
fn moment_words(state: &Files, at: &str) -> String {
    format!(
        "As it was {}",
        timeline::label(at, librift::time::now(), state.offset)
    )
}

/// The buttons of a moment: through the moments Vault has, the one that puts things back, and the
/// way out of the Timeline.
fn moment_buttons<'a>(
    state: &Files,
    id: window::Id,
    browser: &Browser,
    look: Colors,
) -> Vec<Element<'a, Message>> {
    let at = browser.location.at().unwrap_or_default();
    let step = |earlier: bool, label: &'static str| {
        action(
            look,
            label,
            timeline::step(&state.moments, at, earlier)
                .map(|_| Message::Do(id, Act::Step { earlier })),
        )
    };
    let selected = !browser.selected.is_empty();
    vec![
        step(true, "Earlier"),
        step(false, "Later"),
        action(
            look,
            timeline::restoring(selected),
            browser.ready.then_some(Message::Do(id, Act::Bring)),
        ),
        primary(look, "Back to now", Some(Message::Do(id, Act::Now))),
    ]
}

fn slash<'a>(look: Colors) -> Element<'a, Message> {
    text("/").size(TEXT_SIZE).color(look.dim).into()
}

/// One folder of the path bar, pressed to go there.
fn crumb<'a>(
    look: Colors,
    label: String,
    last: bool,
    press: Option<Message>,
) -> Element<'a, Message> {
    let words = text(label)
        .size(TEXT_SIZE)
        .color(look.text)
        .font(if last { BOLD } else { crate::widgets::FONT })
        .wrapping(text::Wrapping::None);
    let mut pressable = button(words)
        .padding([4, 8])
        .style(move |_: &Theme, status| button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => look.hover,
                    _ => Color::TRANSPARENT,
                }
                .into(),
            ),
            text_color: look.text,
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        });
    if let Some(press) = press {
        pressable = pressable.on_press(press);
    }
    pressable.into()
}

/// The places, one row each, then the exchange partition of the drive and the disks that are
/// plugged in, then the trash, the way GNOME's Files lists them. The place the window shows is in
/// the accent.
fn sidebar<'a>(
    state: &'a Files,
    id: window::Id,
    browser: &Browser,
    look: Colors,
) -> Element<'a, Message> {
    let mut rows = column![].width(Fill).spacing(2).padding([8, 8]);
    for place in &state.places {
        // a moment of a folder is still that folder, so its row stays marked in the Timeline
        let here = browser.location.about() == Some(place.path.as_path());
        rows = rows.push(side_row(
            look,
            place.icon,
            place.name.clone(),
            here,
            Some(Message::Go(id, Location::Folder(place.path.clone()))),
        ));
    }
    if !state.drives.is_empty() || state.exchange.listed() {
        rows = rows.push(space().height(8.0));
    }
    if state.exchange.listed() {
        // the drive's own, so it is a folder and never a disk to eject. Every boot but a Ghost one
        // has it mounted before anyone logs in; there a press is what mounts it, the way a press
        // mounts a disk that was plugged in
        let busy = state.working.iter().any(|busy| busy == "exchange");
        rows = rows.push(side_row(
            look,
            "drive-harddisk-symbolic",
            drives::EXCHANGE_NAME.to_string(),
            state
                .exchange
                .mount()
                .is_some_and(|path| under(browser, path)),
            (!busy).then_some(Message::Do(id, Act::MountExchange)),
        ));
    }
    for drive in &state.drives {
        rows = rows.push(drive_row(state, id, browser, look, drive));
    }
    rows = rows.push(space().height(8.0));
    rows = rows.push(side_row(
        look,
        if state.trash_full {
            "user-trash-full-symbolic"
        } else {
            "user-trash-symbolic"
        },
        "Trash".to_string(),
        browser.location == Location::Trash,
        Some(Message::Go(id, Location::Trash)),
    ));
    row![
        container(scroll(look, rows).height(Fill))
            .width(Length::Fixed(SIDEBAR))
            .height(Fill)
            .style(move |_: &Theme| fill(look.side)),
        container(space().width(1.0).height(Fill)).style(move |_: &Theme| fill(look.line)),
    ]
    .into()
}

/// Whether a window is showing a folder on this drive.
fn under(browser: &Browser, mount: &Path) -> bool {
    browser
        .location
        .about()
        .is_some_and(|folder| folder.starts_with(mount))
}

/// One disk in the sidebar: a press mounts it, or opens it when it is mounted already, and the
/// button at its right end unmounts it and ejects it, so the stick can be pulled out. A locked
/// disk says so, and a press on it asks for its passphrase.
fn drive_row<'a>(
    state: &Files,
    id: window::Id,
    browser: &Browser,
    look: Colors,
    drive: &Volume,
) -> Element<'a, Message> {
    let busy = state.working.contains(&drive.id);
    let here = drive
        .mount
        .as_deref()
        .is_some_and(|mount| under(browser, mount));
    let name = if drive.locked {
        format!("{} (locked)", drive.name)
    } else {
        drive.name.clone()
    };
    let press = (!busy).then(|| match (&drive.mount, drive.locked) {
        (_, true) => Message::Do(id, Act::Unlock(drive.id.clone())),
        (Some(mount), _) => Message::Go(id, Location::Folder(mount.clone())),
        (None, _) => Message::Do(id, Act::Mount(drive.id.clone())),
    });
    let row = side_row_with(look, drive.icon, name, here, press, EJECT);
    if !drive.mounted() {
        return row;
    }
    let colour = if here { look.on_accent } else { look.text };
    let eject = button(icons::symbolic(colour, "media-eject-symbolic", 16.0))
        .padding(6)
        .on_press_maybe((!busy).then(|| Message::Do(id, Act::Eject(drive.id.clone()))))
        .style(move |_: &Theme, status| button::Style {
            background: Some(
                match status {
                    button::Status::Hovered | button::Status::Pressed => look.hover,
                    _ => Color::TRANSPARENT,
                }
                .into(),
            ),
            text_color: colour,
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        });
    container(stack![
        row,
        container(eject)
            .width(Fill)
            .height(Length::Fixed(SIDE_ROW))
            .align_right(Fill)
            .center_y(Fill)
            .padding([0, 4]),
    ])
    .height(Length::Fixed(SIDE_ROW))
    .into()
}

/// One row of the sidebar, the way Settings draws its own.
fn side_row<'a>(
    look: Colors,
    icon: &str,
    label: String,
    here: bool,
    press: Option<Message>,
) -> Element<'a, Message> {
    side_row_with(look, icon, label, here, press, 0.0)
}

/// The same row with room kept at its right end for a button that stands over it.
fn side_row_with<'a>(
    look: Colors,
    icon: &str,
    label: String,
    here: bool,
    press: Option<Message>,
    trailing: f32,
) -> Element<'a, Message> {
    let colour = if here { look.on_accent } else { look.text };
    // a button lays its content out at the top of its box, so the row is centred by hand
    button(
        container(
            row![
                icons::symbolic(colour, icon, 16.0),
                container(
                    text(label)
                        .size(TEXT_SIZE)
                        .color(colour)
                        .wrapping(text::Wrapping::None)
                )
                .width(Fill)
                .clip(true),
                space().width(trailing),
            ]
            .align_y(Center)
            .spacing(10),
        )
        .center_y(Fill),
    )
    .width(Fill)
    .height(Length::Fixed(SIDE_ROW))
    .padding([0, 10])
    .on_press_maybe(press)
    .style(move |_: &Theme, status| button::Style {
        background: Some(
            match (here, status) {
                (true, _) => look.accent,
                (false, button::Status::Hovered | button::Status::Pressed) => look.hover,
                _ => Color::TRANSPARENT,
            }
            .into(),
        ),
        text_color: colour,
        border: Border {
            radius: 4.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    })
    .into()
}

/// What stands at the bottom of the list: the job this window started that has taken a while, with
/// how far it has got, or else the toast.
fn bottom<'a>(
    state: &'a Files,
    id: window::Id,
    browser: &'a Browser,
    look: Colors,
) -> Option<Element<'a, Message>> {
    let slow = state
        .jobs
        .iter()
        .find(|job| job.running() && job.window == Some(id) && job.started.elapsed() >= PATIENCE);
    if let Some(job) = slow {
        return Some(working(job, id, look));
    }
    let said = browser.toast.as_ref()?;
    Some(toast(
        look,
        said.said.clone(),
        said.undo
            .map(|number| ("Undo", Message::Do(id, Act::Undo(number)))),
    ))
}

/// A job that is running: what it is doing, how far it has got, and Stop when it can stop.
fn working(job: &Job, id: window::Id, look: Colors) -> Element<'_, Message> {
    let mut inside = row![
        text(job.work.doing()).size(TEXT_SIZE).color(look.text),
        progress(look, job.percent()),
    ]
    .spacing(16)
    .align_y(Center);
    if job.work.stoppable() {
        inside = inside.push(action(
            look,
            "Stop",
            Some(Message::Do(id, Act::Stop(job.number))),
        ));
    }
    container(inside)
        .padding([8, 14])
        .style(move |_: &Theme| container::Style {
            background: Some(look.header.into()),
            border: Border {
                color: look.edge,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// What is selected, at the bottom right of the list, the way GNOME's Files says it: one thing by
/// its name with its size or what it holds, several by how many and their size together.
fn summary<'a>(browser: &Browser, look: Colors) -> Option<Element<'a, Message>> {
    if browser.toast.is_some() || browser.menu.is_some() {
        return None;
    }
    let chosen = browser.chosen();
    let said = match chosen.as_slice() {
        [] => return None,
        [one] => match (one.kind, one.items) {
            (librift::files::Kind::Folder, Some(items)) => format!(
                "{} selected, {}",
                one.label,
                librift::files::items_words(items).to_lowercase()
            ),
            (librift::files::Kind::Folder, None) => format!("{} selected", one.label),
            _ => format!(
                "{} selected, {}",
                one.label,
                librift::files::size_words(one.size)
            ),
        },
        more => {
            let bytes: u64 = more.iter().map(|entry| entry.size).sum();
            format!(
                "{} items selected, {}",
                more.len(),
                librift::files::size_words(bytes)
            )
        }
    };
    Some(
        container(
            text(said)
                .size(TEXT_SIZE)
                .color(look.dim)
                .wrapping(text::Wrapping::None),
        )
        .padding([6, 10])
        .style(move |_: &Theme| container::Style {
            background: Some(look.header.into()),
            border: Border {
                color: look.edge,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..container::Style::default()
        })
        .into(),
    )
}
