//! The Privacy and security page: what each app was told about the camera, whether apps remember
//! the files opened lately, the firewall and the network switch of each app that runs in a
//! sandbox, Flatpak apps among them, what fwupd says about the firmware, and a sentence each for
//! what there is nothing to set for: the microphone, location, the screen lock and what is sent
//! about the machine.
//!
//! The camera's answers are the portal's, kept in the permission store, and the apps on the
//! network switch are Airlock's, so the page reads all but fwupd as it comes up and every two
//! seconds while it is up: an app asking for the camera, or rift net off in a terminal, shows here
//! too. fwupd is asked once as the page comes up, since the firmware does not change while the
//! machine runs, and it may take a few seconds to start.

use std::thread;
use std::time::Duration;

use iced::futures::channel::{mpsc, oneshot};
use iced::widget::{column, container, row, text};
use iced::{Center, Element, Fill, Subscription, Task};
use librift::airlock;
use librift::privacy::{self, Answer, Security};

use crate::ai::said;
use crate::ghost;
use crate::icons;
use crate::theme::Colors;
use crate::ui::{Message, Settings};
use crate::widgets::{GAP, TEXT_SIZE, group, heading, line, note, setting, switch};

/// How often the page reads again while it is up.
const EVERY: Duration = Duration::from_secs(2);
/// How big an app's icon is in a row.
const ICON: f32 = 24.0;
/// The names `rift-settings --set` takes for this page.
pub const NAMES: [&str; 3] = ["camera-app", "recent-files", "app-network"];
/// How `--state` and `--set` write the id of the programs the portal cannot name, which is empty.
const NO_ID: &str = "-";

/// What the page reads while it is up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// What each app was told about the camera.
    pub camera: Result<Vec<Answer>, String>,
    /// Whether GTK apps remember the files opened lately.
    pub recent: bool,
    /// Whether the firewall's rules are loaded.
    pub firewall: Result<bool, String>,
    /// The apps on the network switch: every Flatpak app that is installed, every app whose
    /// network is off, and every app that runs in a sandbox now.
    pub sandboxes: Result<Vec<airlock::App>, String>,
}

/// Read everything but fwupd now. It asks three services and runs dconf, so it runs on a thread.
#[must_use]
pub fn reading() -> Picture {
    Picture {
        camera: privacy::camera(),
        recent: privacy::recent_files(),
        firewall: privacy::firewall(),
        sandboxes: airlock::apps(),
    }
}

/// The page while it is up: read as it comes up and every two seconds after, on a thread that ends
/// at the first send after the page has gone.
pub fn following() -> Subscription<Message> {
    Subscription::run_with("privacy", |_| {
        let (sender, receiver) = mpsc::unbounded();
        thread::spawn(move || {
            while sender
                .unbounded_send(Message::Privacy(Box::new(reading())))
                .is_ok()
            {
                thread::sleep(EVERY);
            }
        });
        receiver
    })
}

/// Read everything once, on a thread of its own, after a change.
fn read() -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(reading());
    });
    Task::perform(receiver, |read| {
        Message::Privacy(Box::new(read.unwrap_or_else(|_| Picture {
            camera: Err("It stopped before it finished.".to_string()),
            recent: true,
            firewall: Err("It stopped before it finished.".to_string()),
            sandboxes: Err("It stopped before it finished.".to_string()),
        })))
    })
}

/// Ask fwupd, on a thread of its own.
pub fn ask_fwupd() -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(privacy::security());
    });
    Task::perform(receiver, |answered| {
        Message::Security(answered.unwrap_or_else(|_| Err("fwupd did not answer.".to_string())))
    })
}

/// What the owner asked for on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// Let this app take the camera without asking, or refuse it without asking.
    Camera(String, bool),
    /// Remember the files opened lately, or not.
    Recent(bool),
    /// Give this app on the network switch the network, or take it away.
    Network(String, bool),
}

