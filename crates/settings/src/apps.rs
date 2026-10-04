//! The Apps page: which app opens each kind of file, and a sentence for what each app may reach.
//!
//! The defaults are kept in the owner's and the image's `mimeapps.list` files, which xdg-mime and
//! `GLib` read, so the page reads them as it comes up and every two seconds while it is up, and a
//! choice made with xdg-mime in a terminal shows here too. A kind with more than one app on the
//! drive opens its list under its row, and pressing an app there makes it the default for every
//! type of the kind, in the owner's own list.

use std::thread;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::widget::{button, column, container, row, space, text};
use iced::{Border, Center, Color, Element, Fill, Length, Subscription, Task, Theme};
use librift::apps::App;
use librift::defaults::{self, Found, Kind, Opens};

use crate::ghost;
use crate::icons;
use crate::theme::Colors;
use crate::ui::{Message, Settings};
use crate::widgets::{GAP, TEXT_SIZE, group, heading, line, note, pressable};

/// How often the page reads the lists again while it is up.
const EVERY: Duration = Duration::from_secs(2);
/// How big an app's icon is in a row.
const ICON: f32 = 24.0;
/// How far the apps of an open row stand in from its edge.
const INDENT: f32 = 20.0;
/// The name `rift-settings --set` takes for this page.
pub const NAMES: [&str; 1] = ["default-app"];

/// What opens each kind now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// Every kind, with its default and the apps that open it.
    pub kinds: Vec<Opens>,
}

/// Read the lists now. They are a few small files, so the window reads them itself as the page
/// comes up, and after a choice.
#[must_use]
pub fn reading(apps: &[App]) -> Picture {
    Picture {
        kinds: Found::read().opens(apps),
    }
}

/// The lists while the page is up: read every two seconds, on a thread that ends at the first send
/// after the page has gone. The desktop entries are read once, when it starts.
pub fn following() -> Subscription<Message> {
    Subscription::run_with("apps", |_| {
        let (sender, receiver) = mpsc::unbounded();
        thread::spawn(move || {
            let apps = librift::apps::load();
            while sender
                .unbounded_send(Message::Kinds(reading(&apps)))
                .is_ok()
            {
                thread::sleep(EVERY);
            }
        });
        receiver
    })
}

/// What the owner asked for on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// Open or close the list of apps under a kind's row.
    Unfold(Kind),
    /// Make this app, by its entry's id, the one every type of the kind opens with.
    Choose(Kind, String),
}

/// Do what the owner asked. A choice is written at once, since it is one small file, and the page
/// then shows what the lists say.
pub fn asked(state: &mut Settings, asked: Asked) -> Task<Message> {
    match asked {
        Asked::Unfold(kind) => {
            state.unfolded = (state.unfolded != Some(kind)).then_some(kind);
        }
        Asked::Choose(kind, app) => {
            state.problem = defaults::choose(kind, &app).err();
            state.unfolded = None;
            state.defaults = Some(reading(&state.apps));
        }
    }
    Task::none()
}

/// What `rift-settings --set default-app <kind> <app>` asks, the way pressing it would: the kind by
/// its word and an app that opens it by its entry's id, with or without `.desktop`. Nothing for a
/// kind that is not one or an app that does not open it.
#[must_use]
pub fn named(state: &Settings, name: &str, value: &str) -> Option<Asked> {
    if name != "default-app" {
        return None;
    }
    let (word, app) = value.trim().split_once(' ')?;
    let kind = Kind::from_word(word)?;
    let app = defaults::entry_id(app.trim());
    let opens = state
        .defaults
        .as_ref()?
        .kinds
        .iter()
        .find(|opens| opens.kind == kind)?;
    opens
        .apps
        .iter()
        .any(|id| id == app)
        .then(|| Asked::Choose(kind, app.to_string()))
}

