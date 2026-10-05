//! The window: a header bar with the field in it, a sidebar of two rows, and one page beside it,
//! the way a store is laid out. Drawn with iced in software, in the colours the owner has chosen.
//!
//! The app is a daemon: the window can close while an app is still installing, the app keeps going
//! until the queue is empty, and `rift-store` opens the window again meanwhile.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use iced::widget::{button, column, container, row, space, text};
use iced::{
    Border, Center, Color, Element, Fill, Length, Size, Subscription, Task, Theme, theme, window,
};
use librift::appearance::Look;

use crate::app::{self, Shown};
use crate::catalog::{self, Catalog};
use crate::control::{self, Command};
use crate::jobs::{self, Doing, Step, Work};
use crate::page::Page;
use crate::theme::{Colors, colors};
use crate::widgets::{BOLD, FONT, PAD, TEXT_SIZE, field, fill, scroll, tool};
use crate::{found, front, installed};

/// What the window calls itself: the name of its desktop entry, which the dock, the compositor and
/// the boot test all know it by.
const APP_ID: &str = "dev.rift.Store";

/// How wide the sidebar is.
const SIDEBAR: f32 = 176.0;
/// How tall the header bar is. It is the title bar of the window, which the app draws itself.
const HEADER: f32 = 45.0;
/// How tall a row of the sidebar is.
const ROW: f32 = 34.0;
/// The field in the header bar, so a search can be typed into it from the socket as well.
const FIELD: &str = "store-field";

/// What the app is started with.
#[derive(Debug, Default)]
pub struct Start {
    /// The page to open on, when one was named.
    pub page: Option<Page>,
    /// The app to open, when one was named.
    pub app: Option<String>,
    /// The words to search for, when some were given.
    pub words: Option<String>,
    /// Where to save a picture of the window once it has drawn, and then quit.
    pub screenshot: Option<PathBuf>,
    /// Whether to draw the pages with what a remote said once rather than ask flatpak, which is
    /// how a picture of them is taken on a machine that is not Rift. Debug builds only.
    #[cfg(debug_assertions)]
    pub pretend: bool,
}

/// The app's state.
pub struct Store {
    /// The page that is up, or that comes up when the window opens again.
    pub page: Page,
    /// The window, while it is open.
    pub window: Option<window::Id>,
    /// Dark or light, the accent, and the rest of what Settings writes.
    pub look: Look,
    /// The apps Rift suggests, out of the list the image keeps.
    pub apps: Result<Vec<librift::suggested::App>, String>,
    /// What flatpak says about them, once it has answered.
    pub catalog: Option<Box<Catalog>>,
    /// Whether flatpak is being asked now.
    pub asking: bool,
    /// What is in the field.
    pub words: String,
    /// How many times the field has changed, so an answer to words that have changed is known.
    pub typed: u64,
    /// Whether a search is running.
    pub searching: bool,
    /// What the last search found.
    pub found: Option<found::Answer>,
    /// The app whose own page is up.
    pub shown: Option<Shown>,
    /// Every install and remove asked for, in order.
    pub work: Vec<Work>,
    /// The queue they wait in.
    queue: jobs::Shared,
    /// The last thing that could not be done.
    pub problem: Option<String>,
    /// Where `--screenshot` saves a picture of the window.
    screenshot: Option<PathBuf>,
    /// Whether the pages are drawn with what a remote said once rather than what flatpak says now.
    #[cfg(debug_assertions)]
    pub pretend: bool,
}

