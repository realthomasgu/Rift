//! The app: a window for each folder the owner opens, each with a header bar, the places down the
//! left and a list of what is in the folder, the way GNOME's Files is laid out. Drawn with iced in
//! software, in the colours the owner has chosen, and following them when they change.
//!
//! The app is a daemon: it runs while a window is open or a job is still copying, and
//! `rift-files` asks the one that is running for another window.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use iced::futures::channel::{mpsc, oneshot};
use iced::keyboard::{self, Modifiers};
use iced::widget::scrollable::Viewport;
use iced::{Point, Size, Subscription, Task, Theme, event, mouse, theme, window};
use librift::appearance::Look;
use librift::drives::{self, Exchange, Volume};
use librift::files::places::{self, Place};
use librift::files::trash::{self as bin, Trash, Trashed};
use librift::files::{self, Entry, Options, Sort, mime};

use crate::browser::{Browser, Location};
use crate::control::{self, Command};
use crate::jobs::{self, Job, Step};
use crate::theme::{Colors, colors};
use crate::thumbs::Thumbs;
use crate::widgets::{FONT, TEXT_SIZE};
use crate::{actions, find, list, thumbs, view};

/// What the windows call themselves: the name of the desktop entry, which the dock, the compositor
/// and the boot test all know them by.
pub const APP_ID: &str = "dev.rift.Files";

/// How big a window opens.
const WIDTH: f32 = 960.0;
const HEIGHT: f32 = 640.0;

/// How often the folders on screen are looked at for a change.
const LOOK: Duration = Duration::from_secs(1);

/// How long an app the session bus started waits for the call that follows before it gives up and
/// ends. Nothing is on screen in the meantime, so nothing is taken away from anyone.
const BUS_WAIT: Duration = Duration::from_secs(30);

/// What a place's own folders said when they were last read: each one's time and size.
pub type Stamp = Vec<(i64, i64, u64)>;

/// What the app is started with.
#[derive(Debug, Default)]
pub struct Start {
    /// The folders to open a window on, or files to show in theirs. Home when there are none.
    pub open: Vec<PathBuf>,
    /// Where to save a picture of the first window once it has drawn, and then quit.
    pub screenshot: Option<PathBuf>,
    /// Whether the session bus started the app: it opens no window of its own and waits for the
    /// call that follows to say what to show.
    pub bus: bool,
}

/// What Cut and Copy put on the app's clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    /// What was cut or copied.
    pub paths: Vec<PathBuf>,
    /// Whether it was cut, so a paste moves it.
    pub cut: bool,
    /// The paths as the text the session's clipboard got, one a line, to tell whether the
    /// clipboard still holds them when they are pasted.
    pub text: String,
}

/// The app's state.
pub struct Files {
    /// The windows, each with its own folder.
    pub windows: BTreeMap<window::Id, Browser>,
    /// The window in front, which `--set` and `--state` are about.
    pub front: Option<window::Id>,
    /// How many windows have opened, to number the next.
    opened: usize,
    /// Dark or light and the accent, which the windows follow.
    pub look: Look,
    /// Whether hidden files show and the order of a list, for every window.
    pub options: Options,
    /// The kinds of file.
    pub types: Arc<mime::Database>,
    /// The small pictures of files the grid draws, and the programs that make them.
    pub thumbs: Thumbs,
    /// Home and the folders the sidebar lists.
    pub places: Vec<Place>,
    /// The disks a person plugged in, as udisks last said.
    pub drives: Vec<Volume>,
    /// The moments Vault last listed, oldest first, for the windows that show one.
    pub moments: Vec<String>,
    /// The drive's own exchange partition: where it is mounted, or that it is there to be mounted,
    /// which is what a Ghost session has.
    pub exchange: Exchange,
    /// The disks being mounted, unmounted or ejected at the moment, by what names them on the bus,
    /// so a row says what it is doing and is not pressed twice.
    pub working: Vec<String>,
    /// What was cut or copied last.
    pub clipboard: Option<Clip>,
    /// The jobs that are running, and the ones that ended, for Undo.
    pub jobs: Vec<Job>,
    /// The local zone's distance from UTC, in seconds.
    pub offset: i32,
    /// Whether anything is in the trash.
    pub trash_full: bool,
    /// The modifier keys held down, for a click with Ctrl or Shift.
    pub modifiers: Modifiers,
    /// Where `--screenshot` saves a picture of the window.
    screenshot: Option<PathBuf>,
    /// Where `--set picture` saves the next picture of the window in front.
    picture: Option<PathBuf>,
    /// Whether the picture `--screenshot` asked for is on its way.
    shooting: bool,
    /// While the session bus started the app and no window has opened yet: when to give up
    /// waiting for the call that follows.
    waiting: Option<std::time::Instant>,
}

/// What an app on the session bus asked Files to show, through `org.freedesktop.FileManager1`.
// only the bus makes one, and a bus is a linux thing
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Show {
    /// These files, each in the folder it is in and selected there.
    Items(Vec<PathBuf>),
    /// These folders.
    Folders(Vec<PathBuf>),
    /// What is known about these files, in the Properties dialog over their folder.
    Properties(Vec<PathBuf>),
}