/// Do what the owner asked on a thread of its own, then read again, so the page shows what the
/// services did rather than what was asked for.
pub fn asked(state: &mut Settings, asked: Asked) -> Task<Message> {
    state.problem = None;
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(match &asked {
            Asked::Camera(app, allowed) => privacy::set_camera(app, *allowed),
            Asked::Recent(on) => privacy::set_recent_files(*on),
            Asked::Network(app, on) => airlock::set_network(app, *on).map(|_| ()),
        });
    });
    Task::perform(receiver, |said| {
        said.unwrap_or_else(|_| Err("It stopped before it finished.".to_string()))
    })
    .then(|said| Task::done(Message::Acted(said)).chain(read()))
}

/// What `rift-settings --set` asks of this page, the way pressing it would: `camera-app <app> on`
/// for an app that has asked for the camera, `recent-files off`, and `app-network <app> off` for an
/// app Airlock lists. Nothing for anything else.
#[must_use]
pub fn named(state: &Settings, name: &str, value: &str) -> Option<Asked> {
    let on = |word: &str| match word.trim() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    };
    let now = state.privacy.as_deref()?;
    match name {
        "recent-files" => on(value).map(Asked::Recent),
        "camera-app" => {
            let (app, word) = value.trim().rsplit_once(' ')?;
            let app = match app.trim() {
                NO_ID => "",
                app => app,
            };
            let known = now
                .camera
                .as_ref()
                .is_ok_and(|answers| answers.iter().any(|answer| answer.app == app));
            known.then_some(Asked::Camera(app.to_string(), on(word)?))
        }
        "app-network" => {
            let (app, word) = value.trim().rsplit_once(' ')?;
            let app = app.trim();
            let known = now
                .sandboxes
                .as_ref()
                .is_ok_and(|apps| apps.iter().any(|one| one.name == app));
            known.then_some(Asked::Network(app.to_string(), on(word)?))
        }
        _ => None,
    }
}

/// The lines `rift-settings --state` prints, once the page has read: how many apps have an answer
/// about the camera and a line each, whether recent files are remembered, whether the firewall's
/// rules are loaded, how many apps Airlock lists and a line each with its network and its kind,
/// and what fwupd says. `none` is a service that did not answer.
#[must_use]
pub fn state(state: &Settings) -> Vec<String> {
    let mut lines = Vec::new();
    let word = |on: bool| if on { "on" } else { "off" };
    if let Some(now) = state.privacy.as_deref() {
        match &now.camera {
            Ok(answers) => {
                lines.push(format!("camera-apps {}", answers.len()));
                for answer in answers {
                    let app = if answer.app.is_empty() {
                        NO_ID
                    } else {
                        answer.app.as_str()
                    };
                    lines.push(format!("camera-app {} {app}", answer.word()));
                }
            }
            Err(_) => lines.push("camera-apps none".to_string()),
        }
        lines.push(format!("recent-files {}", word(now.recent)));
        lines.push(format!(
            "firewall {}",
            now.firewall.as_ref().map_or("none", |on| word(*on))
        ));
        match &now.sandboxes {
            Ok(apps) => {
                lines.push(format!("sandboxed {}", apps.len()));
                for app in apps {
                    let kind = if app.flatpak { "flatpak" } else { "command" };
                    lines.push(format!(
                        "app-network {} {kind} {}",
                        word(app.network),
                        app.name
                    ));
                }
            }
            Err(_) => lines.push("sandboxed none".to_string()),
        }
    }
    if let Some(answered) = &state.security {
        lines.push(format!(
            "device-security {}",
            answered
                .as_ref()
                .map_or("none", |security| security.id.as_str())
        ));
    }
    lines
}

