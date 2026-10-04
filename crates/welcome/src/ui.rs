//! The window: a header bar along the top, then either the start page by itself or a sidebar of
//! the steps with one page beside it and a row of buttons under it, the way an installer or a first
//! run assistant is laid out. Drawn with iced in software, in the colours the owner has chosen,
//! which the Appearance page changes as it goes.
//!
//! The app is a daemon: the window can close while apps are still installing, the app keeps going
//! until they are done, and `rift-welcome` opens the window again meanwhile.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::widget::{button, column, container, row, space, text};
use iced::{
    Border, Center, Color, Element, Fill, Length, Size, Subscription, Task, Theme, theme, window,
};
use librift::appearance::{Accent, Look, Theme as Mode};
use librift::suggested::App;
use librift::wallpaper::Choice;
use librift::{bus, network};

use crate::control::{self, Command};
use crate::install::{self, Doing, Install, Step};
use crate::page::Page;
use crate::theme::{Colors, colors};
use crate::widgets::{BOLD, FONT, PAD, TEXT_SIZE, action, fill, hairline, primary, scroll};
use crate::{appearance, apps, developer, done, note, start};

/// What the window calls itself: the name of its desktop entry, which the dock, the compositor and
/// the boot test all know it by.
const APP_ID: &str = "dev.rift.Welcome";

/// How wide the sidebar is.
const SIDEBAR: f32 = 208.0;
/// How tall the header bar is. It is the title bar of the window, which the app draws itself.
const HEADER: f32 = 45.0;
/// How tall a row of the sidebar is.
const ROW: f32 = 34.0;
/// How tall the row of buttons under a step is.
const FOOT: f32 = 56.0;
/// How wide the close button is, so the title of the start page is centred over the whole window.
const CLOSE: f32 = 30.0;

/// What the app is started with.
#[derive(Debug, Default)]
pub struct Start {
    /// The page to open on, when one was named.
    pub page: Option<Page>,
    /// Where to save a picture of the window once it has drawn, and then quit.
    pub screenshot: Option<PathBuf>,
}

/// The app's state.
pub struct Welcome {
    /// The page that is up, or that comes up when the window opens again.
    pub page: Page,
    /// The window, while it is open.
    pub window: Option<window::Id>,
    /// Dark or light, the accent and the wallpaper, and the rest of what Settings writes.
    pub look: Look,
    /// The wallpapers to choose from.
    pub choices: Vec<Choice>,
    /// Whether the machine is on a network with a way off it, as `NetworkManager` says.
    pub online: bool,
    /// The apps Rift suggests, out of the list the image keeps.
    pub apps: Result<Vec<App>, String>,
    /// What flatpak says about them, once it has answered.
    pub catalog: Option<Box<apps::Catalog>>,
    /// Whether flatpak is being asked now.
    pub asking: bool,
    /// The apps ticked on the Apps page, by id.
    pub ticked: Vec<String>,
    /// Every install asked for, in order.
    pub installs: Vec<Install>,
    /// The queue the installs wait in.
    queue: install::Shared,
    /// The command the Developer page copied last, by its place in the list.
    pub copied: Option<usize>,
    /// The last thing that could not be done.
    pub problem: Option<String>,
    /// Where `--screenshot` saves a picture of the window.
    screenshot: Option<PathBuf>,
}

/// What a press or a line on the socket asks for, and what the machine answers.
#[derive(Debug, Clone)]
pub enum Message {
    /// Show this page.
    Show(Page),
    /// The button that goes on to the next step.
    Next,
    /// The button that goes back a step.
    Back,
    /// Dark or light.
    Mode(Mode),
    /// One of the nine accents.
    Accent(Accent),
    /// The wallpaper at this place in the list.
    Wallpaper(usize),
    /// Whether the machine is on a network now.
    Online(bool),
    /// What flatpak says about the apps now.
    Catalog(Box<apps::Catalog>),
    /// Ask flatpak again.
    Ask,
    /// An app on the Apps page was ticked or not.
    Tick(String, bool),
    /// The Install button.
    Install,
    /// How an install is going.
    Installing(String, Step),
    /// Copy the command at this place in the Developer page's list.
    Copy(usize),
    /// The Done button: the drive has been welcomed, and the window closes.
    Done,
    /// Open later, on the page that says there is no network: the window closes, and the next login
    /// opens it again.
    Later,
    /// The close button, which counts as Done.
    Close,
    /// The window has opened.
    Opened,
    /// The compositor asked the window to close.
    CloseRequested(window::Id),
    /// A line from the socket.
    Said(Command),
    /// The picture of the window `--screenshot` asked for.
    Shot(window::Screenshot),
}