/// What can be done to the selection or in a folder, from a menu, a key or the socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Open the selection: go into a folder, or open files with the apps that open them.
    Open,
    /// Open the selection with this app.
    OpenWith(String),
    /// Open the selected folder in a window of its own.
    OpenWindow,
    /// Make a folder here.
    NewFolder,
    /// Rename the one thing selected.
    Rename,
    /// Put the selection on the clipboard to copy.
    Copy,
    /// Put the selection on the clipboard to move.
    Cut,
    /// Copy or move what is on the clipboard here.
    Paste,
    /// Move the selection to the trash.
    Trash,
    /// Delete the selection for good, after asking.
    Delete,
    /// Put the selection in the trash back.
    Restore,
    /// Delete the selection in the trash for good, after asking.
    Forget,
    /// Empty the trash, after asking.
    Empty,
    /// Select everything.
    SelectAll,
    /// Show hidden files, or stop.
    Hidden,
    /// Put the list in this order, or the other way round when it is already.
    Sort(Sort),
    /// Open a terminal in this folder.
    Terminal,
    /// Open another window.
    NewWindow,
    /// Close this window.
    Close,
    /// Read the folder again.
    Reload,
    /// Type where to go in the path bar.
    Location,
    /// Take back what this job did.
    Undo(u64),
    /// Stop this job.
    Stop(u64),
    /// Mount this disk and go to it.
    Mount(String),
    /// Unmount everything on this disk and eject it.
    Eject(String),
    /// Show this folder as it was, at the newest moment Vault has.
    Timeline,
    /// The moment before this one, or the one after it.
    Step {
        /// Whether it steps back in time.
        earlier: bool,
    },
    /// Leave the Timeline and show the folder as it is now.
    Now,
    /// Bring what is selected in a moment back into the folder, or everything in the moment when
    /// nothing is selected.
    Bring,
    /// Search this folder and what is under it: the field in the header bar opens.
    Search,
    /// Show the folder as a grid of pictures, or as a list of rows.
    Grid(bool),
    /// What is known about what is selected.
    Properties,
    /// Ask for the passphrase of this locked disk.
    Unlock(String),
    /// Mount the drive's own exchange partition and go to it, which only a Ghost session needs:
    /// every other boot has it mounted before anyone logs in.
    MountExchange,
}

/// What a press, a key, a line on the socket or a job asks for.
#[derive(Debug, Clone)]
pub enum Message {
    /// A row of the list was pressed.
    Press(window::Id, usize),
    /// A row was pressed twice.
    Twice(window::Id, usize),
    /// A row was pressed with the right button.
    RowMenu(window::Id, usize),
    /// The pointer came onto a row.
    Hover(window::Id, usize),
    /// The pointer left a row.
    Unhover(window::Id, usize),
    /// The empty part of the list was pressed.
    Blank(window::Id),
    /// The empty part of the list was pressed with the right button.
    BlankMenu(window::Id),
    /// Where a press landed, before what it was on.
    At(window::Id, Point),
    /// The list was scrolled.
    Scrolled(window::Id, Viewport),
    /// Go to a folder or the trash.
    Go(window::Id, Location),
    /// Back through the history.
    Back(window::Id),
    /// Forward through it.
    Forward(window::Id),
    /// Up to the folder this one is in.
    Up(window::Id),
    /// What reading a folder found.
    Read(window::Id, Location, Box<Result<Vec<Entry>, String>>),
    /// What reading the trash found.
    TrashRead(window::Id, Vec<Trashed>),
    /// The moments Vault listed, for the window that asked.
    Moments(window::Id, Box<Result<Vec<String>, String>>),
    /// What is typed in the search field.
    SearchTyped(window::Id, String),
    /// Enter in the search field, which searches by meaning.
    SearchEntered(window::Id),
    /// What a search found: the words it was for, whether it was by meaning, and the rows or the
    /// sentence to show in their place.
    Searched(window::Id, String, bool, Box<Result<Vec<Entry>, String>>),
    /// Open the header bar's menu.
    MainMenu(window::Id),
    /// Close the menu.
    CloseMenu(window::Id),
    /// Do something to the selection or in the folder.
    Do(window::Id, Act),
    /// What is typed in a dialog's field.
    Typed(window::Id, String),
    /// The dialog's default button.
    Confirm(window::Id),
    /// The other answer to the question before a name is replaced.
    Replace(window::Id),
    /// The dialog's Cancel.
    Cancel(window::Id),
    /// What is typed in the path bar.
    PathTyped(window::Id, String),
    /// Enter in the path bar.
    PathEntered(window::Id),
    /// A key the window did not use.
    Key(window::Id, keyboard::Key, Modifiers),
    /// Escape, which closes whatever is open even from a field.
    Escape(window::Id),
    /// The modifier keys changed.
    Modifiers(Modifiers),
    /// A window changed size.
    Resized(window::Id, Size),
    /// A window came to the front.
    Focused(window::Id),
    /// The compositor asked a window to close.
    CloseRequested(window::Id),
    /// A window has closed.
    Closed(window::Id),
    /// Time to look at the folders on screen again.
    Tick,
    /// How a job is going.
    Job(u64, Step),
    /// A toast's time is up.
    ToastGone(window::Id, u64),
    /// What the session's clipboard held when Paste was pressed.
    Pasted(window::Id, Option<String>),
    /// A line from the socket.
    Said(Command),
    /// An app on the session bus asked for something to be shown.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Show(Show),
    /// A picture of a window.
    Shot(window::Screenshot),
    /// A window has opened.
    Opened(window::Id),
    /// What udisks says is plugged in now.
    Drives(Vec<Volume>),
    /// Whether the drive has an exchange partition, as Vault answered. Asked once, since a drive
    /// does not grow one while the machine is on.
    Exchange(bool),
    /// The exchange partition was mounted, or it was not: where it went or what went wrong.
    Mounted(Option<window::Id>, Box<Result<PathBuf, String>>),
    /// A disk was mounted, unmounted or ejected, or it was not: which one, what was being done,
    /// and where it went or what went wrong.
    Disk(
        Option<window::Id>,
        String,
        Box<Result<Option<PathBuf>, String>>,
    ),
    /// The picture of a file, when one was made or found, and nothing when there is none to have.
    Thumb(PathBuf, Option<i64>, Option<PathBuf>),
    /// What the Properties dialog was waiting to know: how big it is, and how many pixels across a
    /// picture is.
    Measured(window::Id, String, Option<(u32, u32)>),
    /// A locked disk was unlocked and what came out of it mounted, or it was not.
    Unlocked(window::Id, String, Box<Result<PathBuf, String>>),
}