/// The page.
pub fn view(state: &Settings, look: Colors) -> Element<'_, Message> {
    let Some(now) = state.privacy.as_deref() else {
        return note(look, "Reading the privacy settings.");
    };
    let recent = vec![setting(
        look,
        "Remember recent files",
        Some("Apps list the files opened lately."),
        switch(look, now.recent, |on| Message::Private(Asked::Recent(on))),
    )];
    let mut history = column![heading(look, "File history"), group(look, recent)].spacing(8);
    // the list of files opened lately is under home and the camera's answers are in the portal's
    // permission store, which is under home too: both are memory in a Ghost boot, so a switch here
    // holds for this login and is on no drive afterwards (ADR-0084)
    if ghost::on() {
        history = history.push(ghost::only_now(look, FILES_FOR_THIS_SESSION));
    }
    let mut page = column![
        the_camera(state, look, now),
        part(look, "Microphone", MICROPHONE),
        part(look, "Location", LOCATION),
        part(look, "Screen lock", LOCKING),
        history,
        the_network(state, look, now),
        the_firmware(state, look),
        part(look, "Diagnostics", DIAGNOSTICS),
    ]
    .spacing(GAP)
    .width(Fill);
    if let Some(why) = &state.problem {
        page = page.push(text(why).size(TEXT_SIZE).color(look.error));
    }
    page.into()
}

/// A heading and one sentence, for something there is nothing to set for.
fn part<'a>(look: Colors, title: &'a str, said: &'a str) -> Element<'a, Message> {
    column![heading(look, title), note(look, said)]
        .spacing(8)
        .into()
}

/// A line inside a group, for a list with nothing in it or a service that did not answer.
fn inside<'a>(look: Colors, said: &str) -> Element<'a, Message> {
    container(
        text(said.to_string())
            .size(TEXT_SIZE)
            .color(look.dim)
            .width(Fill),
    )
    .padding([8, 12])
    .into()
}

/// Each app that has asked for the camera, with a switch that allows it or refuses it.
fn the_camera<'a>(state: &'a Settings, look: Colors, now: &'a Picture) -> Element<'a, Message> {
    let rows = match &now.camera {
        Err(why) => vec![inside(look, why)],
        Ok(answers) if answers.is_empty() => {
            vec![inside(look, "No app has asked for the camera yet.")]
        }
        Ok(answers) => answers
            .iter()
            .map(|answer| camera_row(state, look, answer))
            .collect(),
    };
    let mut section = column![
        heading(look, "Camera"),
        group(look, rows),
        note(look, CAMERA)
    ]
    .spacing(8);
    if ghost::on() {
        section = section.push(ghost::only_now(look, CAMERA_FOR_THIS_SESSION));
    }
    section.into()
}

/// One app's answer: its icon and name from its desktop entry, and the switch.
fn camera_row<'a>(state: &'a Settings, look: Colors, answer: &'a Answer) -> Element<'a, Message> {
    let entry = state.apps.iter().find(|app| app.id == answer.app);
    let name = match entry {
        Some(app) => app.name.as_str(),
        None if answer.app.is_empty() => "Other programs",
        None => answer.app.as_str(),
    };
    let mut words = column![line(look, name)].spacing(2);
    if answer.app.is_empty() {
        words = words.push(note(look, "Ones started from a terminal"));
    } else if answer.allowed.is_none() {
        words = words.push(note(look, "Asked again next time"));
    }
    let app = answer.app.clone();
    container(
        row![
            icons::of_app(look.text, entry.and_then(|app| app.icon.as_deref()), ICON),
            container(words).width(Fill),
            switch(look, answer.allowed == Some(true), move |on| {
                Message::Private(Asked::Camera(app.clone(), on))
            }),
        ]
        .spacing(12)
        .align_y(Center),
    )
    .width(Fill)
    .padding([8, 12])
    .into()
}

/// The firewall, and a switch for the network of each app on it.
fn the_network<'a>(state: &'a Settings, look: Colors, now: &'a Picture) -> Element<'a, Message> {
    let wall = match &now.firewall {
        Ok(true) => "On",
        Ok(false) => "Off",
        Err(_) => "Not known",
    };
    let mut rows = vec![setting(look, "Firewall", Some(FIREWALL), said(look, wall))];
    match &now.sandboxes {
        Err(why) => rows.push(inside(look, why)),
        Ok(apps) if apps.is_empty() => {
            rows.push(inside(
                look,
                "No app is on the switch, and every app has the network.",
            ));
        }
        Ok(apps) => rows.extend(apps.iter().map(|app| sandbox_row(state, look, app))),
    }
    column![
        heading(look, "Network access"),
        group(look, rows),
        note(look, SANDBOXES)
    ]
    .spacing(8)
    .into()
}