/// What a press or a line on the socket asks for, and what flatpak answers.
#[derive(Debug, Clone)]
pub enum Message {
    /// Show this page.
    Show(Page),
    /// Something was typed in the field.
    Typed(String),
    /// Search for what is in the field now.
    Find,
    /// Search for these words, which go in the field first.
    Look(String),
    /// The wait after a letter is over.
    Waited(u64),
    /// What a search found.
    Searched(u64, Box<found::Answer>),
    /// Open one app's own page: its id and the remote it comes from.
    Open(String, String),
    /// What a remote says about the app whose page is up.
    About(Box<app::Answer>),
    /// Back to the page the app was opened from.
    Back,
    /// Install the app whose page is up.
    Install,
    /// Take off the app whose page is up.
    Remove,
    /// Take off an app from a row of the Installed page: its id and its name.
    Take(String, String),
    /// How an install or a remove is going.
    Doing(String, Step),
    /// What flatpak says about the suggested apps now.
    Catalog(Box<Catalog>),
    /// Ask flatpak again.
    Ask,
    /// The close button.
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
        .title("Store")
        .theme(|state: &Store, _| {
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
        .scale_factor(|state: &Store, _| {
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
        size: Size::new(880.0, if screenshot { 1100.0 } else { 660.0 }),
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

fn boot(start: &Start) -> (Store, Task<Message>) {
    let mut state = Store {
        page: start.page.unwrap_or(Page::Apps),
        look: Look::read(),
        apps: librift::suggested::read(),
        screenshot: start.screenshot.clone(),
        ..Store::bare()
    };
    #[cfg(debug_assertions)]
    if start.pretend {
        crate::pretend::fill(&mut state);
    }
    let mut work = vec![open(&mut state)];
    // with an answer in hand already there is nothing to ask
    if state.catalog.is_none() {
        work.push(catalog::ask(&mut state));
    }
    if let Some(said) = &start.words {
        work.push(Task::done(Message::Look(said.clone())));
    }
    if let Some(id) = &start.app {
        work.push(Task::done(Message::Said(Command::Set(
            "open".to_string(),
            id.clone(),
        ))));
    }
    if start.screenshot.is_some() {
        work.push(shoot());
    }
    keep(&state);
    (state, Task::batch(work))
}

/// Open the window, or bring the one that is open to the front.
fn open(state: &mut Store) -> Task<Message> {
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
        async { std::thread::sleep(Duration::from_millis(1200)) },
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

impl Store {
    /// A window that has asked for nothing yet, which the pages' own tests build on.
    #[must_use]
    pub fn bare() -> Self {
        Self {
            page: Page::Apps,
            window: None,
            look: Look::default(),
            apps: Ok(Vec::new()),
            catalog: None,
            asking: false,
            words: String::new(),
            typed: 0,
            searching: false,
            found: None,
            shown: None,
            work: Vec::new(),
            queue: Arc::default(),
            problem: None,
            screenshot: None,
            #[cfg(debug_assertions)]
            pretend: false,
        }
    }

    /// The colours the window is drawn in.
    #[must_use]
    pub fn colors(&self) -> Colors {
        colors(self.look.theme, self.look.accent)
    }

    /// The queue the installs and removes wait in.
    #[must_use]
    pub fn queue(&self) -> &jobs::Shared {
        &self.queue
    }

    /// Whether something is still to finish.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.work.iter().any(|work| work.doing.pending())
    }

    /// The lines `rift-store --state` prints.
    fn state(&self) -> String {
        [
            format!("page {}", self.page.word()),
            format!(
                "window {}",
                if self.window.is_some() {
                    "open"
                } else {
                    "closed"
                }
            ),
            format!("theme {}", self.look.theme.word()),
            format!("accent {}", self.look.accent.word()),
        ]
        .into_iter()
        .chain(front::lines(self))
        .chain(found::lines(self))
        .chain(app::lines(self))
        .chain(installed::lines(self))
        .chain(
            self.work
                .iter()
                .map(|work| format!("job {} {}", work.id, work.doing.word())),
        )
        .chain(self.problem.as_ref().map(|why| format!("problem {why}")))
        .collect::<Vec<_>>()
        .join("\n")
            + "\n"
    }
}

/// Handle a message, and keep what `--state` prints up to date: the socket answers from it on a
/// thread of its own, and there may be no window to draw it.
fn update(state: &mut Store, message: Message) -> Task<Message> {
    let task = handle(state, message);
    keep(state);
    task
}

fn handle(state: &mut Store, message: Message) -> Task<Message> {
    match message {
        Message::Show(page) => return show(state, page),
        Message::Typed(words) => return found::typed(state, words),
        Message::Find => return found::find(state),
        Message::Look(words) => {
            if found::looked(state, words) {
                return found::find(state);
            }
        }
        Message::Waited(when) => return found::waited(state, when),
        Message::Searched(when, answer) => found::searched(state, when, *answer),
        Message::Open(id, remote) => return app::open(state, &id, &remote),
        Message::About(answer) => app::answered(state, *answer),
        Message::Back => {
            let back = state.shown.as_ref().map_or(Page::Apps, |shown| shown.from);
            return show(state, back);
        }
        Message::Install => return app::press(state, false),
        Message::Remove => return app::press(state, true),
        Message::Take(id, name) => return take(state, &id, name),
        Message::Doing(id, step) => return doing(state, &id, step),
        Message::Catalog(catalog) => {
            state.asking = false;
            state.catalog = Some(catalog);
        }
        Message::Ask => return catalog::ask(state),
        Message::Close => return close(state),
        Message::CloseRequested(id) => {
            if state.window == Some(id) {
                return close(state);
            }
        }
        Message::Said(command) => return said_on_socket(state, command),
        Message::Shot(shot) => {
            if let Some(path) = &state.screenshot
                && let Err(why) = save(path, &shot)
            {
                eprintln!("rift-store: {why}");
            }
            return iced::exit();
        }
        Message::Opened => {}
    }
    Task::none()
}

/// Show a page. Going to the page of one app needs an app, and there may be none.
fn show(state: &mut Store, page: Page) -> Task<Message> {
    if page == Page::App && state.shown.is_none() {
        return Task::none();
    }
    if page == Page::Found && state.words.trim().is_empty() {
        return Task::none();
    }
    if state.page == page {
        return Task::none();
    }
    state.page = page;
    state.problem = None;
    if page == Page::Apps || page == Page::Installed {
        return catalog::ask_once(state);
    }
    Task::none()
}

/// Take an app off from a row of the Installed page.
fn take(state: &mut Store, id: &str, name: String) -> Task<Message> {
    if catalog::is_busy(state, id) || !catalog::is_installed(state, id) {
        return Task::none();
    }
    state.work.push(Work {
        id: id.to_string(),
        name,
        remove: true,
        doing: Doing::Waiting,
    });
    jobs::start(
        state.queue(),
        jobs::Job {
            id: id.to_string(),
            remote: String::new(),
            remove: true,
        },
    )
}

/// How an install or a remove is going, from the worker.
fn doing(state: &mut Store, id: &str, step: Step) -> Task<Message> {
    let windowless = state.window.is_none();
    let Some(at) = state
        .work
        .iter()
        .position(|work| work.id == id && work.doing.pending())
    else {
        return Task::none();
    };
    match step {
        Step::Started => state.work[at].doing = Doing::Running(0),
        Step::Moved(percent) => state.work[at].doing = Doing::Running(percent),
        Step::Finished(done) => {
            if windowless {
                jobs::tell(&state.work[at], &done);
            }
            let removed = state.work[at].remove;
            state.work[at].doing = match &done {
                Ok(()) => Doing::Done,
                Err(why) => Doing::Failed(why.clone()),
            };
            if done.is_ok() {
                catalog::settled(state, id, removed);
            }
            if windowless && !state.busy() {
                return iced::exit();
            }
        }
    }
    Task::none()
}

/// Close the window. The app goes on while something is installing, and ends when nothing is.
fn close(state: &mut Store) -> Task<Message> {
    if !state.busy() {
        return iced::exit();
    }
    state.window.take().map_or_else(Task::none, window::close)
}

/// A line from the socket. Setting something over it does what pressing it on the page does.
fn said_on_socket(state: &mut Store, command: Command) -> Task<Message> {
    match command {
        Command::Open => return open(state),
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

fn set(state: &Store, name: &str, value: &str) -> Task<Message> {
    let value = value.trim();
    let message = match name {
        "search" => Message::Look(value.to_string()),
        "field" => Message::Typed(value.to_string()),
        "open" => Message::Open(value.to_string(), remote_of(state, value)),
        // the value is there to be typed, the way a switch takes on or off: there is one thing to do
        "install" => Message::Install,
        "remove" => Message::Remove,
        "find" => Message::Find,
        "back" => Message::Back,
        "ask" => Message::Ask,
        "close" => Message::Close,
        _ => return Task::none(),
    };
    Task::done(message)
}

/// The remote an app comes from, as far as the Store knows: the suggested list, then a row of the
/// last search, then where an installed app came from.
fn remote_of(state: &Store, id: &str) -> String {
    if let Some(app) = catalog::offered(state).into_iter().find(|app| app.id == id) {
        return app.remote.clone();
    }
    if let Some(found) = state
        .found
        .as_ref()
        .and_then(|answer| answer.as_ref().ok())
        .and_then(|found| found.iter().find(|one| one.id == id))
    {
        return found.remote.clone();
    }
    state
        .catalog
        .as_ref()
        .and_then(|catalog| catalog.installed.iter().find(|one| one.id == id))
        .map(|one| one.remote.clone())
        .unwrap_or_default()
}

fn subscription(state: &Store) -> Subscription<Message> {
    let mut followed = vec![window::close_requests().map(Message::CloseRequested)];
    // a window that is only there to have its picture taken answers nothing, so a running Store
    // keeps its socket and the page stays the one asked for
    if state.screenshot.is_none() {
        followed.push(terminal());
    }
    Subscription::batch(followed)
}

/// The socket in the runtime directory, read on a thread of its own. The state query is answered
/// there, from the lines the app keeps up to date.
fn terminal() -> Subscription<Message> {
    Subscription::run_with("terminal", |_| {
        let (sender, receiver) = iced::futures::channel::mpsc::unbounded();
        thread::spawn(move || {
            if let Err(why) = control::serve(|command| {
                if command == Command::State {
                    return Some(kept());
                }
                let _ = sender.unbounded_send(Message::Said(command));
                None
            }) {
                eprintln!("rift-store: {why}");
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
fn keep(state: &Store) {
    if let Ok(mut kept) = STATE.lock() {
        *kept = state.state();
    }
}

static STATE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// A line that says less, out of words made up as the page is drawn.
#[must_use]
pub fn said<'a>(look: Colors, words: String) -> Element<'a, Message> {
    text(words).size(TEXT_SIZE).color(look.dim).into()
}

fn view(state: &Store, _: window::Id) -> Element<'_, Message> {
    let look = state.colors();
    column![
        header(state, look),
        row![
            sidebar(state, look),
            container(page(state, look))
                .width(Fill)
                .height(Fill)
                .style(move |_: &Theme| fill(look.page)),
        ]
        .height(Fill),
    ]
    .into()
}

/// The title bar the app draws for itself: the name of the app over the sidebar, or the way back
/// when the page is not one of the sidebar's; the name of the page; the field; and the close button.
fn header(state: &Store, look: Colors) -> Element<'_, Message> {
    let left: Element<'_, Message> = if state.page.in_sidebar() {
        container(text("Store").size(TEXT_SIZE).font(BOLD).color(look.text))
            .width(Length::Fixed(SIDEBAR))
            .padding([0, 14])
            .center_y(Fill)
            .into()
    } else {
        container(tool(look, "go-previous-symbolic", Some(Message::Back)))
            .width(Length::Fixed(SIDEBAR))
            .padding([0, 8])
            .center_y(Fill)
            .into()
    };
    let title = match (state.page, state.shown.as_ref()) {
        (Page::App, Some(shown)) => shown.name.clone(),
        (page, _) => page.label().to_string(),
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
        container(
            row![
                left,
                container(text(title).size(TEXT_SIZE).font(BOLD).color(look.text))
                    .width(Fill)
                    .center_y(Fill),
                field(
                    look,
                    "Search",
                    &state.words,
                    false,
                    FIELD,
                    Message::Typed,
                    Message::Find,
                ),
                close,
            ]
            .align_y(Center)
            .spacing(10)
            .padding([0, 8])
        )
        .width(Fill)
        .height(Length::Fixed(HEADER))
        .style(move |_: &Theme| fill(look.header)),
        line,
    ]
    .into()
}

/// The two rows of the sidebar, the one that is up in the accent, with what is installing under
/// them.
fn sidebar(state: &Store, look: Colors) -> Element<'_, Message> {
    let mut rows = column![].width(Fill).spacing(2).padding([8, 8]);
    for page in Page::SIDE {
        let here = page == state.page
            || (page == Page::Apps && matches!(state.page, Page::Found))
            || (page == Page::Apps && state.page == Page::App);
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

/// The page that is up.
fn page(state: &Store, look: Colors) -> Element<'_, Message> {
    let inside = match state.page {
        Page::Apps => front::view(state, look),
        Page::Found => found::view(state, look),
        Page::App => app::view(state, look),
        Page::Installed => installed::view(state, look),
    };
    scroll(look, container(inside).width(Fill).padding(PAD))
        .height(Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::tests::{answered, listed};
    use librift::flatpak::FLATHUB;

    fn ready() -> Store {
        let mut state = Store::bare();
        state.apps = Ok(listed());
        state.catalog = Some(Box::new(answered(&[FLATHUB, "rift-test"], &[])));
        state
    }

    #[test]
    fn the_socket_opens_an_app_on_the_remote_the_store_knows_it_from() {
        let state = ready();
        assert_eq!(remote_of(&state, "org.videolan.VLC"), FLATHUB);
        assert_eq!(remote_of(&state, "dev.rift.TestEditor"), "rift-test");
        assert_eq!(remote_of(&state, "org.example.Nothing"), "");
    }

    #[test]
    fn a_page_that_needs_something_it_has_not_got_does_not_come_up() {
        let mut state = ready();
        let _ = show(&mut state, Page::App);
        assert_eq!(state.page, Page::Apps);
        let _ = show(&mut state, Page::Found);
        assert_eq!(state.page, Page::Apps);
        let _ = show(&mut state, Page::Installed);
        assert_eq!(state.page, Page::Installed);
        // and the way back from an app's page is the page it was opened from
        let _ = app::open(&mut state, "dev.rift.TestEditor", "rift-test");
        assert_eq!(state.page, Page::App);
        let back = state.shown.as_ref().unwrap().from;
        assert_eq!(back, Page::Installed);
        let _ = show(&mut state, back);
        assert_eq!(state.page, Page::Installed);
    }

    #[test]
    fn the_state_says_the_page_and_what_every_part_of_it_knows() {
        let mut state = ready();
        state.work.push(Work {
            id: "dev.rift.TestEditor".to_string(),
            name: "Rift test editor".to_string(),
            remove: false,
            doing: Doing::Running(40),
        });
        let said = state.state();
        let lines: Vec<&str> = said.lines().collect();
        assert!(lines.contains(&"page apps"));
        assert!(lines.contains(&"window closed"));
        assert!(lines.contains(&"remotes flathub,rift-test"));
        assert!(lines.contains(&"app dev.rift.TestEditor unknown"));
        assert!(lines.contains(&"words none"));
        assert!(lines.contains(&"shown none"));
        assert!(lines.contains(&"installs 0"));
        assert!(lines.contains(&"job dev.rift.TestEditor running 40"));
        assert!(state.busy());
    }

    #[test]
    fn a_remove_from_a_row_is_only_for_an_app_that_is_there() {
        let mut state = Store::bare();
        state.catalog = Some(Box::new(answered(&[FLATHUB], &["org.videolan.VLC"])));
        let _ = take(&mut state, "org.gimp.GIMP", "GIMP".to_string());
        assert!(state.work.is_empty());
        let _ = take(&mut state, "org.videolan.VLC", "VLC".to_string());
        assert_eq!(state.work.len(), 1);
        assert!(state.work[0].remove);
        // and not twice over
        let _ = take(&mut state, "org.videolan.VLC", "VLC".to_string());
        assert_eq!(state.work.len(), 1);
    }
}