/// Run until the last window is closed and no job is running.
///
/// # Errors
///
/// When a window cannot be opened.
pub fn run(start: Start) -> iced::Result {
    let ran = iced::daemon(move || boot(&start), update, view::window)
        .title(|state: &Files, id| {
            state
                .windows
                .get(&id)
                .map_or_else(|| "Files".to_string(), |browser| browser.location.label())
        })
        .theme(|state: &Files, _| {
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
        .scale_factor(|state: &Files, _| {
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

/// A window's settings. On Wayland the app id comes from the platform settings and nowhere else,
/// and it is the name of the desktop entry, which is how the dock and the compositor know the
/// window.
fn settings(screenshot: bool) -> window::Settings {
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut settings = window::Settings {
        size: Size::new(WIDTH, if screenshot { 720.0 } else { HEIGHT }),
        min_size: Some(Size::new(560.0, 360.0)),
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

fn boot(start: &Start) -> (Files, Task<Message>) {
    let mut state = Files {
        windows: BTreeMap::new(),
        front: None,
        opened: 0,
        look: Look::read(),
        options: Options::read(),
        types: Arc::new(mime::Database::load()),
        thumbs: Thumbs::load(),
        places: places::places(),
        drives: Vec::new(),
        moments: Vec::new(),
        exchange: first_exchange(),
        working: Vec::new(),
        clipboard: None,
        jobs: Vec::new(),
        offset: files::utc_offset(),
        trash_full: false,
        modifiers: Modifiers::empty(),
        screenshot: start.screenshot.clone(),
        picture: None,
        shooting: false,
        waiting: start.bus.then(|| std::time::Instant::now() + BUS_WAIT),
    };
    state.trash_full = state.anything_trashed();
    let mut work: Vec<Task<Message>> = if start.bus {
        // the call that started the app says what to show, and opens the window for it
        Vec::new()
    } else if start.open.is_empty() {
        vec![open_window(&mut state, home_location(), None)]
    } else {
        start
            .open
            .iter()
            .map(|path| open_path(&mut state, path))
            .collect()
    };
    // whether the drive has an exchange partition is written in its table and nowhere else, so
    // Vault is the one that can say. It is asked once: a drive does not grow one while the machine
    // is on, and on every boot but a Ghost one the partition is mounted here already
    if state.exchange == Exchange::None {
        work.push(actions::look_at_exchange());
    }
    keep(&state);
    (state, Task::batch(work))
}

/// What the drive's exchange partition starts as: the folder it is mounted at, which is every boot
/// but a Ghost one, or nothing until Vault has said whether the drive has one at all.
fn first_exchange() -> Exchange {
    if let Some(path) = drives::exchange() {
        return Exchange::At(path);
    }
    #[cfg(debug_assertions)]
    if AS_GHOST.load(std::sync::atomic::Ordering::Relaxed) {
        // told to draw as a Ghost session does, so the row is there and nothing has mounted it
        return Exchange::There;
    }
    Exchange::None
}

/// Whether this process was told to draw as a Ghost session does. A debug build only, for a picture
/// of the sidebar on a machine that cannot boot a Rift drive (ADR-0085).
#[cfg(debug_assertions)]
static AS_GHOST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Draw as a Ghost session does from here on. `--as-ghost` calls it before anything is read.
#[cfg(debug_assertions)]
pub fn as_ghost() {
    AS_GHOST.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Home, or the root of the file system on a machine with no home.
fn home_location() -> Location {
    Location::Folder(
        std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map_or_else(|| PathBuf::from("/"), PathBuf::from),
    )
}

/// A window on a folder, or on the folder a file is in with the file selected. The trash's own
/// address opens the trash.
pub fn open_path(state: &mut Files, path: &Path) -> Task<Message> {
    if files::is_trash(path) {
        return open_window(state, Location::Trash, None);
    }
    if path.as_os_str().is_empty() {
        return open_window(state, home_location(), None);
    }
    match (path.is_dir(), path.parent(), path.file_name()) {
        (true, _, _) => open_window(state, Location::Folder(path.to_path_buf()), None),
        (false, Some(folder), Some(name)) if folder.is_dir() => open_window(
            state,
            Location::Folder(folder.to_path_buf()),
            Some(name.to_owned()),
        ),
        _ => open_window(state, home_location(), None),
    }
}

/// Open a window on a place, with one name selected in it when it is given.
pub fn open_window(
    state: &mut Files,
    location: Location,
    select: Option<OsString>,
) -> Task<Message> {
    let (id, opened) = window::open(settings(state.screenshot.is_some()));
    state.opened += 1;
    let mut browser = Browser::new(state.opened, location, Size::new(WIDTH, HEIGHT));
    browser.stamp = stamp(state, &browser.location);
    if let Some(name) = select {
        browser.select_after = vec![name];
    }
    state.windows.insert(id, browser);
    state.front = Some(id);
    Task::batch([opened.map(Message::Opened), read(state, id)])
}

/// For `--screenshot`, once the first window has read its folder: wait a moment for it to draw
/// itself, then take the picture.
fn shoot(state: &mut Files) -> Task<Message> {
    if state.screenshot.is_none() || state.shooting {
        return Task::none();
    }
    state.shooting = true;
    Task::perform(async { thread::sleep(Duration::from_millis(900)) }, |()| ())
        .then(|()| window::oldest())
        .and_then(window::screenshot)
        .map(Message::Shot)
}

/// Write a picture of a window as a png.
fn save(path: &Path, shot: &window::Screenshot) -> Result<(), String> {
    let (wide, tall) = (shot.size.width, shot.size.height);
    image::RgbaImage::from_raw(wide, tall, shot.rgba.to_vec())
        .ok_or_else(|| format!("the picture is not {wide} by {tall}"))?
        .save(path)
        .map_err(|e| format!("Could not write {}: {e}", path.display()))
}

/// Read what a window shows, on a thread of its own: the folder, the folder inside a snapshot,
/// the trash, or what the search field is looking for.
pub fn read(state: &Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get(&id) else {
        return Task::none();
    };
    if let Some(words) = browser.searching() {
        let meaning = browser.search.as_ref().is_some_and(|query| query.meaning);
        return search(state, id, words.to_string(), meaning);
    }
    let location = browser.location.clone();
    let types = Arc::clone(&state.types);
    let offset = state.offset;
    let bins = state.trashes();
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let message = match (&location, location.place()) {
            (_, Some(path)) => {
                let found = files::read(&path, &types).map_err(|why| missing(&location, why));
                Message::Read(id, location, Box::new(found))
            }
            (Location::Trash, None) => {
                let mut found: Vec<Trashed> =
                    bins.iter().flat_map(|trash| trash.list(offset)).collect();
                bin::newest_first(&mut found);
                Message::TrashRead(id, found)
            }
            (_, None) => Message::Read(
                id,
                location,
                Box::new(Err("This folder is not in the Timeline.".to_string())),
            ),
        };
        let _ = sender.send(message);
    });
    Task::perform(receiver, move |said| said.unwrap_or(Message::CloseMenu(id)))
}

/// Why a place cannot be shown. A folder in a moment that is not there was not there then, which
/// is not the same as a folder that is gone.
fn missing(location: &Location, why: String) -> String {
    match location {
        Location::Moment { .. } if !location.place().is_some_and(|path| path.exists()) => {
            "There was no such folder at this moment.".to_string()
        }
        _ => why,
    }
}

/// Search the folder and what is under it, on a thread of its own: by name, or by meaning through
/// the index of home.
fn search(state: &Files, id: window::Id, words: String, meaning: bool) -> Task<Message> {
    let Some(root) = state
        .windows
        .get(&id)
        .and_then(|browser| browser.location.place())
    else {
        return Task::none();
    };
    let types = Arc::clone(&state.types);
    let hidden = state.options.hidden;
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let found = if meaning {
            find::by_meaning(&root, &words, &types)
        } else {
            Ok(find::by_name(&root, &words, hidden, &types))
        };
        let _ = sender.send(Message::Searched(id, words, meaning, Box::new(found)));
    });
    Task::perform(receiver, move |said| said.unwrap_or(Message::CloseMenu(id)))
}

/// What a place's own entries say when they were last changed, to tell when something in them
/// changes: the folder's time and size, or every trash's notes folder's.
#[must_use]
pub fn stamp(state: &Files, location: &Location) -> Stamp {
    use std::os::unix::fs::MetadataExt;
    let folders: Vec<PathBuf> = match location {
        Location::Folder(path) => vec![path.clone()],
        // a snapshot is read only, so what it holds never changes under the window
        Location::Moment { .. } => Vec::new(),
        Location::Trash => state
            .trashes()
            .iter()
            .map(|trash| trash.root().join("info"))
            .collect(),
    };
    folders
        .iter()
        .filter_map(|folder| std::fs::metadata(folder).ok())
        .map(|meta| (meta.mtime(), meta.mtime_nsec(), meta.len()))
        .collect()
}

impl Files {
    /// The tops of the file systems a drive's own trash can be at: the exchange partition of the
    /// drive, and every disk that is mounted.
    #[must_use]
    pub fn tops(&self) -> Vec<PathBuf> {
        self.exchange
            .mount()
            .map(Path::to_path_buf)
            .into_iter()
            .chain(self.drives.iter().filter_map(|drive| drive.mount.clone()))
            .collect()
    }

    /// Every trash the owner's things can be in: the one in home, and the one at the top of each
    /// drive that is mounted.
    #[must_use]
    pub fn trashes(&self) -> Vec<Trash> {
        let uid = files::uid();
        Trash::home()
            .into_iter()
            .chain(self.tops().iter().map(|top| Trash::on(top, uid)))
            .collect()
    }

    /// Whether anything is in any of them.
    #[must_use]
    pub fn anything_trashed(&self) -> bool {
        self.trashes().iter().any(|trash| trash.count() > 0)
    }

    /// The disk this names, when udisks still says it is there.
    #[must_use]
    pub fn drive(&self, id: &str) -> Option<&Volume> {
        self.drives.iter().find(|drive| drive.id == id)
    }

    /// The colours the windows are drawn in.
    #[must_use]
    pub fn colors(&self) -> Colors {
        colors(self.look.theme, self.look.accent)
    }

    /// Whether a job is still running.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.jobs.iter().any(Job::running)
    }

    /// The window in front, or any window when none has come to the front yet.
    #[must_use]
    pub fn front_id(&self) -> Option<window::Id> {
        self.front
            .filter(|id| self.windows.contains_key(id))
            .or_else(|| self.windows.keys().next().copied())
    }

    /// The lines `rift-files --state` prints: the app's, then the window in front's.
    fn state(&self) -> String {
        let mut lines = vec![
            format!("windows {}", self.windows.len()),
            format!("trash {}", if self.trash_full { "full" } else { "empty" }),
            format!("hidden {}", if self.options.hidden { "on" } else { "off" }),
            format!("view {}", if self.options.grid { "grid" } else { "list" }),
            format!("thumbnails {}", self.thumbs.count()),
            format!(
                "sort {}{}",
                self.options.sort.word(),
                if self.options.reversed {
                    " reversed"
                } else {
                    ""
                }
            ),
            match &self.clipboard {
                Some(clip) => format!(
                    "clipboard {} {}",
                    if clip.cut { "cut" } else { "copy" },
                    clip.paths.len()
                ),
                None => "clipboard none".to_string(),
            },
            format!("exchange {}", self.exchange.word()),
        ];
        for drive in &self.drives {
            lines.push(format!(
                "drive {} {} {}",
                drive.name,
                if drive.locked {
                    "locked"
                } else if drive.mounted() {
                    "mounted"
                } else {
                    "there"
                },
                drive
                    .mount
                    .as_ref()
                    .map_or_else(|| "none".to_string(), |path| path.display().to_string()),
            ));
        }
        for job in self.jobs.iter().filter(|job| job.running()) {
            lines.push(format!("job {} {}", job.number, job.percent()));
        }
        lines.push(format!("moments {}", self.moments.len()));
        if let Some(browser) = self.front_id().and_then(|id| self.windows.get(&id)) {
            lines.extend(list::state(browser));
        }
        lines.join("\n") + "\n"
    }
}

/// Handle a message, and keep what `--state` prints up to date: the socket answers from it on a
/// thread of its own, and there may be no window to draw it.
fn update(state: &mut Files, message: Message) -> Task<Message> {
    let task = handle(state, message);
    keep(state);
    task
}

fn handle(state: &mut Files, message: Message) -> Task<Message> {
    match message {
        Message::Press(id, at) => actions::press(state, id, at),
        Message::Twice(id, at) => actions::twice(state, id, at),
        Message::RowMenu(id, at) => actions::row_menu(state, id, at),
        Message::Hover(..)
        | Message::Unhover(..)
        | Message::Blank(_)
        | Message::At(..)
        | Message::Modifiers(_)
        | Message::Focused(_)
        | Message::Opened(_) => {
            pointer(state, &message);
            Task::none()
        }
        // what is on screen has changed, so the pictures of the tiles there are asked for
        Message::Scrolled(id, _) | Message::Resized(id, _) => {
            pointer(state, &message);
            thumbs::want(state, id)
        }
        Message::BlankMenu(id) => actions::blank_menu(state, id),
        Message::Go(id, location) => go(state, id, location),
        Message::Back(id) => travel(state, id, Browser::go_back),
        Message::Forward(id) => travel(state, id, Browser::go_forward),
        Message::Up(id) => up(state, id),
        Message::MainMenu(id) => actions::main_menu(state, id),
        Message::CloseMenu(id) => {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.menu = None;
            }
            Task::none()
        }
        Message::Do(id, act) => {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.menu = None;
            }
            actions::act(state, id, act)
        }
        Message::Typed(id, typed) => {
            if let Some(dialog) = state.windows.get_mut(&id).and_then(|b| b.dialog.as_mut()) {
                dialog.type_in(typed);
            }
            Task::none()
        }
        Message::Confirm(id) => actions::confirm(state, id),
        Message::Replace(id) => actions::replace(state, id),
        Message::Cancel(id) => {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.dialog = None;
            }
            Task::none()
        }
        Message::PathTyped(id, typed) => {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.typing = Some(typed);
            }
            Task::none()
        }
        Message::PathEntered(id) => actions::path_entered(state, id),
        Message::Key(id, key, modifiers) => actions::key(state, id, &key, modifiers),
        Message::Escape(id) => actions::escape(state, id),
        Message::CloseRequested(id) => window::close(id),
        Message::Closed(id) => {
            state.windows.remove(&id);
            if state.front == Some(id) {
                state.front = None;
            }
            if state.windows.is_empty() && !state.busy() {
                return iced::exit();
            }
            Task::none()
        }
        other => answered(state, other),
    }
}