/// The lines `rift-settings --state` prints, once the page has read the lists: for every kind, its
/// word, the type it opens with and the desktop id of the app that opens it, as `xdg-mime query
/// default` prints it, then the ids of the apps that open it.
#[must_use]
pub fn state(state: &Settings) -> Vec<String> {
    let Some(now) = state.defaults.as_ref() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for opens in &now.kinds {
        lines.push(format!(
            "default-app {} {} {}",
            opens.kind.word(),
            opens.kind.main_type(),
            opens.default.as_deref().unwrap_or("none")
        ));
        lines.push(
            format!("can-open {} {}", opens.kind.word(), opens.apps.join(" "))
                .trim_end()
                .to_string(),
        );
    }
    lines
}

/// The page.
pub fn view(state: &Settings, look: Colors) -> Element<'_, Message> {
    let Some(now) = state.defaults.as_ref() else {
        return note(look, "Reading which app opens each kind of file.");
    };
    let mut rows: Vec<Element<'_, Message>> = Vec::new();
    // a kind no app on the drive opens has no row: mail, until a mail app is installed
    for opens in now
        .kinds
        .iter()
        .filter(|opens| !opens.apps.is_empty() || opens.default.is_some())
    {
        rows.push(kind_row(state, look, opens));
        if state.unfolded == Some(opens.kind) && opens.choosable() {
            for id in &opens.apps {
                rows.push(app_row(state, look, opens, id));
            }
        }
    }
    let mut defaults = column![
        heading(look, "Default apps"),
        group(look, rows),
        note(look, DEFAULTS)
    ]
    .spacing(8);
    // the owner's own list is a file under home, which is memory in a Ghost boot: a choice made
    // here opens files for this login and is on no drive afterwards (ADR-0084)
    if ghost::on() {
        defaults = defaults.push(ghost::only_now(look, FOR_THIS_SESSION));
    }
    let mut page = column![
        defaults,
        column![heading(look, "App permissions"), note(look, PERMISSIONS)].spacing(8),
    ]
    .spacing(GAP)
    .width(Fill);
    if let Some(why) = &state.problem {
        page = page.push(text(why).size(TEXT_SIZE).color(look.error));
    }
    page.into()
}

/// A kind's row: its name, and the app it opens with in the app's own colours. A kind there is
/// another app for is pressed to open its list.
fn kind_row<'a>(state: &'a Settings, look: Colors, opens: &'a Opens) -> Element<'a, Message> {
    let app = opens
        .default_id()
        .and_then(|id| state.apps.iter().find(|app| app.id == id));
    let named = match (app, opens.default_id()) {
        (Some(app), _) => row![
            icons::of_app(look.text, app.icon.as_deref(), ICON),
            text(app.name.clone()).size(TEXT_SIZE).color(look.dim),
        ],
        // a cache can name an app whose entry is not here, which goes by its id
        (None, Some(id)) => row![text(id.to_string()).size(TEXT_SIZE).color(look.dim)],
        (None, None) => row![text("None").size(TEXT_SIZE).color(look.dim)],
    };
    let mut right = named.spacing(12).align_y(Center);
    let open = state.unfolded == Some(opens.kind);
    if opens.choosable() {
        let arrow = if open {
            "pan-up-symbolic"
        } else {
            "pan-down-symbolic"
        };
        right = right.push(icons::symbolic(look.dim, arrow, 16.0));
    }
    let inside = row![container(line(look, opens.kind.label())).width(Fill), right]
        .align_y(Center)
        .spacing(GAP)
        .height(Length::Fixed(ICON));
    if !opens.choosable() {
        return container(inside).width(Fill).padding([8, 12]).into();
    }
    button(inside)
        .width(Fill)
        .padding([8, 12])
        .on_press(Message::Apps(Asked::Unfold(opens.kind)))
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
        })
        .into()
}

/// One app of an open row, the mark on the one the kind opens with. An app that runs in a terminal
/// says so, since a file opens with it in a terminal window of its own.
fn app_row<'a>(
    state: &'a Settings,
    look: Colors,
    opens: &'a Opens,
    id: &'a str,
) -> Element<'a, Message> {
    let app = state.apps.iter().find(|app| app.id == id);
    let name = app.map_or(id, |app| app.name.as_str());
    let mut words = column![line(look, name)].spacing(2);
    if app.is_some_and(|app| app.terminal) {
        words = words.push(note(look, "Opens in a terminal window"));
    }
    let left = row![
        space().width(INDENT),
        icons::of_app(look.text, app.and_then(|app| app.icon.as_deref()), ICON),
        words,
    ]
    .spacing(12)
    .align_y(Center);
    pressable(
        look,
        left.into(),
        None,
        opens.default_id() == Some(id),
        Message::Apps(Asked::Choose(opens.kind, id.to_string())),
    )
}