/// Run until the window is closed and nothing is installing.
///
/// # Errors
///
/// When the window cannot be opened.
pub fn run(start: Start) -> iced::Result {
    let ran = iced::daemon(move || boot(&start), update, view)
        .title("Welcome")
        .theme(|state: &Welcome, _| {
            let look = state.colors();
            Theme::custom(
                "Rift",
                theme::Palette {
                    background: look.page,
                    text: look.text,
                    primary: look.accent,
                    success: look.accent,
                    warning: look.accent,
                    danger: look.error,
                },
            )
        })
        .subscription(subscription)
        // the window follows the interface text size the way a GTK app does, since it is not one
        .scale_factor(|state: &Welcome, _| {
            f32::from(u16::try_from(state.look.text).unwrap_or(100)) / 100.0
        })
        .default_font(FONT)
        .settings(iced::Settings {
            id: Some(APP_ID.to_string()),
            default_font: FONT,
            default_text_size: TEXT_SIZE.into(),
            ..iced::Settings::default()
        })
        .run();
    control::close();
    ran
}

/// The window itself. On Wayland the app id comes from the platform settings and nowhere else, and
/// it is the name of the desktop entry, which is how the dock and the compositor know the window.
/// A window that is only there to have its picture taken is as tall as a whole page.
fn window(screenshot: bool) -> window::Settings {
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut settings = window::Settings {
        size: Size::new(880.0, if screenshot { 1200.0 } else { 640.0 }),
        min_size: Some(Size::new(640.0, 480.0)),
        exit_on_close_request: false,
        // the app draws its own title bar, the way the GTK apps of the session do
        decorations: false,
        ..window::Settings::default()
    };
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = APP_ID.to_string();
    }
    settings
}

fn boot(start: &Start) -> (Welcome, Task<Message>) {
    // the network decides the first page, so it is asked before the window opens, the way the
    // rest of the session would see it. no NetworkManager to ask says nothing either way
    let online = start.page != Some(Page::Offline) && on_network(network::read());
    let mut state = Welcome {
        page: start
            .page
            .unwrap_or(if online { Page::Start } else { Page::Offline }),
        online,
        look: Look::read(),
        choices: librift::wallpaper::choices(),
        apps: librift::suggested::read(),
        screenshot: start.screenshot.clone(),
        ..Welcome::bare()
    };
    let mut work = vec![open(&mut state)];
    if state.page == Page::Apps {
        work.push(apps::ask(&mut state));
    }
    if start.screenshot.is_some() {
        work.push(shoot());
    }
    keep(&state);
    (state, Task::batch(work))
}

/// Whether what `NetworkManager` says is a network with a way off it: a cable or a wireless
/// network that is up and has a gateway, which is what an app needs to reach Flathub. When
/// `NetworkManager` does not answer nothing is known, and nothing is held back.
#[must_use]
pub fn on_network(said: Result<network::Picture, String>) -> bool {
    let Ok(picture) = said else {
        return true;
    };
    let wired = picture
        .wired
        .as_ref()
        .map(|wired| (wired.link, wired.addresses.gateway.is_some()));
    let wireless = picture
        .wireless
        .as_ref()
        .map(|wireless| (wireless.link, wireless.addresses.gateway.is_some()));
    [wired, wireless]
        .into_iter()
        .flatten()
        .any(|(link, gateway)| link == network::Link::Connected && gateway)
}

/// Open the window, or bring the one that is open to the front.
fn open(state: &mut Welcome) -> Task<Message> {
    if let Some(id) = state.window {
        return window::gain_focus(id);
    }
    let (id, opened) = window::open(window(state.screenshot.is_some()));
    state.window = Some(id);
    opened.map(|_| Message::Opened)
}