/// What comes back from somewhere else: a folder, a search or a disk read on a thread of its own,
/// a job, the clipboard, the socket, or a picture of a window.
fn answered(state: &mut Files, message: Message) -> Task<Message> {
    match message {
        Message::Read(id, location, found) => arrived(state, id, &location, *found),
        Message::TrashRead(id, trashed) => trash_read(state, id, trashed),
        Message::Moments(id, listed) => actions::moments(state, id, *listed),
        Message::SearchTyped(id, typed) => actions::search_typed(state, id, typed),
        Message::SearchEntered(id) => actions::search_entered(state, id),
        Message::Searched(id, words, meaning, found) => {
            actions::searched(state, id, &words, meaning, *found)
        }
        Message::Tick => tick(state),
        Message::Drives(found) => drives_read(state, found),
        Message::Exchange(there) => {
            if there && state.exchange == Exchange::None {
                state.exchange = Exchange::There.again();
            }
            Task::none()
        }
        Message::Mounted(id, done) => actions::exchange_done(state, id, *done),
        Message::Disk(id, drive, done) => actions::disk_done(state, id, &drive, *done),
        Message::Job(number, step) => actions::job_step(state, number, step),
        Message::ToastGone(id, number) => {
            if let Some(browser) = state.windows.get_mut(&id)
                && browser
                    .toast
                    .as_ref()
                    .is_some_and(|toast| toast.number == number)
            {
                browser.toast = None;
            }
            Task::none()
        }
        Message::Thumb(path, modified, picture) => thumbs::thumb(state, path, modified, picture),
        Message::Measured(id, size, pixels) => actions::measured(state, id, size, pixels),
        Message::Unlocked(id, drive, done) => actions::unlocked(state, id, &drive, *done),
        Message::Pasted(id, text) => actions::pasted(state, id, text.as_deref()),
        Message::Said(command) => said(state, command),
        Message::Show(what) => shown(state, what),
        Message::Shot(shot) => {
            let path = state.picture.take().or_else(|| state.screenshot.clone());
            if let Some(path) = &path
                && let Err(why) = save(path, &shot)
            {
                eprintln!("rift-files: {why}");
            }
            if state.screenshot.is_some() {
                return iced::exit();
            }
            Task::none()
        }
        _ => Task::none(),
    }
}