/// One app Airlock lists: the name it is known by, how many of its sandboxes run, and its network
/// switch. Airlock knows a Flatpak app by its id, so the name here is its desktop entry's when the
/// drive has one.
fn sandbox_row<'a>(
    state: &'a Settings,
    look: Colors,
    app: &'a airlock::App,
) -> Element<'a, Message> {
    let entry = app
        .flatpak
        .then(|| state.apps.iter().find(|one| one.id == app.name))
        .flatten();
    let name = entry.map_or(app.name.as_str(), |one| one.name.as_str());
    let running = match (app.flatpak, app.running) {
        (true, 0) => "Flatpak app".to_string(),
        (true, 1) => "Flatpak app, running".to_string(),
        (true, count) => format!("Flatpak app, {count} running"),
        (false, 0) => "Not running".to_string(),
        (false, 1) => "Running in a sandbox".to_string(),
        (false, count) => format!("Running in {count} sandboxes"),
    };
    let asked = app.name.clone();
    // no icon: the firewall's row in the same group has none, and a program in a sandbox has no
    // entry to take one from
    container(
        row![
            column![
                line(look, name),
                text(running).size(TEXT_SIZE).color(look.dim),
            ]
            .spacing(2)
            .width(Fill),
            switch(look, app.network, move |on| {
                Message::Private(Asked::Network(asked.clone(), on))
            }),
        ]
        .spacing(12)
        .align_y(Center),
    )
    .width(Fill)
    .padding([8, 12])
    .into()
}

/// What fwupd says about the firmware.
fn the_firmware(state: &Settings, look: Colors) -> Element<'_, Message> {
    let row = match &state.security {
        None => inside(look, "Asking fwupd."),
        Some(Err(why)) => inside(look, why),
        Some(Ok(security)) => level_row(look, security),
    };
    column![
        heading(look, "Device security"),
        group(look, vec![row]),
        note(look, FIRMWARE)
    ]
    .spacing(8)
    .into()
}

/// The security level in words, with fwupd's own id under it, and what a level it did not measure
/// or a problem it found means.
fn level_row<'a>(look: Colors, security: &Security) -> Element<'a, Message> {
    let mut words = column![
        line(look, "Security level"),
        text(security.id.clone()).size(TEXT_SIZE).color(look.dim),
    ]
    .spacing(2)
    .width(Fill);
    if security.level.is_none() {
        words = words.push(note(look, UNMEASURED));
    } else if security.issues {
        words = words.push(note(look, ISSUES));
    }
    container(
        row![words, said(look, security.words())]
            .spacing(GAP)
            .align_y(Center),
    )
    .width(Fill)
    .padding([8, 12])
    .into()
}

/// Under the camera's apps.
const CAMERA: &str = "An app asks before it takes the camera, and the answer is kept here. Off, \
                      the app is refused without being asked.";
/// What an answer given in a Ghost boot is, for the camera and for the file history.
const CAMERA_FOR_THIS_SESSION: &str =
    "What apps were told before is on the drive, and an app can be answered for this session";
const FILES_FOR_THIS_SESSION: &str = "This switch is this session's own";

/// What the microphone does.
const MICROPHONE: &str = "An app reaches the microphone without asking: only a sandbox keeps it \
                          away. The Sound page mutes the microphone for every app.";
/// What location does.
const LOCATION: &str = "No location service runs on this drive, so the system has no location to \
                        give an app.";
/// What locks the screen.
const LOCKING: &str = "Mod+L and Lock in the system menu lock the screen. Nothing locks it when \
                       the machine is left alone, and the lock screen shows no notifications.";
/// Under the firewall's row.
const FIREWALL: &str = "Nothing on the network can open a connection to this machine. Ping and \
                        the answers of printers it looks for come through.";