/// For `--screenshot`: wait a moment for the window to draw itself, then take the picture.
fn shoot() -> Task<Message> {
    Task::perform(
        async { std::thread::sleep(Duration::from_millis(800)) },
        |()| (),
    )
    .then(|()| window::oldest())
    .and_then(window::screenshot)
    .map(Message::Shot)
}

/// Write a picture of the window as a png.
fn save(path: &Path, shot: &window::Screenshot) -> Result<(), String> {
    let (wide, tall) = (shot.size.width, shot.size.height);
    image::RgbaImage::from_raw(wide, tall, shot.rgba.to_vec())
        .ok_or_else(|| format!("the picture is not {wide} by {tall}"))?
        .save(path)
        .map_err(|e| format!("Could not write {}: {e}", path.display()))
}

impl Welcome {
    /// A window that has asked for nothing yet, which the pages' own tests build on.
    #[must_use]
    pub fn bare() -> Self {
        Self {
            page: Page::Start,
            window: None,
            look: Look::default(),
            choices: Vec::new(),
            online: true,
            apps: Ok(Vec::new()),
            catalog: None,
            asking: false,
            ticked: Vec::new(),
            installs: Vec::new(),
            queue: Arc::default(),
            copied: None,
            problem: None,
            screenshot: None,
        }
    }

    /// The colours the window is drawn in.
    pub fn colors(&self) -> Colors {
        colors(self.look.theme, self.look.accent)
    }

    /// The queue the installs wait in.
    pub fn queue(&self) -> &install::Shared {
        &self.queue
    }

    /// Whether an install is still to finish.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.installs.iter().any(|install| install.doing.pending())
    }

    /// The lines `rift-welcome --state` prints.
    fn state(&self) -> String {
        let look = &self.look;
        [
            format!("page {}", self.page.word()),
            format!("network {}", if self.online { "online" } else { "offline" }),
            format!("welcomed {}", if note::welcomed() { "yes" } else { "no" }),
            format!("ghost {}", if librift::ghost::on() { "on" } else { "off" }),
            format!(
                "window {}",
                if self.window.is_some() {
                    "open"
                } else {
                    "closed"
                }
            ),
            format!("theme {}", look.theme.word()),
            format!("accent {}", look.accent.word()),
            format!("wallpaper {}", look.wallpaper),
        ]
        .into_iter()
        .chain(apps::state(self))
        .chain(
            self.installs
                .iter()
                .map(|install| format!("install {} {}", install.id, install.doing.word())),
        )
        .chain(self.problem.as_ref().map(|why| format!("problem {why}")))
        .collect::<Vec<_>>()
        .join("\n")
            + "\n"
    }
}

/// Handle a message, and keep what `--state` prints up to date: the socket answers from it on a
/// thread of its own, and there may be no window to draw it.
fn update(state: &mut Welcome, message: Message) -> Task<Message> {
    let task = handle(state, message);
    keep(state);
    task
}

fn handle(state: &mut Welcome, message: Message) -> Task<Message> {
    match message {
        Message::Show(page) => return show(state, page),
        Message::Next => {
            if let Some(next) = state.page.next() {
                return show(state, next);
            }
        }
        Message::Back => {
            if let Some(back) = state.page.back() {
                return show(state, back);
            }
        }
        Message::Mode(mode) => {
            state.look.theme = mode;
            appearance::wrote(state);
        }
        Message::Accent(accent) => {
            state.look.accent = accent;
            appearance::wrote(state);
        }
        Message::Wallpaper(at) => {
            if let Some(choice) = state.choices.get(at) {
                state.look.wallpaper = choice.wallpaper.clone();
                appearance::wrote(state);
            }
        }
        Message::Online(online) => return network_changed(state, online),
        Message::Catalog(catalog) => {
            state.asking = false;
            state.catalog = Some(catalog);
        }
        Message::Ask => return apps::ask(state),
        Message::Tick(id, on) => apps::tick(state, &id, on),
        Message::Install => return apps::install(state),
        Message::Installing(id, step) => return installing(state, &id, step),
        Message::Copy(at) => return developer::copy(state, at),
        Message::Done | Message::Close => {
            state.problem = note::keep().err();
            return close(state);
        }
        Message::Later => return close(state),
        Message::CloseRequested(id) => {
            if state.window == Some(id) {
                state.problem = note::keep().err();
                return close(state);
            }
        }
        Message::Said(command) => return said(state, command),
        Message::Shot(shot) => {
            if let Some(path) = &state.screenshot
                && let Err(why) = save(path, &shot)
            {
                eprintln!("rift-welcome: {why}");
            }
            return iced::exit();
        }
        Message::Opened => {}
    }
    Task::none()
}