/// What the pointer, the keys held down and the compositor say about a window, which changes what
/// is drawn and nothing else.
fn pointer(state: &mut Files, message: &Message) {
    match *message {
        Message::Modifiers(modifiers) => state.modifiers = modifiers,
        Message::Focused(id) | Message::Opened(id) => state.front = Some(id),
        Message::At(id, point) => {
            state.front = Some(id);
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.pointer = point;
            }
        }
        Message::Hover(id, at) => {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.hover = Some(at);
            }
        }
        Message::Unhover(id, at) => {
            if let Some(browser) = state.windows.get_mut(&id)
                && browser.hover == Some(at)
            {
                browser.hover = None;
            }
        }
        Message::Blank(id) => {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.select_none();
                browser.menu = None;
            }
        }
        Message::Scrolled(id, ref viewport) => {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.scroll = viewport.absolute_offset().y;
                browser.viewport = viewport.bounds().height;
            }
        }
        Message::Resized(id, size) => {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.size = size;
            }
        }
        _ => {}
    }
}

/// The trash has been read, for a window that still shows it.
fn trash_read(state: &mut Files, id: window::Id, trashed: Vec<Trashed>) -> Task<Message> {
    let options = state.options;
    let types = Arc::clone(&state.types);
    state.trash_full = !trashed.is_empty();
    let shown = match state.windows.get_mut(&id) {
        Some(browser) if browser.location == Location::Trash && browser.searching().is_none() => {
            browser.show_trash(trashed, &types, options);
            actions::select_waiting(browser, options.grid)
        }
        _ => Task::none(),
    };
    Task::batch([shown, shoot(state)])
}