/// Under the network group.
const SANDBOXES: &str = "Every Flatpak app is here as soon as it is installed, and so is every \
                         program rift run --sandbox starts. Its switch takes the network away from \
                         it, in what runs now and every time it starts again, as rift net off \
                         does.";
/// Under the security level.
const FIRMWARE: &str = "fwupd checks how the firmware protects the machine: secure boot, the TPM \
                        and more. fwupdmgr security lists every check.";
/// What a level fwupd did not measure means.
const UNMEASURED: &str = "fwupd measures desktops, laptops and servers, and this machine does not \
                          say it is one.";
/// What the mark after a level means.
const ISSUES: &str = "fwupd found a problem in the running system as well.";
/// What Rift sends.
const DIAGNOSTICS: &str = "Rift sends nothing about this machine anywhere. There are no crash \
                           reports and no usage counts to turn off.";

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        let mut state = Settings::bare();
        state.privacy = Some(Box::new(Picture {
            camera: Ok(vec![
                Answer {
                    app: String::new(),
                    allowed: Some(false),
                },
                Answer {
                    app: "org.gnome.Snapshot".to_string(),
                    allowed: Some(true),
                },
            ]),
            recent: true,
            firewall: Ok(true),
            sandboxes: Ok(vec![
                airlock::App {
                    name: "dev.rift.TestApp".to_string(),
                    network: true,
                    running: 1,
                    flatpak: true,
                },
                airlock::App {
                    name: "fetcher".to_string(),
                    network: false,
                    running: 0,
                    flatpak: false,
                },
            ]),
        }));
        state
    }

    #[test]
    fn the_state_says_every_answer_and_every_switch() {
        assert!(state(&Settings::bare()).is_empty());
        let mut kept = settings();
        kept.security = Some(Ok(Security::from_id("HSI:1 (v2.1.6)")));
        assert_eq!(
            state(&kept),
            [
                "camera-apps 2",
                "camera-app no -",
                "camera-app yes org.gnome.Snapshot",
                "recent-files on",
                "firewall on",
                "sandboxed 2",
                "app-network on flatpak dev.rift.TestApp",
                "app-network off command fetcher",
                "device-security HSI:1 (v2.1.6)",
            ]
        );
        kept.security = Some(Err("fwupd is not running.".to_string()));
        assert_eq!(
            state(&kept).last().map(String::as_str),
            Some("device-security none")
        );
    }

    #[test]
    fn a_setting_from_a_terminal_is_what_the_page_would_press() {
        let kept = settings();
        assert_eq!(
            named(&kept, "camera-app", "org.gnome.Snapshot off"),
            Some(Asked::Camera("org.gnome.Snapshot".into(), false))
        );
        assert_eq!(
            named(&kept, "camera-app", "- on"),
            Some(Asked::Camera(String::new(), true))
        );
        assert_eq!(named(&kept, "camera-app", "org.gnome.Loupe off"), None);
        assert_eq!(named(&kept, "camera-app", "org.gnome.Snapshot"), None);
        assert_eq!(
            named(&kept, "recent-files", "off"),
            Some(Asked::Recent(false))
        );
        assert_eq!(named(&kept, "recent-files", "later"), None);
        assert_eq!(
            named(&kept, "app-network", "fetcher on"),
            Some(Asked::Network("fetcher".into(), true))
        );
        assert_eq!(named(&kept, "app-network", "curl on"), None);
        assert_eq!(named(&Settings::bare(), "recent-files", "off"), None);
    }

    #[test]
    fn the_sentences_are_sentences() {
        for sentence in [
            CAMERA,
            MICROPHONE,
            LOCATION,
            LOCKING,
            FIREWALL,
            SANDBOXES,
            FIRMWARE,
            UNMEASURED,
            ISSUES,
            DIAGNOSTICS,
        ] {
            assert!(sentence.ends_with('.'), "{sentence}");
            assert!(sentence.is_ascii(), "{sentence}");
        }
    }
}