/// Under the default apps.
const DEFAULTS: &str = "Each kind of file, and each link another app hands on, opens with the app \
                        beside it. Where the drive has more than one app for a kind, press its row \
                        to choose another.";
/// What a choice made in a Ghost boot is.
const FOR_THIS_SESSION: &str =
    "These are the image's own here, and an app can be chosen for this session";

/// What the page cannot do yet.
const PERMISSIONS: &str = "What each app may reach is not in Settings yet. rift run --sandbox \
                           starts a program that sees only the folder it runs in, and Privacy and \
                           security keeps what each app was told about the camera.";

#[cfg(test)]
mod tests {
    use super::*;
    use librift::apps::Category;

    fn app(id: &str, name: &str, terminal: bool) -> App {
        App {
            id: id.to_string(),
            name: name.to_string(),
            exec: vec![id.to_lowercase()],
            terminal,
            icon: None,
            wm_class: None,
            category: Category::Accessories,
            types: vec!["text/plain".to_string()],
            line: String::new(),
        }
    }

    fn settings() -> Settings {
        let mut state = Settings::bare();
        state.apps = vec![
            app("dev.zed.Zed", "Zed", false),
            app("Helix", "Helix", true),
        ];
        state.defaults = Some(Picture {
            kinds: vec![
                Opens {
                    kind: Kind::Web,
                    default: Some("firefox.desktop".to_string()),
                    apps: vec!["firefox".to_string()],
                },
                Opens {
                    kind: Kind::Mail,
                    default: None,
                    apps: Vec::new(),
                },
                Opens {
                    kind: Kind::Text,
                    default: Some("dev.zed.Zed.desktop".to_string()),
                    apps: vec!["dev.zed.Zed".to_string(), "Helix".to_string()],
                },
            ],
        });
        state
    }

    #[test]
    fn the_state_says_what_opens_each_kind_as_xdg_mime_would() {
        assert!(state(&Settings::bare()).is_empty());
        assert_eq!(
            state(&settings()),
            [
                "default-app web x-scheme-handler/http firefox.desktop",
                "can-open web firefox",
                "default-app mail x-scheme-handler/mailto none",
                "can-open mail",
                "default-app text text/plain dev.zed.Zed.desktop",
                "can-open text dev.zed.Zed Helix",
            ]
        );
    }

    #[test]
    fn a_setting_from_a_terminal_is_what_the_page_would_press() {
        let kept = settings();
        assert_eq!(
            named(&kept, "default-app", "text Helix"),
            Some(Asked::Choose(Kind::Text, "Helix".into()))
        );
        assert_eq!(
            named(&kept, "default-app", " text  Helix.desktop "),
            Some(Asked::Choose(Kind::Text, "Helix".into()))
        );
        // only an app that opens the kind, and only a kind there is
        assert_eq!(named(&kept, "default-app", "web Helix"), None);
        assert_eq!(named(&kept, "default-app", "calendar Helix"), None);
        assert_eq!(named(&kept, "default-app", "text"), None);
        assert_eq!(named(&Settings::bare(), "default-app", "text Helix"), None);
    }

    #[test]
    fn a_row_opens_and_closes() {
        let mut kept = settings();
        let _ = asked(&mut kept, Asked::Unfold(Kind::Text));
        assert_eq!(kept.unfolded, Some(Kind::Text));
        let _ = asked(&mut kept, Asked::Unfold(Kind::Text));
        assert_eq!(kept.unfolded, None);
    }

    #[test]
    fn the_sentences_are_sentences() {
        for sentence in [DEFAULTS, PERMISSIONS] {
            assert!(sentence.ends_with('.'), "{sentence}");
            assert!(sentence.is_ascii(), "{sentence}");
        }
    }
}