/// Go somewhere in a window, and read it.
pub fn go(state: &mut Files, id: window::Id, location: Location) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    browser.go(location);
    let now = stamp(state, &state.windows[&id].location);
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.stamp = now;
    }
    read(state, id)
}

/// Back or forward, when there is somewhere to go.
pub fn travel(state: &mut Files, id: window::Id, way: fn(&mut Browser) -> bool) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    if !way(browser) {
        return Task::none();
    }
    let now = stamp(state, &state.windows[&id].location);
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.stamp = now;
    }
    read(state, id)
}

/// Up to the folder this one is in, with this one selected there.
pub fn up(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    let Some(folder) = browser.location.folder().map(Path::to_path_buf) else {
        return Task::none();
    };
    let (Some(parent), Some(name)) = (folder.parent(), folder.file_name()) else {
        return Task::none();
    };
    let parent = parent.to_path_buf();
    let name = name.to_owned();
    browser.go(Location::Folder(parent));
    browser.select_after = vec![name];
    let now = stamp(state, &state.windows[&id].location);
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.stamp = now;
    }
    read(state, id)
}

/// A folder has been read. Only the place the window is still at counts, and only while the list
/// is the folder's own: a read that was on its way when the search field was opened would take the
/// place of what the search found.
fn arrived(
    state: &mut Files,
    id: window::Id,
    location: &Location,
    found: Result<Vec<Entry>, String>,
) -> Task<Message> {
    let options = state.options;
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    if &browser.location != location || browser.searching().is_some() {
        return Task::none();
    }
    let shown = match found {
        Ok(entries) => {
            browser.show(entries, options);
            actions::select_waiting(browser, options.grid)
        }
        Err(why) => {
            browser.read.clear();
            browser.rows.clear();
            browser.ready = true;
            browser.problem = Some(why);
            Task::none()
        }
    };
    Task::batch([
        shown,
        asked_about(state, id),
        thumbs::want(state, id),
        shoot(state),
    ])
}