/// Show a page. The start page and the page that says there is no network are one place, and
/// which of the two it is follows the network.
fn show(state: &mut Welcome, page: Page) -> Task<Message> {
    let page = match (page, state.online) {
        (Page::Start, false) => Page::Offline,
        (Page::Offline, true) => Page::Start,
        _ => page,
    };
    if state.page == page {
        return Task::none();
    }
    state.page = page;
    state.problem = None;
    state.copied = None;
    if page == Page::Apps {
        return apps::ask_once(state);
    }
    Task::none()
}

/// The network came or went. The start page says whether there is one, and the Apps page asks
/// Flathub again once there is.
fn network_changed(state: &mut Welcome, online: bool) -> Task<Message> {
    let was = state.online;
    state.online = online;
    match (state.page, online) {
        (Page::Offline, true) => state.page = Page::Start,
        (Page::Start, false) => state.page = Page::Offline,
        _ => {}
    }
    if online && !was && state.page == Page::Apps {
        return apps::ask(state);
    }
    Task::none()
}

/// How an install is going, from the worker.
fn installing(state: &mut Welcome, id: &str, step: Step) -> Task<Message> {
    let windowless = state.window.is_none();
    let Some(install) = state
        .installs
        .iter_mut()
        .find(|install| install.id == id && install.doing.pending())
    else {
        return Task::none();
    };
    match step {
        Step::Started => install.doing = Doing::Running(0),
        Step::Moved(percent) => install.doing = Doing::Running(percent),
        Step::Finished(done) => {
            if windowless {
                install::tell(&install.name, &done);
            }
            install.doing = match &done {
                Ok(()) => Doing::Installed,
                Err(why) => Doing::Failed(why.clone()),
            };
            if done.is_ok() {
                apps::installed(state, id);
            }
            if windowless && !state.busy() {
                return iced::exit();
            }
        }
    }
    Task::none()
}

/// Close the window. The app goes on while apps are installing, and ends when nothing is.
fn close(state: &mut Welcome) -> Task<Message> {
    if !state.busy() {
        return iced::exit();
    }
    state.window.take().map_or_else(Task::none, window::close)
}

/// A line from the socket. Setting something over it does what pressing it on the page does.
fn said(state: &mut Welcome, command: Command) -> Task<Message> {
    match command {
        Command::Open => {
            // a window opened again shows what is installing, or the start page
            if state.window.is_none() {
                state.page = if state.busy() {
                    Page::Apps
                } else if state.online {
                    Page::Start
                } else {
                    Page::Offline
                };
            }
            return open(state);
        }
        Command::Page(word) => {
            if let Some(page) = Page::from_word(&word) {
                let opening = open(state);
                return Task::batch([opening, show(state, page)]);
            }
        }
        Command::Set(name, value) => return set(state, &name, &value),
        // answered on the socket's own thread
        Command::State => {}
    }
    Task::none()
}

fn set(state: &Welcome, name: &str, value: &str) -> Task<Message> {
    let value = value.trim();
    let message = match name {
        "theme" => Message::Mode(Mode::from_setting(value)),
        "accent" => Message::Accent(Accent::from_setting(value)),
        "wallpaper" => match state.choices.iter().position(|choice| choice.names(value)) {
            Some(at) => Message::Wallpaper(at),
            None => return Task::none(),
        },
        "tick" => Message::Tick(value.to_string(), true),
        "untick" => Message::Tick(value.to_string(), false),
        "copy" => match developer::named(value) {
            Some(at) => Message::Copy(at),
            None => return Task::none(),
        },
        // the value is there to be typed, the way a switch takes on or off: there is one thing to do
        "next" => Message::Next,
        "back" => Message::Back,
        "install" => Message::Install,
        "ask" => Message::Ask,
        "done" => Message::Done,
        "later" => Message::Later,
        "close" => Message::Close,
        _ => return Task::none(),
    };
    Task::done(message)
}