/// What is known about the selection, for a window a `ShowItemProperties` call opened: the folder
/// has been read now, so there is something to say about what it selected.
fn asked_about(state: &mut Files, id: window::Id) -> Task<Message> {
    let asked = state
        .windows
        .get_mut(&id)
        .is_some_and(|browser| std::mem::take(&mut browser.properties_after));
    if !asked {
        return Task::none();
    }
    actions::act(state, id, Act::Properties)
}

/// What udisks says is plugged in now. A drive coming or going takes its own trash with it, so a
/// window on the trash reads it again.
fn drives_read(state: &mut Files, found: Vec<Volume>) -> Task<Message> {
    if state.drives == found {
        return Task::none();
    }
    // a disk that is gone is no longer being worked on
    let still: Vec<String> = state
        .working
        .iter()
        .filter(|id| found.iter().any(|drive| &&drive.id == id))
        .cloned()
        .collect();
    state.working = still;
    state.drives = found;
    state.trash_full = state.anything_trashed();
    let showing: Vec<window::Id> = state
        .windows
        .iter()
        .filter(|(_, browser)| browser.location == Location::Trash)
        .map(|(id, _)| *id)
        .collect();
    Task::batch(showing.into_iter().map(|id| read(state, id)))
}

/// Look at every window's place for a change, and at the trash and the colours.
fn tick(state: &mut Files) -> Task<Message> {
    // an app the bus started that was never told what to show ends rather than sit there
    if let Some(until) = state.waiting {
        if !state.windows.is_empty() || state.busy() {
            state.waiting = None;
        } else if std::time::Instant::now() > until {
            return iced::exit();
        }
    }
    let look = Look::read();
    if look != state.look {
        state.look = look;
    }
    state.exchange = state.exchange.again();
    state.trash_full = state.anything_trashed();
    let stamps: Vec<(window::Id, Stamp)> = state
        .windows
        .iter()
        .map(|(id, browser)| (*id, stamp(state, &browser.location)))
        .collect();
    let mut changed = Vec::new();
    for (id, now) in stamps {
        if let Some(browser) = state.windows.get_mut(&id)
            && now != browser.stamp
        {
            browser.stamp = now;
            // a search by meaning stands still: reading it again would ask Quasar for the words
            // again every time anything in the folder changed
            if !browser.search.as_ref().is_some_and(|query| query.meaning) {
                changed.push(id);
            }
        }
    }
    Task::batch(changed.into_iter().map(|id| read(state, id)))
}

/// A line from the socket. Setting something over it does what pressing it would in the window in
/// front.
fn said(state: &mut Files, command: Command) -> Task<Message> {
    match command {
        Command::Open(path) => {
            let path = files::path_of(&path);
            if path.as_os_str().is_empty() {
                open_window(state, home_location(), None)
            } else {
                open_path(state, &path)
            }
        }
        Command::Set(name, value) => actions::set(state, &name, &value),
        // answered on the socket's own thread
        Command::State => Task::none(),
    }
}

/// What an app on the bus asked to be shown: a window for each folder, with the files it named in
/// that folder selected, and Properties over the first of them when it asked for that. A window is
/// opened for it whether or not one is open already, the way a call that starts Files opens one.
fn shown(state: &mut Files, what: Show) -> Task<Message> {
    match what {
        Show::Items(paths) => in_folders(state, &paths, false),
        Show::Properties(paths) => in_folders(state, &paths, true),
        Show::Folders(paths) => {
            let mut work = Vec::new();
            for path in &paths {
                work.push(open_path(state, path));
            }
            Task::batch(work)
        }
    }
}

/// One window for each folder the files are in, with the files in it selected once it has been
/// read. A folder named here is shown in the folder over it, selected, which is what Show in
/// folder means for a folder.
fn in_folders(state: &mut Files, paths: &[PathBuf], properties: bool) -> Task<Message> {
    let mut folders: Vec<(PathBuf, Vec<OsString>)> = Vec::new();
    for path in paths {
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            continue;
        };
        if !folder.is_dir() {
            continue;
        }
        match folders.iter_mut().find(|(there, _)| there == folder) {
            Some((_, names)) => names.push(name.to_owned()),
            None => folders.push((folder.to_path_buf(), vec![name.to_owned()])),
        }
    }
    let mut work = Vec::new();
    for (folder, names) in folders {
        work.push(open_showing(state, folder, names, properties));
    }
    Task::batch(work)
}

/// A window on a folder with these names selected once it has been read, and what is known about
/// them over it when the call asked for that.
fn open_showing(
    state: &mut Files,
    folder: PathBuf,
    names: Vec<OsString>,
    properties: bool,
) -> Task<Message> {
    let opening = open_window(state, Location::Folder(folder), None);
    // the window it opened is the one in front, and the one to select in when it has been read
    if let Some(browser) = state.front.and_then(|id| state.windows.get_mut(&id)) {
        browser.select_after = names;
        browser.properties_after = properties;
    }
    opening
}

/// Take a picture of the window in front for `--set picture`.
pub fn picture(state: &mut Files, path: PathBuf) -> Task<Message> {
    let Some(id) = state.front_id() else {
        return Task::none();
    };
    state.picture = Some(path);
    window::screenshot(id).map(Message::Shot)
}

fn subscription(state: &Files) -> Subscription<Message> {
    let mut followed = vec![
        window::close_requests().map(Message::CloseRequested),
        window::close_events().map(Message::Closed),
        window::resize_events().map(|(id, size)| Message::Resized(id, size)),
        event::listen_with(events),
    ];
    // a window that is only there to have its picture taken answers nothing and follows nothing,
    // so a running Files keeps its socket
    if state.screenshot.is_none() {
        followed.push(terminal());
        followed.push(ticking());
        followed.push(disks());
        #[cfg(target_os = "linux")]
        followed.push(crate::bus::serve());
    }
    Subscription::batch(followed)
}

/// The keys, the pointer's back and forward buttons and a window coming to the front. A key a
/// field has taken is its own, except Escape, which closes whatever is open.
fn events(event: iced::Event, status: event::Status, id: window::Id) -> Option<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
            Some(Message::Modifiers(modifiers))
        }
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) => Some(Message::Escape(id)),
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
            if status == event::Status::Ignored =>
        {
            Some(Message::Key(id, key, modifiers))
        }
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => {
            Some(Message::Back(id))
        }
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => {
            Some(Message::Forward(id))
        }
        iced::Event::Window(window::Event::Focused) => Some(Message::Focused(id)),
        _ => None,
    }
}

/// A tick every second, to look at the folders on screen for a change. The subscription is named,
/// because iced tells two apart by the type of the stream and the address of the function that
/// makes it.
fn ticking() -> Subscription<Message> {
    Subscription::run_with("tick", |_| {
        let (sender, receiver) = mpsc::unbounded();
        thread::spawn(move || {
            loop {
                thread::sleep(LOOK);
                if sender.unbounded_send(Message::Tick).is_err() {
                    return;
                }
            }
        });
        receiver
    })
}

/// The disks a person plugs in, read from udisks now and again whenever it says something has
/// changed. Without udisks, which is every machine that is not a Rift drive, the watch fails, is
/// tried again after a few seconds, and the list stays empty.
fn disks() -> Subscription<Message> {
    Subscription::run_with("disks", |_| {
        let (sender, receiver) = mpsc::unbounded();
        librift::bus::follow(
            |poke| {
                // the watch is tried again every few seconds, and a machine with no udisks would
                // say the same thing for ever, so it is said once
                let said = std::sync::atomic::AtomicBool::new(false);
                librift::bus::listen(
                    poke,
                    |each: &mut dyn FnMut() -> bool| drives::watch(each),
                    move |why| {
                        if !said.swap(true, std::sync::atomic::Ordering::Relaxed) {
                            eprintln!("rift-files: {why}");
                        }
                    },
                );
            },
            || drives::volumes().unwrap_or_default(),
            move |found| sender.unbounded_send(Message::Drives(found)).is_ok(),
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
                eprintln!("rift-files: {why}");
            }
        });
        receiver
    })
}

/// What the socket answers a state query with.
fn kept() -> String {
    STATE
        .lock()
        .map_or_else(|_| "the app is busy\n".to_string(), |state| state.clone())
}

/// Keep what `--state` prints.
fn keep(state: &Files) {
    if let Ok(mut kept) = STATE.lock() {
        *kept = state.state();
    }
}

static STATE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Start a job, and keep it to follow.
pub fn start_job(state: &mut Files, id: Option<window::Id>, work: jobs::Work) -> Task<Message> {
    let number = state.jobs.iter().map(|job| job.number).max().unwrap_or(0) + 1;
    let job = Job::new(number, work, id);
    let task = jobs::start(&job, state.offset);
    state.jobs.push(job);
    // the ones that ended long ago go, except the last few, which Undo may still want
    let ended: Vec<u64> = state
        .jobs
        .iter()
        .filter(|job| !job.running())
        .map(|job| job.number)
        .collect();
    if ended.len() > 8 {
        let keep_from = ended[ended.len() - 8];
        state
            .jobs
            .retain(|job| job.running() || job.number >= keep_from);
    }
    task
}