fn subscription(state: &Welcome) -> Subscription<Message> {
    let mut followed = vec![window::close_requests().map(Message::CloseRequested)];
    // a window that is only there to have its picture taken answers nothing and follows nothing,
    // so a running Welcome keeps its socket and the page stays the one asked for
    if state.screenshot.is_none() {
        followed.push(terminal());
        followed.push(following_network());
    }
    Subscription::batch(followed)
}

/// Whether the machine is on a network, now and after every change `NetworkManager` announces.
/// The subscription is named, because iced tells two apart by the type of the stream and the
/// address of the function that makes it.
fn following_network() -> Subscription<Message> {
    Subscription::run_with("network", |_| {
        let (sender, receiver) = mpsc::unbounded();
        bus::follow(
            |poke| {
                bus::listen(
                    poke,
                    |each| bus::signals(network::SERVICE, each),
                    |why| eprintln!("rift-welcome: {why}"),
                );
            },
            || Message::Online(on_network(network::read())),
            move |message| sender.unbounded_send(message).is_ok(),
        );
        receiver
    })
}

/// The socket in the runtime directory, read on a thread of its own. The state query is answered
/// there, from the lines the app keeps up to date.
fn terminal() -> Subscription<Message> {
    Subscription::run_with("terminal", |_| {
        let (sender, receiver) = mpsc::unbounded();
        thread::spawn(move || {
            if let Err(why) = control::serve(|command| {
                if command == Command::State {
                    return Some(kept());
                }
                let _ = sender.unbounded_send(Message::Said(command));
                None
            }) {
                eprintln!("rift-welcome: {why}");
            }
        });
        receiver
    })
}

/// What the socket answers a state query with.
fn kept() -> String {
    STATE.lock().map_or_else(
        |_| "the window is busy\n".to_string(),
        |state| state.clone(),
    )
}

/// Keep what `--state` prints.
fn keep(state: &Welcome) {
    if let Ok(mut kept) = STATE.lock() {
        *kept = state.state();
    }
}

static STATE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

fn view(state: &Welcome, _: window::Id) -> Element<'_, Message> {
    let look = state.colors();
    let body: Element<'_, Message> = if state.page.is_step() {
        row![
            sidebar(state, look),
            column![
                container(page(state, look))
                    .width(Fill)
                    .height(Fill)
                    .style(move |_: &Theme| fill(look.page)),
                foot(state, look),
            ],
        ]
        .height(Fill)
        .into()
    } else {
        container(scroll(look, start::view(state, look)))
            .width(Fill)
            .height(Fill)
            .center_y(Fill)
            .style(move |_: &Theme| fill(look.page))
            .into()
    };
    column![header(state, look), body].into()
}

/// The title bar the app draws for itself: the name of the app over the sidebar, the name of the
/// page over the page, and the close button at the right end. The start page has no sidebar, and
/// its title is centred over the whole window.
fn header(state: &Welcome, look: Colors) -> Element<'_, Message> {
    let title = container(
        text(state.page.label())
            .size(TEXT_SIZE)
            .font(BOLD)
            .color(look.text),
    )
    .width(Fill)
    .center_x(Fill)
    .center_y(Fill);
    let left: Element<'_, Message> = if state.page.is_step() {
        container(text("Welcome").size(TEXT_SIZE).font(BOLD).color(look.text))
            .width(Length::Fixed(SIDEBAR))
            .padding([0, 14])
            .center_y(Fill)
            .into()
    } else {
        space().width(Length::Fixed(CLOSE)).into()
    };
    let close = button(crate::icons::symbolic(
        look.text,
        "window-close-symbolic",
        16.0,
    ))
    .padding(7)
    .on_press(Message::Close)
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
    column![
        container(row![left, title, close].align_y(Center).padding([0, 8]))
            .width(Fill)
            .height(Length::Fixed(HEADER))
            .style(move |_: &Theme| fill(look.header)),
        line,
    ]
    .into()
}

/// The steps, one row each, the one that is up in the accent.
fn sidebar(state: &Welcome, look: Colors) -> Element<'_, Message> {
    let mut rows = column![].width(Fill).spacing(2).padding([8, 8]);
    for page in Page::STEPS {
        let here = page == state.page;
        let colour = if here { look.on_accent } else { look.text };
        // a button lays its content out at the top of its box, so the row is centred by hand
        rows = rows.push(
            button(
                container(
                    row![
                        crate::icons::symbolic(colour, page.icon(), 16.0),
                        text(page.label()).size(TEXT_SIZE).color(colour),
                    ]
                    .align_y(Center)
                    .spacing(10),
                )
                .center_y(Fill),
            )
            .width(Fill)
            .height(Length::Fixed(ROW))
            .padding([0, 10])
            .on_press(Message::Show(page))
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
            }),
        );
    }
    row![
        container(rows)
            .width(Length::Fixed(SIDEBAR))
            .height(Fill)
            .style(move |_: &Theme| fill(look.side)),
        container(space().width(1.0).height(Fill)).style(move |_: &Theme| fill(look.line)),
    ]
    .into()
}

/// The step that is up.
fn page(state: &Welcome, look: Colors) -> Element<'_, Message> {
    let inside = match state.page {
        Page::Appearance => appearance::view(state, look),
        Page::Apps => apps::view(state, look),
        Page::Developer => developer::view(state, look),
        Page::Done | Page::Start | Page::Offline => done::view(state, look),
    };
    scroll(look, container(inside).width(Fill).padding(PAD))
        .height(Fill)
        .into()
}

/// The row of buttons under a step: back at the left, and on at the right, in the accent. On the
/// Apps page Install is the one in the accent, and only while something is ticked.
fn foot(state: &Welcome, look: Colors) -> Element<'_, Message> {
    let back = action(look, "Back", Some(Message::Back));
    let on: Element<'_, Message> = match state.page {
        Page::Apps => row![
            primary(
                look,
                "Install",
                apps::can_install(state).then_some(Message::Install)
            ),
            action(look, "Next", Some(Message::Next)),
        ]
        .spacing(8)
        .into(),
        Page::Done => primary(look, "Done", Some(Message::Done)),
        _ => primary(look, "Next", Some(Message::Next)),
    };
    column![
        hairline(look),
        container(row![back, space().width(Fill), on].align_y(Center))
            .width(Fill)
            .height(Length::Fixed(FOOT))
            .padding([0.0, PAD])
            .center_y(Length::Fixed(FOOT))
            .style(move |_: &Theme| fill(look.page)),
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use librift::network::{Addresses, Link, Picture, Wired};

    fn cable(link: Link, gateway: Option<&str>) -> Picture {
        Picture {
            wifi: false,
            radio: true,
            wired: Some(Wired {
                path: "/org/freedesktop/NetworkManager/Devices/2".to_string(),
                link,
                addresses: Addresses {
                    gateway: gateway.map(str::to_string),
                    ..Addresses::default()
                },
            }),
            wireless: None,
        }
    }

    #[test]
    fn a_network_needs_a_device_that_is_up_with_a_gateway() {
        assert!(on_network(Ok(cable(Link::Connected, Some("10.0.2.2")))));
        assert!(!on_network(Ok(cable(Link::Connected, None))));
        assert!(!on_network(Ok(cable(Link::Connecting, Some("10.0.2.2")))));
        assert!(!on_network(Ok(Picture {
            wifi: false,
            radio: true,
            wired: None,
            wireless: None,
        })));
        // no NetworkManager to ask holds nothing back
        assert!(on_network(
            Err("NetworkManager is not running.".to_string())
        ));
    }

    #[test]
    fn the_start_page_follows_the_network() {
        let mut state = Welcome::bare();
        let _ = network_changed(&mut state, false);
        assert_eq!(state.page, Page::Offline);
        let _ = network_changed(&mut state, true);
        assert_eq!(state.page, Page::Start);
        // a step stays where it is
        state.page = Page::Developer;
        let _ = network_changed(&mut state, false);
        assert_eq!(state.page, Page::Developer);
        // and going back from the first step finds the page that says so
        state.page = Page::Appearance;
        let _ = show(&mut state, Page::Start);
        assert_eq!(state.page, Page::Offline);
    }
}
