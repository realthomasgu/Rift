//! The shell: the top bar along the top of the screen, the dock along the bottom, and the menus,
//! notifications and the key popup that come and go over them. One process with a layer surface per
//! part, drawn with iced on the software renderer and placed by the layer-shell protocol, so it
//! works on any machine the drive meets.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use iced::widget::container;
use iced::{
    Element, Font, Length, Subscription, Task, Theme, event, font, keyboard, mouse, theme, window,
};
use iced_layershell::actions::{LayerShellCustomAction, LayerShellCustomActionWithId};
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};
use iced_layershell::settings::{LayerShellSettings, Settings};
use librift::appearance;
use librift::battery::Battery;
use librift::dock::Edge;
use librift::drives;
use librift::files::places::{self, Place};
use librift::notifications::{self, Sender};
use librift::os::{self, Action};
use librift::sound::{self, Side, Volume};
use librift::{bluetooth, network, quasar, session};

use crate::access;
use crate::answer;
use crate::banner;
use crate::bar;
use crate::calendar::{Day, Month, Weekday};
use crate::clock;
use crate::control::{self, Command, Level, Recording};
use crate::datemenu;
use crate::dialog::{self, Ask, Dialog};
use crate::dock::{self, Dock};
use crate::find;
use crate::horizon::{self, Open};
use crate::launcher::{self, App};
use crate::menu::{self, Menu, Results};
use crate::notice::{self, Effect, Fitted, Notices, Notification, Outbox};
use crate::nu;
use crate::popup::{self, Popup};
use crate::restore::{self, Restore};
use crate::route::{self, Interpretation};
use crate::status::{self, Status};
use crate::system;
use crate::talk;
use crate::theme::Palette;
use crate::watch::{self, Latest};

/// What the line under the field says while the shell is listening. It names the key, since the
/// same key is what stops it and the field is the only place that teaches what the field does.
const LISTENING: &str = "Listening. Press Super and H again to stop.";

/// The interface font.
pub const FONT: Font = Font {
    family: font::Family::Name("Noto Sans"),
    ..Font::DEFAULT
};
/// What a command printed is terminal output, and a table only lines up in a fixed width.
pub const MONO: Font = Font {
    family: font::Family::Name("DejaVu Sans Mono"),
    ..Font::DEFAULT
};
/// The name over a group of rows, the same size as the rows and in bold, the way a settings page
/// heads a section.
pub const HEADING: Font = Font {
    weight: font::Weight::Bold,
    ..FONT
};

/// What `lens --state` prints. The shell writes it after every message and the thread that
/// answers the socket reads it, so a query never waits for the one that draws.
fn kept() -> &'static Mutex<String> {
    static KEPT: OnceLock<Mutex<String>> = OnceLock::new();
    KEPT.get_or_init(|| Mutex::new(String::new()))
}

/// How much bigger than its own sizes the shell draws itself, in per cent, from the owner's
/// interface text size. Every surface is asked for at this much of its size in the pixels the
/// compositor places it in, and iced is told the same factor, so the bar, the dock and the menus
/// grow with the text in the apps and everything inside them keeps the numbers it is written with.
/// It is one number for the whole process because the step that turns a message into a layer-shell
/// action has no state to read.
static SCALE: AtomicU32 = AtomicU32::new(appearance::TEXT_DEFAULT);

/// A size of the shell's own, in the pixels the compositor places surfaces in.
fn scaled(size: u32) -> u32 {
    (size.saturating_mul(SCALE.load(Ordering::Relaxed)) + 50) / 100
}

/// The same for a margin, which the protocol takes as a whole number that may be negative.
fn margin(size: u32) -> i32 {
    i32::try_from(scaled(size)).unwrap_or(0)
}

/// What iced draws a surface at, so a bar asked for at half again its height holds the same rows
/// half again as big.
fn factor() -> f32 {
    f32::from(u16::try_from(SCALE.load(Ordering::Relaxed)).unwrap_or(100)) / 100.0
}

/// How long after the compositor closed a menu a press of that menu's own button is the click that
/// closed it, and not a click to open it again.
const REOPEN: Duration = Duration::from_millis(400);

/// How long a network may take to come up after it was picked.
const JOIN_WAIT: Duration = Duration::from_secs(45);

/// The icon of the notification about the windows that did not come back.
const PASSED_OVER: &str = "focus-windows-symbolic";

/// The icon of the notification a Ghost login puts up. The drive is locked, which is what the
/// session is, and it is the one the Owner page uses for the same thing.
const GHOST_ICON: &str = "channel-secure-symbolic";

/// How long after what is open changed the journal of it is written. A session being taken down
/// closes its windows and stops the shell in the same breath, so a shell that is already gone writes
/// no journal of an empty desktop over the one the next login brings back. It also holds the file
/// still while a window is being dragged from one column to another.
const JOURNAL_AFTER: Duration = Duration::from_secs(2);

/// Where the dock's surface stands and how much of the screen it keeps, in the pixels the
/// compositor places surfaces in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stand {
    anchor: Anchor,
    size: (u32, u32),
    margin: (i32, i32, i32, i32),
    zone: i32,
}

/// The shell's state. The bar and the dock are always there; a menu, a dialog, a notification or the
/// key popup comes and goes with its surface.
struct Lens {
    /// Dark or light, from the owner's setting.
    theme: appearance::Theme,
    /// Which of GNOME's nine the accent is, from the owner's setting.
    accent: appearance::Accent,
    look: Palette,
    apps: Vec<App>,
    /// Home and the folders of it that are there, then the drive's own exchange partition, which
    /// the Applications menu lists over the apps.
    places: Vec<Place>,
    clock: String,
    /// Today, for the calendar.
    today: Option<Day>,
    /// The day the locale starts its weeks on.
    first: Weekday,
    status: Status,
    menu: Option<Menu>,
    system: Option<system::Menu>,
    /// The clock menu.
    datemenu: Option<datemenu::Menu>,
    dialog: Option<Dialog>,
    dock: Dock,
    /// Where the dock's surface was last asked to stand.
    placed: Stand,
    /// The bar's own surface, once the compositor has opened it. It is the one surface the shell
    /// does not open itself, so it has no id until then.
    bar: Option<window::Id>,
    /// How wide the screen is in the shell's own pixels, which is how wide the bar is.
    screen: u32,
    /// The keyboard layouts there are, for the short name the bar gives the one in use.
    keymaps: Vec<librift::keyboard::Layout>,
    /// The short name of the layout in use, while there are two or more.
    layout: Option<String>,
    /// The notifications on screen and the ones kept.
    notices: Notices,
    /// The apps that have sent a notification, which the Notifications page lists.
    senders: Vec<Sender>,
    /// Where the signals about them go out on the bus, once the server has the name.
    outbox: Option<Outbox>,
    /// The key popup, while it is up.
    popup: Option<Popup>,
    /// The file the screen recorder is writing, while it is running. The bar is marked for it.
    recording: Option<String>,
    /// The recording push to talk is making, while the shell is listening.
    listening: Option<talk::Recording>,
    /// Counts the times the shell has started listening, so only the last one's minute stops it.
    ears: u64,
    /// The last answer the shell read out loud, which `lens --state` prints.
    said: Option<String>,
    /// Counts the keys the popup showed, so only the last one's second closes it.
    keys: u64,
    /// The menu the compositor closed by taking the keyboard away, and when.
    dismissed: Option<(Closed, Instant)>,
    /// Sets the volume the slider asks for, on a thread of its own.
    volume: Latest<u8>,
    /// Sets the brightness the slider asks for.
    brightness: Latest<u8>,
    /// The windows and the workspaces the session journal was last written from. A change the
    /// journal does not keep, like a window growing, matches these and writes nothing, which is
    /// most of what the compositor has to say while a window is being resized.
    seen: Vec<session::Seen>,
    spaces: Vec<session::Space>,
    /// Counts the changes the journal is waiting to be written for, so only the last one writes it.
    journal: u64,
    /// Bringing the last session back, while it is going on.
    restore: Option<Restore>,
    /// What became of the session the last login left, which `lens --state` prints.
    back: Back,
}

/// What became of the session the last login left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Back {
    /// The owner has turned bringing it back off.
    Off,
    /// There was nothing to bring back, or this login has had its session back already.
    Nothing,
    /// It came back: this many windows, and this many the journal named that nothing here opens.
    Windows(usize, usize),
}

/// Which of the bar's menus closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Closed {
    Applications,
    Clock,
    System,
}

/// What happens to the shell.
#[derive(Debug, Clone)]
pub enum Message {
    /// The minute turned, and this is what the clock says now.
    Tick(clock::Now),
    /// What `NetworkManager` says now, or why it did not answer. It is the biggest thing a
    /// message carries, so it travels behind a pointer.
    Network(Box<Result<network::Picture, String>>),
    /// What `UPower` says about the battery now.
    Battery(Option<Battery>),
    /// What `BlueZ` says now.
    Bluetooth(Option<bluetooth::Picture>),
    /// The default sink's volume now.
    Sound(Option<Volume>),
    /// The backlight now.
    Brightness(Option<u8>),
    /// The Applications button, or Mod+Space.
    ToggleMenu,
    /// The clock in the bar.
    ToggleClock,
    /// A button or the switch of the clock menu.
    Clock(datemenu::Event),
    /// An app sent a notification.
    Notified(Notification),
    /// An app closed the notification it sent.
    Recalled(u32),
    /// The notification server has the name, and takes the signals the shell sends through this.
    Outbox(Outbox),
    /// A notification on screen was pressed, closed, or the pointer came or went.
    Banner(banner::Event),
    /// A notification's time on screen ran out, for this run of it.
    Expire(u32, u64),
    /// The key popup's second is over, for this key.
    PopupDone(u64),
    /// The status icons in the bar.
    ToggleSystem,
    /// A row, a switch or a slider of the system menu.
    System(system::Event),
    /// Something the system menu asked for finished, or what went wrong.
    Acted(Result<(), String>),
    /// A dialog's field or buttons.
    Dialog(dialog::Event),
    /// What a dialog asked for finished, or what went wrong. The id is the dialog's.
    DialogDone(window::Id, Result<(), String>),
    /// Enter, which a dialog with no field takes as its default button.
    Enter,
    /// New words in the field.
    Input(String),
    /// Enter in the field.
    Submit,
    /// Up or down the list.
    Move(isize),
    /// A click on a row of the app list: that app starts.
    Pick(usize),
    /// A click on a place over the apps: it opens in the file manager.
    PickPlace(usize),
    /// The typing in the field has stopped for these words: home is looked through for them.
    Search(String),
    /// What a search of home came back with, and the words it was made for.
    Found(String, Result<Vec<find::File>, String>),
    /// A click on a file a search found: it opens with the app its kind opens with.
    PickFile(usize),
    /// Escape: clear the field, or close the menu when it is already empty.
    Escape,
    /// Close the menu, whatever surface it is on.
    Dismiss,
    /// A command or a pipeline finished.
    Done(Result<String, String>),
    /// Quasar answered.
    Answered(Result<(String, String), String>),
    /// A line came in on the socket.
    Typed(Command),
    /// The minute the shell listens for at most is over, for this run of it.
    Deaf(u64),
    /// A recording became words, or nothing at all when nobody spoke into it.
    Heard(Result<Option<String>, String>),
    /// The answer to a spoken question went to the speakers, or would not play.
    Said(Result<String, String>),
    /// Horizon opened, closed or focused something.
    Windows(Open),
    /// Write the session down, when nothing has changed since this write was asked for.
    Journal(u64),
    /// The patience for the window of this start of an app is over.
    Restoring(u64),
    /// A click on a dock item: its app starts, or its window comes forward.
    Dock(String),
    /// A middle click on one: another window of that app.
    DockNew(String),
    /// A right click on one: the menu of what can be done with it.
    DockMenu(String),
    /// The pointer came onto a surface, or went off it.
    Pointer(window::Id, bool),
    /// The wait with this number after the pointer went off the dock is over.
    HideDock(u64),
    /// A row of that menu.
    DockRow(dock::Row),
    /// A click on a workspace button.
    Space(u8),
    /// Something went into the owner's trash, or the last thing came out of it.
    Trashed(bool),
    /// What udisks says the disks are now. It travels behind a pointer, being the biggest thing
    /// after the network.
    Drives(Vec<drives::Volume>),
    /// A click on the name of the keyboard layout in the bar: the next one.
    NextLayout,
    /// Open the dock's surface, standing here.
    OpenDock(window::Id, Stand),
    /// Stand the dock on these edges at this size.
    Shape(window::Id, Anchor, (u32, u32)),
    /// Stand the dock this far off its edges.
    Margins(window::Id, (i32, i32, i32, i32)),
    /// Keep this much of the screen for the dock.
    Zone(window::Id, i32),
    /// Open the menu of a dock item, this tall, with its left edge here, on the dock's edge, this
    /// far off the edge of the working area.
    OpenItemMenu(window::Id, u32, i32, Edge, i32),
    /// Open the menu's surface, this tall and this far from the top of the working area.
    Open(window::Id, u32, i32),
    /// Open the system menu's surface, this tall and this far from the top of the working area.
    OpenSystem(window::Id, u32, i32),
    /// Open a dialog's surface, this tall.
    OpenDialog(window::Id, u32),
    /// Open the clock menu's surface, this tall and this far from the top of the working area.
    OpenClock(window::Id, u32, i32),
    /// Open a notification's surface, this tall and this far under the bar.
    OpenBanner(window::Id, u32, u32),
    /// Open the key popup's surface.
    OpenPopup(window::Id),
    /// Put a notification's surface this far under the bar.
    Place(window::Id, u32),
    /// A menu's surface has to grow or shrink to this width and height.
    Resize(window::Id, u32, u32),
    /// A bar has to keep this much of the screen for itself.
    Reserve(window::Id, u32),
    /// A surface opened, this wide in the shell's own pixels.
    Opened(window::Id, f32),
    /// A surface is this wide now, in the shell's own pixels.
    Sized(window::Id, f32),
    /// Close a surface.
    Close(window::Id),
    /// A surface took or lost the keyboard.
    Focus(window::Id, bool),
}

// the layer-shell runtime asks every message whether it is one of its own actions. the ones that
// are carry a surface: opening a menu, a dialog or the dock, resizing a menu as it grows, and
// closing one. an action that makes a new surface must not name it as the target, or the runtime
// waits for a surface that does not exist yet
impl TryFrom<Message> for LayerShellCustomActionWithId {
    type Error = Message;

    fn try_from(message: Message) -> Result<Self, Message> {
        match message {
            Message::Open(id, height, top) => Ok(Self::new(
                None,
                LayerShellCustomAction::NewLayerShell {
                    settings: menu_surface(height, top),
                    id,
                },
            )),
            Message::OpenDock(id, stand) => Ok(Self::new(
                None,
                LayerShellCustomAction::NewLayerShell {
                    settings: dock_surface(stand),
                    id,
                },
            )),
            Message::Shape(id, anchor, size) => Ok(Self::new(
                Some(id),
                LayerShellCustomAction::AnchorSizeChange(anchor, size),
            )),
            Message::Margins(id, margins) => Ok(Self::new(
                Some(id),
                LayerShellCustomAction::MarginChange(margins),
            )),
            Message::Zone(id, zone) => Ok(Self::new(
                Some(id),
                LayerShellCustomAction::ExclusiveZoneChange(zone),
            )),
            Message::OpenItemMenu(id, height, left, edge, above) => Ok(Self::new(
                None,
                LayerShellCustomAction::NewLayerShell {
                    settings: item_menu_surface(height, left, edge, above),
                    id,
                },
            )),
            Message::OpenSystem(id, height, top) => Ok(Self::new(
                None,
                LayerShellCustomAction::NewLayerShell {
                    settings: system_surface(height, top),
                    id,
                },
            )),
            Message::OpenDialog(id, height) => Ok(Self::new(
                None,
                LayerShellCustomAction::NewLayerShell {
                    settings: dialog_surface(height),
                    id,
                },
            )),
            Message::OpenClock(id, height, top) => Ok(Self::new(
                None,
                LayerShellCustomAction::NewLayerShell {
                    settings: clock_surface(height, top),
                    id,
                },
            )),
            Message::OpenBanner(id, height, top) => Ok(Self::new(
                None,
                LayerShellCustomAction::NewLayerShell {
                    settings: banner_surface(height, top),
                    id,
                },
            )),
            Message::OpenPopup(id) => Ok(Self::new(
                None,
                LayerShellCustomAction::NewLayerShell {
                    settings: popup_surface(),
                    id,
                },
            )),
            Message::Place(id, top) => Ok(Self::new(
                Some(id),
                LayerShellCustomAction::MarginChange(banner_margin(top)),
            )),
            Message::Resize(id, width, height) => Ok(Self::new(
                Some(id),
                LayerShellCustomAction::SizeChange((scaled(width), scaled(height))),
            )),
            Message::Reserve(id, height) => Ok(Self::new(
                Some(id),
                LayerShellCustomAction::ExclusiveZoneChange(margin(height)),
            )),
            Message::Close(id) => Ok(Self::new(Some(id), LayerShellCustomAction::RemoveWindow)),
            other => Err(other),
        }
    }
}

/// How far a menu of the bar stands from the top of the working area. A menu keeps nothing of the
/// screen, so it is placed in what the bar, the dock and the on-screen keyboard leave, and never
/// reaches over the keyboard or behind a dock along the bottom. With the dock along the top the
/// working area starts under the dock, so the menu goes up by the dock's height and its gap: it
/// hangs from the button that opened it and stands over the dock, the way a menu does.
fn menu_top(dock: &Dock) -> i32 {
    if dock.options.edge != Edge::Top {
        return 0;
    }
    let gap = if dock.options.extend {
        0
    } else {
        margin(dock::OFF_EDGE)
    };
    -(margin(dock.height()) + gap)
}

/// The menu's surface: on the overlay layer, hanging under the bar inside the working area, its
/// left edge under the Applications button. It takes the keyboard on demand, which the compositor
/// gives it as it appears and takes away as soon as anything else is clicked.
fn menu_surface(height: u32, top: i32) -> NewLayerShellSettings {
    NewLayerShellSettings {
        size: Some((scaled(menu::WIDTH), scaled(height))),
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Left,
        exclusive_zone: Some(0),
        margin: Some((top, 0, 0, margin(menu::PAD))),
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        output_option: OutputOption::Active,
        events_transparent: false,
        namespace: Some("lens-menu".to_string()),
    }
}

/// Where the dock stands: along its edge from one side of the screen to the other, or only as wide
/// as what it holds in the middle of its edge and a gap off it. Either way it keeps its height of
/// the screen, so a window stands clear of it and nothing is ever hidden behind it; the compositor
/// adds the gap to what it keeps. A dock that hides keeps nothing: the windows have the room, and
/// the dock comes out over them. Hidden, it is a line along the bottom edge, as wide as it is.
fn dock_place(dock: &Dock) -> Stand {
    let edge = match dock.options.edge {
        Edge::Bottom => Anchor::Bottom,
        Edge::Top => Anchor::Top,
    };
    let hides = dock.options.hides();
    let zone = if hides { 0 } else { margin(dock.height()) };
    if hides && dock.hidden {
        let (anchor, width) = if dock.options.extend {
            (Anchor::Bottom | Anchor::Left | Anchor::Right, 0)
        } else {
            (Anchor::Bottom, scaled(dock.width()))
        };
        return Stand {
            anchor,
            size: (width, scaled(dock::HIDDEN)),
            margin: (0, 0, 0, 0),
            zone,
        };
    }
    if dock.options.extend {
        return Stand {
            anchor: edge | Anchor::Left | Anchor::Right,
            size: (0, scaled(dock.height())),
            margin: (0, 0, 0, 0),
            zone,
        };
    }
    let gap = margin(dock::OFF_EDGE);
    Stand {
        anchor: edge,
        size: (scaled(dock.width()), scaled(dock.height())),
        margin: match dock.options.edge {
            Edge::Bottom => (0, 0, gap, 0),
            Edge::Top => (gap, 0, 0, 0),
        },
        zone,
    }
}

/// The dock's surface, standing where `dock_place` says. A bar never takes the keyboard.
fn dock_surface(stand: Stand) -> NewLayerShellSettings {
    NewLayerShellSettings {
        size: Some(stand.size),
        layer: Layer::Top,
        anchor: stand.anchor,
        exclusive_zone: Some(stand.zone),
        margin: Some(stand.margin),
        keyboard_interactivity: KeyboardInteractivity::None,
        output_option: OutputOption::Active,
        events_transparent: false,
        namespace: Some("lens-dock".to_string()),
    }
}

/// The menu a right click on a dock item opens: standing on the dock, or hanging from it when the
/// dock is along the top, its left edge where the item is. A surface that reserves nothing is
/// placed inside the working area, so the dock's own height is already taken off and the margin
/// towards it is nothing, `above` being the height of a dock that hides and so keeps nothing. It
/// takes the keyboard the same way the Applications menu does, so a click anywhere else closes it.
fn item_menu_surface(height: u32, left: i32, edge: Edge, above: i32) -> NewLayerShellSettings {
    NewLayerShellSettings {
        size: Some((scaled(dock::MENU_WIDTH), scaled(height))),
        layer: Layer::Overlay,
        anchor: match edge {
            Edge::Bottom => Anchor::Bottom,
            Edge::Top => Anchor::Top,
        } | Anchor::Left,
        exclusive_zone: Some(0),
        margin: Some((0, 0, above, margin(u32::try_from(left).unwrap_or(0)))),
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        output_option: OutputOption::Active,
        events_transparent: false,
        namespace: Some("lens-menu".to_string()),
    }
}

/// The system menu's surface: on the overlay layer, hanging under the bar inside the working area,
/// its right edge under the status icons. It takes the keyboard the way the Applications menu
/// does, so a click anywhere else closes it.
fn system_surface(height: u32, top: i32) -> NewLayerShellSettings {
    NewLayerShellSettings {
        size: Some((scaled(system::WIDTH), scaled(height))),
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Right,
        exclusive_zone: Some(0),
        margin: Some((top, margin(system::PAD), 0, 0)),
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        output_option: OutputOption::Active,
        events_transparent: false,
        namespace: Some("lens-menu".to_string()),
    }
}

/// A dialog's surface: on the overlay layer in the middle of the screen, anchored to no edge. It
/// holds the keyboard until it is answered, so a click on a window does not throw away what was
/// typed into it.
fn dialog_surface(height: u32) -> NewLayerShellSettings {
    NewLayerShellSettings {
        size: Some((scaled(dialog::WIDTH), scaled(height))),
        layer: Layer::Overlay,
        anchor: Anchor::empty(),
        exclusive_zone: Some(0),
        margin: None,
        keyboard_interactivity: KeyboardInteractivity::Exclusive,
        output_option: OutputOption::Active,
        events_transparent: false,
        namespace: Some("lens-dialog".to_string()),
    }
}

/// The clock menu's surface: on the overlay layer, hanging under the bar inside the working area
/// and anchored to no side, so it is in the middle of the screen under the clock. It takes the
/// keyboard the way the other menus do, so a click anywhere else closes it.
fn clock_surface(height: u32, top: i32) -> NewLayerShellSettings {
    NewLayerShellSettings {
        size: Some((scaled(datemenu::WIDTH), scaled(height))),
        layer: Layer::Overlay,
        anchor: Anchor::Top,
        exclusive_zone: Some(0),
        margin: Some((top, 0, 0, 0)),
        keyboard_interactivity: KeyboardInteractivity::OnDemand,
        output_option: OutputOption::Active,
        events_transparent: false,
        namespace: Some("lens-menu".to_string()),
    }
}

/// A notification's surface: on the overlay layer at the top right of the working area, this far
/// under the bar. It never takes the keyboard, so it does not take it away from a window.
fn banner_surface(height: u32, top: u32) -> NewLayerShellSettings {
    NewLayerShellSettings {
        size: Some((scaled(banner::WIDTH), scaled(height))),
        layer: Layer::Overlay,
        anchor: Anchor::Top | Anchor::Right,
        exclusive_zone: Some(0),
        margin: Some(banner_margin(top)),
        keyboard_interactivity: KeyboardInteractivity::None,
        output_option: OutputOption::Active,
        events_transparent: false,
        namespace: Some("lens-notify".to_string()),
    }
}

/// The margins of a notification this far under the bar: top, right, bottom and left.
fn banner_margin(top: u32) -> (i32, i32, i32, i32) {
    (margin(top), margin(notice::GAP), 0, 0)
}

/// The key popup's surface: on the overlay layer, anchored to the bottom alone so it is in the
/// middle, standing above the dock. Clicks go through it to whatever is under it.
fn popup_surface() -> NewLayerShellSettings {
    NewLayerShellSettings {
        size: Some((scaled(popup::WIDTH), scaled(popup::HEIGHT))),
        layer: Layer::Overlay,
        anchor: Anchor::Bottom,
        exclusive_zone: Some(0),
        margin: Some((0, 0, margin(popup::ABOVE), 0)),
        keyboard_interactivity: KeyboardInteractivity::None,
        output_option: OutputOption::Active,
        events_transparent: true,
        namespace: Some("lens-popup".to_string()),
    }
}

/// Open the shell on the session's Wayland display and run until it is closed.
///
/// # Errors
///
/// When there is no display or the compositor has no layer-shell.
pub fn run(apps: Vec<App>) -> Result<(), iced_layershell::Error> {
    let chosen = appearance::Theme::read();
    // the interface text size before the first surface is asked for, since the bar is asked for at
    // its size as the daemon starts
    SCALE.store(appearance::text(), Ordering::Relaxed);
    // apps and the compositor follow the same setting. dconf may have to be started on the bus
    // first, which the bar does not wait for
    thread::spawn(move || {
        if let Err(why) = appearance::apply(chosen) {
            eprintln!("lens: {why}");
        }
        // and how big each screen is drawn, which Orbit keeps in the host profile. a machine with
        // no answer from Orbit is drawn the way the compositor works it out for itself
        if let Err(why) = librift::orbit::follow() {
            eprintln!("lens: the screens: {why}");
        }
        // and the mouse and the touchpad, written the way this image writes them
        if let Err(why) = librift::pointer::apply() {
            eprintln!("lens: the mouse and the touchpad: {why}");
        }
    });
    iced_layershell::daemon(move || boot(chosen, apps.clone()), "lens", update, view)
        .theme(|state: &Lens, _| Theme::custom("Rift", palette(state.look)))
        .scale_factor(|_: &Lens, _| factor())
        .style(|state: &Lens, _: &Theme| theme::Style {
            // every surface paints its own background over all of itself; this is what shows if
            // one ever does not, and a software-rendered surface has no transparency
            background_color: state.look.bar,
            text_color: state.look.text,
        })
        .subscription(subscription)
        .settings(Settings {
            id: Some("dev.rift.Lens".to_string()),
            default_font: FONT,
            default_text_size: bar::TEXT_SIZE.into(),
            layer_settings: LayerShellSettings {
                anchor: Anchor::Top | Anchor::Left | Anchor::Right,
                layer: Layer::Top,
                exclusive_zone: margin(bar::HEIGHT),
                size: Some((0, scaled(bar::HEIGHT))),
                // a bar never takes the keyboard away from a window
                keyboard_interactivity: KeyboardInteractivity::None,
                ..LayerShellSettings::default()
            },
            ..Settings::default()
        })
        .run()
}

fn palette(look: Palette) -> theme::Palette {
    theme::Palette {
        background: look.bar,
        text: look.text,
        primary: look.accent,
        success: look.ok,
        warning: look.warn,
        danger: look.error,
    }
}

fn boot(chosen: appearance::Theme, apps: Vec<App>) -> (Lens, Task<Message>) {
    // the dock is made here, not when something opens it: it is a part of the shell like the bar,
    // and it takes its own height from the screen before the first window is placed
    let dock = Dock::new(window::Id::unique(), &apps);
    let placed = dock_place(&dock);
    let opening = Task::done(Message::OpenDock(dock.id, placed));
    let now = clock::now();
    let accent = appearance::Accent::read();
    // the session the last login left, read before the first picture of what is open writes the
    // journal over it
    let restore = restore::begin(&apps);
    let back = if restore.is_some() || session::restores() {
        Back::Nothing
    } else {
        Back::Off
    };
    let state = Lens {
        theme: chosen,
        accent,
        look: crate::theme::palette(chosen, accent),
        apps,
        places: Vec::new(),
        clock: now.line,
        today: now.today,
        first: clock::first_weekday(),
        status: Status::default(),
        menu: None,
        system: None,
        datemenu: None,
        dialog: None,
        dock,
        placed,
        bar: None,
        screen: 0,
        keymaps: librift::keyboard::installed(),
        layout: None,
        notices: Notices::new(notifications::quiet(), notifications::quiet_apps()),
        senders: notifications::senders(),
        outbox: None,
        popup: None,
        recording: access::running(access::Tool::Recorder).and_then(|(_, file)| file),
        listening: None,
        ears: 0,
        said: None,
        keys: 0,
        dismissed: None,
        volume: Latest::new(|level| report(sound::set_volume(Side::Output, level))),
        brightness: Latest::new(|level| report(status::set_brightness(level))),
        seen: Vec::new(),
        spaces: Vec::new(),
        journal: 0,
        restore,
        back,
    };
    remember(&state);
    (state, opening)
}

/// What a Ghost login says, once, when the bar it says it in is on screen: the name of the mode over
/// the sentence that says what it means. The two words in the bar name the mode and do not say what
/// it is, and the person who most needs the sentence is the one who has not met the mode before.
///
/// It is critical, which in this shell is the only urgency that stays on screen until it is closed
/// and the only one Do not disturb does not hide: what a session is cannot be a banner that went by
/// while somebody was looking elsewhere. It draws like every other notification.
fn ghost_notice() -> Task<Message> {
    Task::done(Message::Notified(Notification {
        id: u32::MAX - 2,
        app: "Lens".to_string(),
        icon: Some(GHOST_ICON.to_string()),
        entry: None,
        summary: librift::ghost::NAME.to_string(),
        body: librift::ghost::SENTENCE.to_string(),
        actions: Vec::new(),
        default: false,
        urgency: notice::Urgency::Critical,
        transient: false,
    }))
}

fn subscription(_: &Lens) -> Subscription<Message> {
    Subscription::batch([
        keys(),
        focus(),
        terminal(),
        ticker(),
        windows(),
        watch::network(),
        watch::battery(),
        watch::bluetooth(),
        watch::sound(),
        watch::zone(),
        watch::drives(),
        watch::trash(),
        notice::serve(),
    ])
}

// the field takes the printable keys for itself, so these come from every event, not only the
// ones no widget wanted
fn keys() -> Subscription<Message> {
    event::listen_with(|event, _, _| match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => match key {
            keyboard::Key::Named(keyboard::key::Named::Escape) => Some(Message::Escape),
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => Some(Message::Move(1)),
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => Some(Message::Move(-1)),
            keyboard::Key::Named(keyboard::key::Named::Enter) => Some(Message::Enter),
            _ => None,
        },
        _ => None,
    })
}

// the compositor gives an on-demand surface the keyboard as it appears and takes it back when
// something else is clicked, which is how a click outside closes the menu. the surface is drawn
// before the keyboard reaches it, so the cursor goes in the field on either event
fn focus() -> Subscription<Message> {
    event::listen_with(|event, _, id| match event {
        iced::Event::Window(window::Event::Opened { size, .. }) => {
            Some(Message::Opened(id, size.width))
        }
        iced::Event::Window(window::Event::Resized(size)) => Some(Message::Sized(id, size.width)),
        iced::Event::Window(window::Event::Focused) => Some(Message::Focus(id, true)),
        iced::Event::Window(window::Event::Unfocused) => Some(Message::Focus(id, false)),
        // the pointer on the dock or off it, which a dock that hides follows
        iced::Event::Mouse(mouse::Event::CursorEntered) => Some(Message::Pointer(id, true)),
        iced::Event::Mouse(mouse::Event::CursorLeft) => Some(Message::Pointer(id, false)),
        _ => None,
    })
}

// the socket in the runtime directory, read on a thread of its own. the state query is answered
// there, from the lines the shell keeps up to date.
//
// every one of these is named: iced tells two subscriptions apart by the type of the stream they
// make and the address of the function that makes it, and every one of ours makes the same kind of
// stream, so two of them whose code the optimiser folds together would be one subscription and the
// second would never be polled. the name is what tells them apart
fn terminal() -> Subscription<Message> {
    Subscription::run_with("terminal", |_| {
        let (sender, receiver) = iced::futures::channel::mpsc::unbounded();
        std::thread::spawn(move || {
            if let Err(why) = control::serve(|command| {
                if command == Command::State {
                    return Some(kept().lock().map_or_else(
                        |_| "the shell is busy".to_string(),
                        |lines| lines.clone() + &access::lines(),
                    ));
                }
                let _ = sender.unbounded_send(Message::Typed(command));
                None
            }) {
                eprintln!("lens: {why}");
            }
        });
        receiver
    })
}

// the clock, once a minute on the minute, read on a thread of its own because it runs a child
// process. the volume is read in the same tick: pw-mon does not say when the default sink becomes
// another one, and a minute is soon enough for that
fn ticker() -> Subscription<Message> {
    Subscription::run_with("ticker", |_| {
        let (sender, receiver) = iced::futures::channel::mpsc::unbounded();
        std::thread::spawn(move || {
            loop {
                if sender.unbounded_send(Message::Tick(clock::now())).is_err() {
                    return;
                }
                if sender
                    .unbounded_send(Message::Sound(sound::volume(Side::Output)))
                    .is_err()
                {
                    return;
                }
                std::thread::sleep(librift::time::until_next_minute());
            }
        });
        receiver
    })
}

// horizon's windows and workspaces, read on a thread of its own because the stream blocks until
// the compositor has something to say. every event the dock draws from turns into one message
fn windows() -> Subscription<Message> {
    Subscription::run_with("windows", |_| {
        let (sender, receiver) = iced::futures::channel::mpsc::unbounded();
        std::thread::spawn(move || {
            horizon::watch(|open| {
                sender
                    .unbounded_send(Message::Windows(open.clone()))
                    .is_ok()
            });
        });
        receiver
    })
}

fn update(state: &mut Lens, message: Message) -> Task<Message> {
    let task = match message {
        Message::Tick(_)
        | Message::Network(_)
        | Message::Battery(_)
        | Message::Bluetooth(_)
        | Message::Sound(_)
        | Message::Brightness(_) => {
            heard(state, message);
            Task::none()
        }
        Message::ToggleMenu => toggle(state),
        Message::ToggleClock
        | Message::Clock(_)
        | Message::Notified(_)
        | Message::Recalled(_)
        | Message::Outbox(_)
        | Message::Banner(_)
        | Message::Expire(..)
        | Message::PopupDone(_) => notices(state, message),
        Message::ToggleSystem => toggle_system(state),
        Message::System(event) => system_event(state, event),
        Message::Acted(result) => {
            match state.system.as_mut() {
                Some(menu) => {
                    menu.notice = None;
                    menu.error = result.err();
                }
                None => report(result),
            }
            Task::none()
        }
        Message::Dialog(event) => dialog_event(state, event),
        Message::DialogDone(id, result) => dialog_done(state, id, result),
        Message::Enter => match state.dialog.as_ref().map(|dialog| &dialog.ask) {
            Some(Ask::Command(_)) => dialog_event(state, dialog::Event::Confirm),
            _ => Task::none(),
        },
        Message::Input(_)
        | Message::Submit
        | Message::Move(_)
        | Message::Pick(_)
        | Message::PickPlace(_)
        | Message::Search(_)
        | Message::Found(..)
        | Message::PickFile(_) => field(state, message),
        Message::Trashed(anything) => trashed(state, anything),
        Message::Drives(found) => plugged(state, found),
        Message::Escape => escape(state),
        Message::Dismiss => close(state),
        Message::Done(result) => {
            if let Some(menu) = state.menu.as_mut() {
                finish(menu, result);
            }
            Task::none()
        }
        Message::Answered(result) => state
            .menu
            .as_mut()
            .map_or_else(Task::none, |menu| answered(menu, result)),
        Message::Typed(command) => typed(state, command),
        Message::Deaf(ears) => deaf(state, ears),
        Message::Heard(result) => spoken(state, result),
        Message::Said(result) => said(state, result),
        Message::Windows(_)
        | Message::Journal(_)
        | Message::Restoring(_)
        | Message::NextLayout
        | Message::Dock(_)
        | Message::DockNew(_)
        | Message::DockMenu(_)
        | Message::DockRow(_)
        | Message::Space(_) => horizon_said(state, message),
        Message::Focus(..) | Message::Opened(..) | Message::Sized(..) => surface(state, &message),
        Message::Pointer(id, over) => pointed(state, id, over),
        Message::HideDock(number) => {
            state.dock.waited(number);
            Task::none()
        }
        // the runtime takes these before update ever sees them
        Message::Open(..)
        | Message::OpenDock(..)
        | Message::Shape(..)
        | Message::Margins(..)
        | Message::Zone(..)
        | Message::OpenItemMenu(..)
        | Message::OpenSystem(..)
        | Message::OpenDialog(..)
        | Message::OpenClock(..)
        | Message::OpenBanner(..)
        | Message::OpenPopup(_)
        | Message::Place(..)
        | Message::Resize(..)
        | Message::Reserve(..)
        | Message::Close(..) => Task::none(),
    };
    let grow = resize(state);
    let stand = place_dock(state);
    remember(state);
    Task::batch([task, grow, stand])
}

/// Stand the dock where its settings say when that is not where it stands: a new edge, a new size
/// of icons or text, or the width of a dock only as wide as what it holds, which follows the apps.
/// The anchor and the size go in one request, because a surface as wide as nothing has to be
/// anchored to both sides in the same commit.
fn place_dock(state: &mut Lens) -> Task<Message> {
    let wanted = dock_place(&state.dock);
    let had = state.placed;
    if wanted == had {
        return Task::none();
    }
    state.placed = wanted;
    let id = state.dock.id;
    let mut work = Vec::new();
    if (wanted.anchor, wanted.size) != (had.anchor, had.size) {
        work.push(Task::done(Message::Shape(id, wanted.anchor, wanted.size)));
    }
    if wanted.margin != had.margin {
        work.push(Task::done(Message::Margins(id, wanted.margin)));
    }
    if wanted.zone != had.zone {
        work.push(Task::done(Message::Zone(id, wanted.zone)));
    }
    Task::batch(work)
}

/// The pointer came onto a surface or went off it. On the line a hidden dock leaves it brings the
/// dock out, and off the dock it starts the wait before a dock that hides goes again.
fn pointed(state: &mut Lens, id: window::Id, over: bool) -> Task<Message> {
    if id != state.dock.id {
        return Task::none();
    }
    state.dock.pointed(over).map_or_else(Task::none, |number| {
        later(dock::HIDE_AFTER, Message::HideDock(number))
    })
}

/// The short name of the keyboard layout in use, while there are two or more to switch between.
fn layout_name(state: &Lens) -> Option<String> {
    let open = &state.dock.open;
    if open.layouts.len() < 2 {
        return None;
    }
    librift::keyboard::short_names(&state.keymaps, &open.layouts)
        .into_iter()
        .nth(open.layout)
}

/// A surface opened, took or lost the keyboard, or changed its width.
fn surface(state: &mut Lens, message: &Message) -> Task<Message> {
    match *message {
        Message::Focus(id, has) => focused(state, id, has),
        // the bar is the one surface the shell does not open itself: the runtime makes it from the
        // settings and names it when it maps, which is before anything can open a menu. its id is
        // what a new interface text size is sent to
        Message::Opened(id, width) => {
            let bar = state.bar.is_none() && id != state.dock.id;
            if bar {
                state.bar = Some(id);
            }
            measured(state, id, width);
            let next = focused(state, id, true);
            // the mode is said once the bar that carries it is up, and not in boot(): the bar is the
            // one surface the shell does not open, so a surface opened before it maps would be taken
            // for the bar by the line above
            if bar && librift::ghost::on() {
                Task::batch([next, ghost_notice()])
            } else {
                next
            }
        }
        Message::Sized(id, width) => {
            measured(state, id, width);
            Task::none()
        }
        _ => Task::none(),
    }
}

/// A surface is this wide. The bar runs from one side of the screen to the other, so its width is
/// the screen's, which is where the menu of an item in a dock in the middle of its edge hangs from.
fn measured(state: &mut Lens, id: window::Id, width: f32) {
    if state.bar == Some(id) {
        // a width is a few thousand pixels at most, and never less than none
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let wide = width.max(0.0).round() as u32;
        state.screen = wide;
    }
}

/// What Horizon has open, and a press on the dock or on the layout in the bar, which goes back to
/// Horizon.
fn horizon_said(state: &mut Lens, message: Message) -> Task<Message> {
    match message {
        Message::Windows(open) => {
            let writing = write_down(state, &open);
            let coming = coming_back(state, &open);
            state.dock.changed(&state.apps, open);
            state.layout = layout_name(state);
            Task::batch([writing, coming])
        }
        Message::Journal(number) => {
            if number == state.journal {
                let windows =
                    session::of(&state.seen, &state.spaces, &state.apps, session::started_by);
                report(session::keep(&windows).map(|_| ()));
            }
            Task::none()
        }
        Message::Restoring(turn) => {
            let Some(restore) = state.restore.as_mut() else {
                return Task::none();
            };
            let next = restore::waited(restore, turn, &state.apps);
            went_on(state, next)
        }
        Message::NextLayout => {
            report(horizon::next_layout());
            Task::none()
        }
        Message::Dock(key) => dock_click(state, &key),
        Message::DockNew(key) => {
            new_window(state, &key);
            Task::none()
        }
        Message::DockMenu(key) => dock_menu(state, &key),
        Message::DockRow(row) => dock_row(state, &row),
        Message::Space(number) => {
            report(horizon::activate(number));
            Task::none()
        }
        _ => Task::none(),
    }
}

/// Write down what is open, the first half of teleport: the app that opened each window, the
/// workspace and the screen it is on, and where it stands in the layout. It is written whenever it
/// changes and nowhere else, since a drive is unplugged without warning and a journal written at
/// the end of a session would be a journal of nothing. A window growing or being drawn again says
/// nothing the journal keeps, so nothing is looked up and nothing is written for it.
///
/// A picture with no workspace in it at all is a compositor that is not there rather than a desktop
/// with nothing open, so it writes nothing: a session ending would otherwise write down a journal of
/// no windows over the one the next login has to bring back. The write itself comes a moment after
/// the change for the same reason. A session being taken down closes its windows and stops the shell
/// in the same breath, and a shell that is stopped writes nothing, so what the drive keeps is what
/// was open while the session was alive.
fn write_down(state: &mut Lens, open: &Open) -> Task<Message> {
    if open.all.is_empty() {
        return Task::none();
    }
    let seen: Vec<session::Seen> = open
        .windows
        .iter()
        .map(|win| session::Seen {
            app_id: win.app_id.clone(),
            title: win.title.clone(),
            pid: win.pid,
            space: win.space,
            floating: win.floating,
            place: win.place,
        })
        .collect();
    if seen == state.seen && open.all == state.spaces {
        return Task::none();
    }
    state.seen = seen;
    state.spaces.clone_from(&open.all);
    state.journal += 1;
    later(JOURNAL_AFTER, Message::Journal(state.journal))
}

/// Bringing the last session back, a window at a time: the window the last app opened is put where
/// it stood and the next app is started. Nothing happens here at a login where the session does not
/// come back, which is every login but the first after a boot.
fn coming_back(state: &mut Lens, open: &Open) -> Task<Message> {
    // a picture with no workspace in it is a compositor that has gone away, and there is nothing to
    // open an app on
    if open.all.is_empty() {
        return Task::none();
    }
    let Some(restore) = state.restore.as_mut() else {
        return Task::none();
    };
    let next = restore::saw(restore, open, &state.apps);
    went_on(state, next)
}

/// What the shell does after a step of bringing the session back: wait for the window, wait for the
/// patience of the app just started to run out, or put up what nothing here could open again.
fn went_on(state: &mut Lens, next: restore::Next) -> Task<Message> {
    match next {
        restore::Next::Wait => Task::none(),
        restore::Next::Started(turn) => later(restore::PATIENCE, Message::Restoring(turn)),
        restore::Next::Soon(turn) => later(restore::SETTLE, Message::Restoring(turn)),
        restore::Next::Done(passed) => {
            let back = state.restore.as_ref().map_or(0, restore::Restore::back);
            state.back = Back::Windows(back, passed.len());
            state.restore = None;
            eprintln!(
                "lens: the session came back: {back} windows, {} passed over",
                passed.len()
            );
            if passed.is_empty() {
                return Task::none();
            }
            let body: Vec<String> = passed.iter().map(session::Passed::line).collect();
            Task::done(Message::Notified(Notification {
                id: u32::MAX - 1,
                app: "Lens".to_string(),
                icon: Some(PASSED_OVER.to_string()),
                entry: None,
                summary: "Some windows did not come back".to_string(),
                body: body.join("\n"),
                actions: Vec::new(),
                default: false,
                urgency: notice::Urgency::Normal,
                transient: false,
            }))
        }
    }
}

/// What the clock and the status sources said: the bar and the menus draw from it.
fn heard(state: &mut Lens, message: Message) {
    match message {
        Message::Tick(now) => {
            state.clock = now.line;
            if now.today.is_some() {
                state.today = now.today;
            }
        }
        Message::Network(picture) => {
            if let Err(why) = &*picture {
                eprintln!("lens: {why}");
            }
            state.status.network = (*picture).ok();
        }
        Message::Battery(battery) => state.status.battery = battery,
        Message::Bluetooth(bluetooth) => state.status.bluetooth = bluetooth,
        Message::Sound(volume) => state.status.volume = volume,
        Message::Brightness(brightness) => state.status.brightness = brightness,
        _ => {}
    }
}

/// The clock menu, notifications and the key popup.
fn notices(state: &mut Lens, message: Message) -> Task<Message> {
    match message {
        Message::ToggleClock => toggle_clock(state),
        Message::Clock(event) => clock_event(state, event),
        Message::Notified(notification) => notified(state, notification),
        Message::Recalled(id) => {
            let effects = state.notices.recall(id);
            apply(state, effects)
        }
        Message::Outbox(outbox) => {
            state.outbox = Some(outbox);
            Task::none()
        }
        Message::Banner(event) => banner_event(state, event),
        Message::Expire(id, epoch) => {
            let effects = state.notices.expire(id, epoch);
            apply(state, effects)
        }
        Message::PopupDone(key) => match state.popup {
            Some(shown) if shown.epoch == key => {
                state.popup = None;
                Task::done(Message::Close(shown.id))
            }
            _ => Task::none(),
        },
        _ => Task::none(),
    }
}

/// Open the menu when it is closed, close it when it is open.
fn toggle(state: &mut Lens) -> Task<Message> {
    if state.menu.is_some() {
        Task::done(Message::Dismiss)
    } else if state.dialog.is_some() || recently(state, Closed::Applications) {
        Task::none()
    } else {
        open(state)
    }
}

/// Open the clock menu when it is closed, close it when it is open. It opens on the month today is
/// in.
fn toggle_clock(state: &mut Lens) -> Task<Message> {
    if state.datemenu.is_some() {
        return close_clock(state);
    }
    if state.dialog.is_some() || recently(state, Closed::Clock) {
        return Task::none();
    }
    let id = window::Id::unique();
    let height = datemenu::height(state.notices.kept.len());
    let month = state.today.map_or(
        Month {
            year: 1970,
            month: 1,
        },
        Month::of,
    );
    state.datemenu = Some(datemenu::Menu { id, height, month });
    Task::done(Message::OpenClock(id, height, menu_top(&state.dock)))
}

fn close_clock(state: &mut Lens) -> Task<Message> {
    state
        .datemenu
        .take()
        .map_or_else(Task::none, |menu| Task::done(Message::Close(menu.id)))
}

/// A button or the switch of the clock menu.
fn clock_event(state: &mut Lens, event: datemenu::Event) -> Task<Message> {
    match event {
        datemenu::Event::Previous | datemenu::Event::Next => {
            if let Some(menu) = state.datemenu.as_mut() {
                menu.month = if event == datemenu::Event::Next {
                    menu.month.next()
                } else {
                    menu.month.previous()
                };
            }
            Task::none()
        }
        datemenu::Event::Clear => {
            let effects = state.notices.clear();
            apply(state, effects)
        }
        datemenu::Event::Quiet(on) => {
            state.notices.quiet = on;
            report(notifications::keep_quiet(on));
            Task::none()
        }
    }
}

/// A notification came in: its icon is found and its words are cut to fit here, once, and then it
/// shows under the bar and goes in the list.
fn notified(state: &mut Lens, notification: Notification) -> Task<Message> {
    // an app that has sent one is one the Notifications page offers a switch for
    if let Some(grown) = notifications::with_sender(
        &state.senders,
        &notification.app,
        notification.entry.as_deref(),
    ) {
        report(notifications::save_senders(&grown));
        state.senders = grown;
    }
    let icon = banner::icon(&state.apps, &notification);
    let (summary, body, lines) = banner::texts(&notification, icon.is_some());
    let (row_summary, row_body) = datemenu::texts(&notification, icon.is_some());
    let height = banner::height(lines, notification.actions.len());
    let fitted = Fitted {
        icon,
        summary,
        body,
        lines,
        row_summary,
        row_body,
    };
    let time = clock::minute(&state.clock).to_string();
    let effects = state.notices.arrive(notification, fitted, height, &time);
    apply(state, effects)
}

/// A press, a button or the pointer on a notification on screen.
fn banner_event(state: &mut Lens, event: banner::Event) -> Task<Message> {
    let effects = match event {
        banner::Event::Press(id) => state.notices.act(id, None),
        banner::Event::Close(id) => state.notices.dismiss(id),
        banner::Event::Action(id, key) => state.notices.act(id, Some(&key)),
        banner::Event::Hover(id, over) => state.notices.hover(id, over),
    };
    apply(state, effects)
}

/// Do what the notifications changing asks for: surfaces to open, move, resize or close, signals
/// for the bus, and the time a notification stays.
fn apply(state: &Lens, effects: Vec<Effect>) -> Task<Message> {
    let mut tasks = Vec::new();
    for effect in effects {
        match effect {
            Effect::Open(id, height, top) => {
                tasks.push(Task::done(Message::OpenBanner(id, height, top)));
            }
            Effect::Move(id, top) => tasks.push(Task::done(Message::Place(id, top))),
            Effect::Resize(id, height) => {
                tasks.push(Task::done(Message::Resize(id, banner::WIDTH, height)));
            }
            Effect::Close(id) => tasks.push(Task::done(Message::Close(id))),
            Effect::Signal(signal) => {
                if let Some(outbox) = &state.outbox {
                    outbox.send(signal);
                }
            }
            Effect::Time(id, epoch) => {
                tasks.push(later(notice::SHOWN, Message::Expire(id, epoch)));
            }
        }
    }
    Task::batch(tasks)
}

/// A message after a while, from a thread that sleeps: the executor has no timer of its own.
fn later(delay: Duration, message: Message) -> Task<Message> {
    Task::perform(
        async move {
            let (sender, receiver) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                let _ = sender.send(());
            });
            let _ = receiver.await;
        },
        move |()| message,
    )
}

/// A volume or a brightness key was pressed: the popup shows the level for a second, or for a
/// second more when it is already up, and the bar follows at once.
fn show_popup(state: &mut Lens, level: Level) -> Task<Message> {
    match level {
        Level::Volume { level, muted } => {
            state.status.volume = Some(Volume {
                level: u16::from(level),
                muted,
            });
        }
        Level::Brightness(level) => state.status.brightness = Some(level),
    }
    state.keys += 1;
    let key = state.keys;
    let timer = later(popup::SHOWN, Message::PopupDone(key));
    if let Some(shown) = state.popup.as_mut() {
        shown.level = level;
        shown.epoch = key;
        return timer;
    }
    let id = window::Id::unique();
    state.popup = Some(Popup {
        id,
        level,
        epoch: key,
    });
    Task::batch([Task::done(Message::OpenPopup(id)), timer])
}

/// Whether the compositor closed this menu a moment ago. The press that took the keyboard away
/// from it may be on its own button, whose click then arrives after it closed.
fn recently(state: &Lens, which: Closed) -> bool {
    state
        .dismissed
        .is_some_and(|(closed, when)| closed == which && when.elapsed() < REOPEN)
}

/// Open the system menu when it is closed, close it when it is open. A dialog holds the keyboard,
/// so nothing opens over it.
fn toggle_system(state: &mut Lens) -> Task<Message> {
    if state.system.is_some() {
        return close_system(state);
    }
    if state.dialog.is_some() || recently(state, Closed::System) {
        return Task::none();
    }
    let id = window::Id::unique();
    let height = system::height(&system::parts(&state.status, None));
    state.system = Some(system::Menu::new(id, height));
    // the backlight is read as the menu opens, and the card is asked to look for networks, so the
    // list is fresh by the time the owner reads it
    let reading = Task::perform(async { status::brightness() }, Message::Brightness);
    let device = state
        .status
        .network
        .as_ref()
        .and_then(|picture| picture.wireless.as_ref())
        .map(|wireless| wireless.path.clone());
    if let Some(device) = device {
        // what the scan finds comes back through NetworkManager's signals
        std::thread::spawn(move || network::scan(&device));
    }
    let top = menu_top(&state.dock);
    Task::batch([Task::done(Message::OpenSystem(id, height, top)), reading])
}

fn close_system(state: &mut Lens) -> Task<Message> {
    state
        .system
        .take()
        .map_or_else(Task::none, |menu| Task::done(Message::Close(menu.id)))
}

/// A row, a switch or a slider of the system menu.
fn system_event(state: &mut Lens, event: system::Event) -> Task<Message> {
    use system::Event;
    match event {
        Event::Volume(level) => {
            if let Some(menu) = state.system.as_mut() {
                menu.volume = Some(level);
            }
            state.status.volume = Some(Volume {
                level: u16::from(level),
                muted: false,
            });
            state.volume.send(level);
            Task::none()
        }
        Event::VolumeSet => {
            if let Some(menu) = state.system.as_mut() {
                menu.volume = None;
            }
            Task::none()
        }
        Event::Mute => off_thread(|| sound::toggle_mute(Side::Output), Message::Acted),
        Event::Brightness(level) => {
            if let Some(menu) = state.system.as_mut() {
                menu.brightness = Some(level);
            }
            state.status.brightness = Some(level);
            state.brightness.send(level);
            Task::none()
        }
        Event::BrightnessSet => {
            if let Some(menu) = state.system.as_mut() {
                menu.brightness = None;
            }
            Task::none()
        }
        Event::Wifi(on) => off_thread(move || network::set_wifi(on), Message::Acted),
        Event::Join(at) => join(state, at),
        Event::Bluetooth(on) => {
            let Some(adapter) = state
                .status
                .bluetooth
                .as_ref()
                .map(|bluetooth| bluetooth.adapter.clone())
            else {
                return Task::none();
            };
            off_thread(move || bluetooth::set_powered(&adapter, on), Message::Acted)
        }
        Event::Device(at) => {
            let Some(device) = state
                .status
                .bluetooth
                .as_ref()
                .and_then(|bluetooth| bluetooth.devices.get(at))
                .cloned()
            else {
                return Task::none();
            };
            if let Some(menu) = state.system.as_mut() {
                let doing = if device.connected {
                    "Disconnecting"
                } else {
                    "Connecting to"
                };
                menu.error = None;
                menu.notice = Some(format!("{doing} {}", device.name));
            }
            off_thread(
                move || bluetooth::connect(&device.path, !device.connected),
                Message::Acted,
            )
        }
        Event::Lock => {
            // the lock screen covers everything, the menu included, and comes back to a desktop
            // with the menu closed
            let closing = close_system(state);
            Task::batch([closing, off_thread(session::lock, Message::Acted)])
        }
        Event::LogOut => ask_first(state, &["power", "logout"]),
        Event::Restart => ask_first(state, &["power", "reboot"]),
        Event::ShutDown => ask_first(state, &["power", "off"]),
    }
}

/// A network in the system menu was picked. One that needs a password nobody saved asks for it in
/// a dialog; any other is joined at once.
fn join(state: &mut Lens, at: usize) -> Task<Message> {
    let picked = state
        .status
        .network
        .as_ref()
        .and_then(|picture| picture.wireless.as_ref())
        .and_then(|wireless| {
            wireless
                .networks
                .get(at)
                .map(|network| (wireless.path.clone(), network.clone()))
        });
    let Some((device, network)) = picked else {
        return Task::none();
    };
    if network.active {
        return Task::none();
    }
    if network.security.joinable() && network.security.secured() && network.saved.is_none() {
        return open_dialog(state, Ask::Password { network, device });
    }
    let Some(menu) = state.system.as_mut() else {
        return Task::none();
    };
    if !network.security.joinable() {
        menu.notice = None;
        menu.error = Some(format!(
            "{} asks for a user name, which the menu cannot do yet.",
            network.name
        ));
        return Task::none();
    }
    menu.error = None;
    menu.notice = Some(format!("Connecting to {}", network.name));
    off_thread(
        move || {
            let joining = network::join(&device, &network, None)?;
            network::wait(&joining, JOIN_WAIT)
                .map_err(|_| format!("Could not connect to {}.", network.name))
        },
        Message::Acted,
    )
}

/// Ask before a command that ends the session or the machine's run.
fn ask_first(state: &mut Lens, words: &[&str]) -> Task<Message> {
    match os::parse(words) {
        Some(Ok(action)) => open_dialog(state, Ask::Command(action)),
        _ => Task::none(),
    }
}

/// Open a dialog in the middle of the screen. The system menu closes: the dialog takes the
/// keyboard, and the question is the owner's whole attention.
fn open_dialog(state: &mut Lens, ask: Ask) -> Task<Message> {
    let closing = close_system(state);
    if state.dialog.is_some() {
        return closing;
    }
    let found = Dialog::new(window::Id::unique(), ask);
    let opening = Task::done(Message::OpenDialog(found.id, found.height()));
    state.dialog = Some(found);
    Task::batch([closing, opening])
}

fn close_dialog(state: &mut Lens) -> Task<Message> {
    state
        .dialog
        .take()
        .map_or_else(Task::none, |found| Task::done(Message::Close(found.id)))
}

/// A dialog's field or buttons.
fn dialog_event(state: &mut Lens, event: dialog::Event) -> Task<Message> {
    let Some(found) = state.dialog.as_mut() else {
        return Task::none();
    };
    match event {
        dialog::Event::Input(value) => {
            found.input = value;
            found.error = None;
            Task::none()
        }
        dialog::Event::Cancel => close_dialog(state),
        dialog::Event::Confirm => {
            if !found.ready() {
                return Task::none();
            }
            found.busy = true;
            found.error = None;
            let id = found.id;
            match found.ask.clone() {
                Ask::Command(action) => off_thread(
                    move || os::run(&action).map(|_| ()),
                    move |result| Message::DialogDone(id, result),
                ),
                Ask::Password { network, device } => {
                    let password = found.input.clone();
                    off_thread(
                        move || {
                            let joining = network::join(&device, &network, Some(&password))?;
                            network::wait(&joining, JOIN_WAIT).map_err(|_| {
                                format!(
                                    "Could not connect to {}. Check the password and try again.",
                                    network.name
                                )
                            })
                        },
                        move |result| Message::DialogDone(id, result),
                    )
                }
            }
        }
    }
}

/// What a dialog asked for finished. It closes when it worked, and says what went wrong when it
/// did not, with the cursor back in its field.
fn dialog_done(state: &mut Lens, id: window::Id, result: Result<(), String>) -> Task<Message> {
    let Some(found) = state.dialog.as_mut().filter(|found| found.id == id) else {
        return Task::none();
    };
    match result {
        Ok(()) => close_dialog(state),
        Err(why) => {
            found.busy = false;
            found.error = Some(why);
            dialog::focus_field()
        }
    }
}

/// Run something that asks a service or a program on a thread of its own, so the shell keeps
/// drawing while it waits, and hand back how it went.
fn off_thread<W, D>(work: W, done: D) -> Task<Message>
where
    W: FnOnce() -> Result<(), String> + Send + 'static,
    D: Fn(Result<(), String>) -> Message + Send + 'static,
{
    Task::perform(
        async move {
            let (sender, receiver) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = sender.send(work());
            });
            receiver
                .await
                .unwrap_or_else(|_| Err("It stopped before it finished.".to_string()))
        },
        done,
    )
}

fn open(state: &mut Lens) -> Task<Message> {
    if state.menu.is_some() {
        return Task::none();
    }
    // the entries are read again here, so an app installed since the session started is in the
    // list without a restart. it is a walk of a few directories, once per opening, and the places
    // are read with them, since a folder of home can be made or taken away in the same way
    state.apps = launcher::load();
    state.places = places::places()
        .into_iter()
        .chain(places::exchange())
        .collect();
    let id = window::Id::unique();
    let menu = Menu::new(id, &state.apps, &state.places);
    let height = menu.height;
    state.menu = Some(menu);
    Task::done(Message::Open(id, height, menu_top(&state.dock)))
}

fn close(state: &mut Lens) -> Task<Message> {
    // the shell listens while this menu is open and nowhere else
    forget(state);
    state
        .menu
        .take()
        .map_or_else(Task::none, |menu| Task::done(Message::Close(menu.id)))
}

/// Escape cancels a dialog and closes the system menu. In the Applications menu it stops the shell
/// listening, then clears the field, the way a search entry does, and closes the menu when there is
/// nothing left to clear.
fn escape(state: &mut Lens) -> Task<Message> {
    if state.dialog.is_some() {
        return close_dialog(state);
    }
    if state.system.is_some() {
        return close_system(state);
    }
    if state.datemenu.is_some() {
        return close_clock(state);
    }
    // escape stops the shell listening before it clears anything, since the line that says it is
    // listening is what it would otherwise take away
    if state.listening.is_some() {
        forget(state);
        return note(state, "The shell stopped listening.".to_string());
    }
    let Lens { apps, menu, .. } = state;
    match menu.as_mut() {
        None => Task::none(),
        Some(menu) if menu.has_anything() => {
            menu.clear(apps);
            Task::batch([menu::focus_field(), menu.scroll()])
        }
        Some(_) => Task::done(Message::Dismiss),
    }
}

/// A click on a dock item: the app starts when it is not running, its window comes forward when
/// it is, and the next of its windows when one of them is the one being used.
fn dock_click(state: &mut Lens, key: &str) -> Task<Message> {
    let closing = close_item_menu(state);
    if let Some(address) = state.dock.place(key).map(|place| place.address.clone()) {
        open_place(state, &address);
        return closing;
    }
    let Some(item) = state.dock.item(key) else {
        return closing;
    };
    if let Some(window) = item.next() {
        report(horizon::focus(window));
    } else if let Some(app) = item.app.as_ref() {
        open_app(app);
    }
    closing
}

/// A middle click on an item, and New window in its menu: one more window of that app.
fn new_window(state: &Lens, key: &str) {
    if let Some(app) = state.dock.item(key).and_then(|item| item.app.as_ref()) {
        open_app(app);
    }
}

/// A right click on an item: the menu of what can be done with it, on a surface of its own where
/// the item is. A second right click on the same item closes it again.
fn dock_menu(state: &mut Lens, key: &str) -> Task<Message> {
    let same = state.dock.menu.as_ref().is_some_and(|menu| menu.key == key);
    let closing = close_item_menu(state);
    if same {
        return closing;
    }
    let rows = state
        .dock
        .item(key)
        .map(dock::Item::rows)
        .or_else(|| state.dock.place(key).map(dock::Place::rows));
    let Some(rows) = rows else {
        return closing;
    };
    if rows.is_empty() {
        return closing;
    }
    // the places stand at the right end, so a menu hanging from one would run off the screen
    let widest = i32::try_from(state.screen.saturating_sub(dock::MENU_WIDTH)).unwrap_or(0);
    let left = state
        .dock
        .left_of(key, state.screen)
        .clamp(0, widest.max(0));
    let height = dock::menu_height(rows.len());
    let id = window::Id::unique();
    state.dock.menu = Some(dock::Menu {
        id,
        key: key.to_string(),
        rows,
    });
    let edge = state.dock.options.edge;
    // a dock that hides keeps nothing of the screen, so its menu stands on it by its height
    let above = if state.dock.options.hides() {
        let gap = if state.dock.options.extend {
            0
        } else {
            margin(dock::OFF_EDGE)
        };
        margin(state.dock.height()) + gap
    } else {
        0
    };
    Task::batch([
        closing,
        Task::done(Message::OpenItemMenu(id, height, left, edge, above)),
    ])
}

/// A row of that menu. Every one of them closes it.
fn dock_row(state: &mut Lens, row: &dock::Row) -> Task<Message> {
    let Some(key) = state.dock.menu.as_ref().map(|menu| menu.key.clone()) else {
        return Task::none();
    };
    let closing = close_item_menu(state);
    match row {
        dock::Row::Window(window, _) => report(horizon::focus(*window)),
        dock::Row::New => new_window(state, &key),
        dock::Row::Pin(_) => state.dock.pin(&key, &state.apps),
        dock::Row::Close => {
            let windows: Vec<u64> = state
                .dock
                .item(&key)
                .map(|item| item.windows.iter().map(|(id, _)| *id).collect())
                .unwrap_or_default();
            for window in windows {
                report(horizon::close(window));
            }
        }
        dock::Row::Open => {
            if let Some(address) = state.dock.place(&key).map(|place| place.address.clone()) {
                open_place(state, &address);
            }
        }
        dock::Row::Eject => {
            let drive = state.dock.place(&key).and_then(|place| place.drive.clone());
            if let Some(drive) = drive {
                // unmounting writes out everything that was waiting, which takes as long as it
                // takes, so the shell keeps drawing while udisks does it
                return Task::batch([
                    closing,
                    off_thread(move || librift::drives::eject(&drive), Message::Acted),
                ]);
            }
        }
    }
    closing
}

/// A click on a place over the apps in the Applications menu: it opens in the file manager and
/// the menu closes, the way it does when a row starts an app.
fn pick_place(state: &mut Lens, at: usize) -> Task<Message> {
    let address = state
        .menu
        .as_ref()
        .and_then(|menu| menu.places.get(at))
        .map(|place| place.path.display().to_string());
    if let Some(address) = address {
        open_place(state, &address);
    }
    close(state)
}

/// Something went into the owner's trash, or the last thing came out of it: the dock keeps a
/// place for the trash while there is something in it.
fn trashed(state: &mut Lens, anything: bool) -> Task<Message> {
    state.dock.trashed(anything);
    Task::none()
}

/// udisks says these are the disks now. A disk that was ejected while its menu was open takes the
/// menu with it.
fn plugged(state: &mut Lens, found: Vec<drives::Volume>) -> Task<Message> {
    state.dock.plugged(found);
    let gone = state.dock.menu.as_ref().is_some_and(|menu| {
        state.dock.place(&menu.key).is_none() && state.dock.item(&menu.key).is_none()
    });
    if gone {
        return close_item_menu(state);
    }
    Task::none()
}

/// Open a place in the file manager: the app the image opens a folder with, started with the
/// place after its own command, in a scope of its own like every other app the shell starts.
fn open_place(state: &Lens, address: &str) {
    let Some(app) = librift::defaults::manager(&state.apps) else {
        eprintln!("lens: there is no app on this machine that opens a folder");
        return;
    };
    report(librift::apps::launch_with(&app, &[address.to_string()]));
}

/// Close the menu a right click opened, when one is open. A dock that hides stayed out while it
/// was, so the wait before it goes starts again unless the pointer is on it.
fn close_item_menu(state: &mut Lens) -> Task<Message> {
    let Some(menu) = state.dock.menu.take() else {
        return Task::none();
    };
    let closing = Task::done(Message::Close(menu.id));
    match state.dock.wait() {
        Some(number) => Task::batch([closing, later(dock::HIDE_AFTER, Message::HideDock(number))]),
        None => closing,
    }
}

/// Start an app from the dock. What went wrong goes in the journal: the dock has no line to say
/// it on, and the app either opens a window or it does not.
fn open_app(app: &App) {
    report(launcher::launch(app));
}

fn report(done: Result<(), String>) {
    if let Err(why) = done {
        eprintln!("lens: {why}");
    }
}

/// A surface took or lost the keyboard. A menu closes when it loses it, which is what happens
/// when anything outside it is clicked; when the Applications menu or a password dialog takes it,
/// the cursor goes in the field.
fn focused(state: &mut Lens, id: window::Id, has: bool) -> Task<Message> {
    if let Some(found) = state.dialog.as_ref().filter(|found| found.id == id) {
        return if has && matches!(found.ask, Ask::Password { .. }) {
            dialog::focus_field()
        } else {
            Task::none()
        };
    }
    if state.system.as_ref().is_some_and(|menu| menu.id == id) {
        if has {
            return Task::none();
        }
        state.dismissed = Some((Closed::System, Instant::now()));
        return close_system(state);
    }
    if state.datemenu.as_ref().is_some_and(|menu| menu.id == id) {
        if has {
            return Task::none();
        }
        state.dismissed = Some((Closed::Clock, Instant::now()));
        return close_clock(state);
    }
    if state.dock.menu.as_ref().is_some_and(|menu| menu.id == id) {
        return if has {
            Task::none()
        } else {
            close_item_menu(state)
        };
    }
    if state.menu.as_ref().is_none_or(|menu| menu.id != id) {
        return Task::none();
    }
    if has {
        menu::focus_field()
    } else {
        state.dismissed = Some((Closed::Applications, Instant::now()));
        Task::done(Message::Dismiss)
    }
}

/// A line from the socket. Typing into the field opens the menu when it is closed, because the
/// boot test and Quasar's step reach the field that way.
fn typed(state: &mut Lens, command: Command) -> Task<Message> {
    match command {
        Command::Type(words) => {
            let opening = open(state);
            Task::batch([opening, write(state, words)])
        }
        Command::Enter(words) => {
            let opening = open(state);
            let writing = if words.is_empty() {
                Task::none()
            } else {
                write(state, words)
            };
            Task::batch([opening, writing, submit(state)])
        }
        Command::Escape => escape(state),
        Command::Menu => toggle(state),
        Command::Listen => listen(state),
        Command::Popup(level) => show_popup(state, level),
        Command::Record(recording) => record(state, &recording),
        Command::Look => look(state),
        Command::Dock => {
            // the menu of an item hangs where the item was, so it goes with the old dock
            let closing = close_item_menu(state);
            state.dock.reload(&state.apps);
            closing
        }
        Command::Notifications => {
            state.notices.quiet = notifications::quiet();
            state.notices.muted = notifications::quiet_apps();
            Task::none()
        }
        // answered on the socket's own thread, from the lines remember() keeps
        Command::State => Task::none(),
    }
}

/// The appearance settings changed. The shell reads the theme and the accent again and draws with
/// them; the desktop, the apps and the lock screen follow through the files Settings wrote.
fn look(state: &mut Lens) -> Task<Message> {
    state.theme = appearance::Theme::read();
    state.accent = appearance::Accent::read();
    state.look = crate::theme::palette(state.theme, state.accent);
    remember(state);
    let text = appearance::text();
    if SCALE.swap(text, Ordering::Relaxed) == text {
        return Task::none();
    }
    // a menu is made when it opens and is asked for at the new size then; the bar is there all
    // session, so it is told its height and how much of the screen it keeps, and the dock is stood
    // at its new size after this message the way it is after every other
    let Some(id) = state.bar else {
        return Task::none();
    };
    Task::batch([
        Task::done(Message::Resize(id, 0, bar::HEIGHT)),
        Task::done(Message::Reserve(id, bar::HEIGHT)),
    ])
}

/// The screen recorder started or stopped. While it runs the bar carries the mark every desktop
/// puts up for it; when it stops, a notification names the file it left behind, and a second
/// recording's notification takes the place of the first.
fn record(state: &mut Lens, recording: &Recording) -> Task<Message> {
    match recording {
        Recording::On(file) => {
            state.recording = Some(file.clone());
            Task::none()
        }
        Recording::Off(file) => {
            state.recording = None;
            Task::done(Message::Notified(Notification {
                id: u32::MAX,
                app: "Lens".to_string(),
                icon: Some(bar::RECORDING.to_string()),
                entry: None,
                summary: "Screen recording saved".to_string(),
                body: file.clone(),
                actions: Vec::new(),
                default: false,
                urgency: notice::Urgency::Normal,
                transient: false,
            }))
        }
    }
}

/// The key for push to talk. The shell listens while the Applications menu is open and nowhere
/// else: the menu opens for the first press, and a second press, the minute the shell listens for
/// at most, or the menu closing all stop the recording. A microphone that runs behind a shut menu
/// is one nobody can see.
fn listen(state: &mut Lens) -> Task<Message> {
    if state.listening.is_some() {
        return stop_listening(state);
    }
    let opening = open(state);
    let recording = match talk::start() {
        Ok(recording) => recording,
        Err(why) => return Task::batch([opening, note(state, why)]),
    };
    state.listening = Some(recording);
    state.ears += 1;
    let ears = state.ears;
    let listening = note(state, LISTENING.to_string());
    Task::batch([opening, listening, later(talk::MOST, Message::Deaf(ears))])
}

/// The minute the shell listens for at most is over. Nobody says a sentence to a launcher for a
/// minute, so this is a key somebody pressed and forgot: the recording is thrown away rather than
/// sent, since the shell would otherwise ask Quasar about a room and read the answer out loud.
fn deaf(state: &mut Lens, ears: u64) -> Task<Message> {
    if state.ears != ears || state.listening.is_none() {
        return Task::none();
    }
    forget(state);
    note(
        state,
        "The shell stopped listening after a minute.".to_string(),
    )
}

/// Stop listening and turn what was recorded into words, on a thread of its own: reading the
/// recording and the model that reads it back both take a moment.
fn stop_listening(state: &mut Lens) -> Task<Message> {
    let Some(recording) = state.listening.take() else {
        return Task::none();
    };
    let working = note(state, "Working out what you said.".to_string());
    let heard = Task::perform(
        async move {
            let (sender, receiver) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = sender.send(recording.stop().and_then(|wav| talk::words(&wav)));
            });
            receiver
                .await
                .unwrap_or_else(|_| Err("The shell stopped before it heard you.".to_string()))
        },
        Message::Heard,
    );
    Task::batch([working, heard])
}

/// Stop listening and keep nothing: the menu closed, or the minute ran out.
fn forget(state: &mut Lens) {
    if let Some(recording) = state.listening.take() {
        std::thread::spawn(move || {
            let _ = recording.stop();
        });
    }
}

/// What the recording turned into. The words go in the field and Enter follows them, since
/// somebody who has spoken a whole sentence has said everything they mean; the line stays in the
/// field, because the shell typed it and what it typed has to be readable.
fn spoken(state: &mut Lens, result: Result<Option<String>, String>) -> Task<Message> {
    let words = match result {
        Ok(Some(words)) => words,
        Ok(None) => return note(state, "Nothing was said.".to_string()),
        Err(why) => {
            eprintln!("lens: the recording could not be read: {why}");
            return note(state, why);
        }
    };
    let Lens { apps, menu, .. } = state;
    let Some(menu) = menu.as_mut() else {
        return Task::none();
    };
    // the line is whole, so there is no wait for more of it and no search of home while it is
    // typed on: Enter is what a spoken line gets, and Enter is the same four readings as ever
    menu.typed(apps, words);
    menu.heard = true;
    let scrolling = menu.scroll();
    Task::batch([scrolling, submit(state)])
}

/// A line under the field about the listening, in the gray a notice has. A machine that cannot
/// hear is not a person making a mistake.
fn note(state: &mut Lens, said: String) -> Task<Message> {
    if let Some(menu) = state.menu.as_mut() {
        menu.notice = Some(said);
        menu.error = None;
    }
    Task::none()
}

/// The answer went to the speakers, or it did not. What was read out loud is what `lens --state`
/// prints, so a test can hear the shell in a machine with no ears.
fn said(state: &mut Lens, result: Result<String, String>) -> Task<Message> {
    match result {
        Ok(said) => state.said = Some(said),
        Err(why) => eprintln!("lens: the answer was not read out loud: {why}"),
    }
    Task::none()
}

/// Read an answer out loud, on a thread of its own: the voice takes a second to make the sound and
/// as long as the sentence to play it.
fn aloud(answer: String) -> Task<Message> {
    Task::perform(
        async move {
            let (sender, receiver) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = sender.send(talk::say(&answer));
            });
            receiver
                .await
                .unwrap_or_else(|_| Err("The voice stopped before it spoke.".to_string()))
        },
        Message::Said,
    )
}

/// The field and the list under it: a line typed or entered, the arrows, a row pressed, and the
/// search of home that plain words start.
fn field(state: &mut Lens, message: Message) -> Task<Message> {
    match message {
        Message::Input(value) => write(state, value),
        Message::Submit => submit(state),
        Message::Move(step) => state.menu.as_mut().map_or_else(Task::none, |menu| {
            menu.step(step);
            menu.scroll()
        }),
        Message::Pick(at) => {
            if let Some(menu) = state.menu.as_mut() {
                menu.selected = Some(at);
            }
            submit(state)
        }
        Message::PickPlace(at) => pick_place(state, at),
        Message::Search(words) => search(state, words),
        Message::Found(words, result) => found(state, &words, result),
        Message::PickFile(at) => pick_file(state, at),
        _ => Task::none(),
    }
}

/// New words in the field. The list is shorter or longer for them, so it goes back to its top,
/// which the widget itself does not do when its contents change. Plain words also start the wait
/// before home is looked through for them.
fn write(state: &mut Lens, words: String) -> Task<Message> {
    let Lens { apps, menu, .. } = state;
    menu.as_mut().map_or_else(Task::none, |menu| {
        menu.typed(apps, words);
        let scrolling = menu.scroll();
        match route::searched(&menu.input, apps) {
            Some(words) => Task::batch([scrolling, later(find::PAUSE, Message::Search(words))]),
            None => scrolling,
        }
    })
}

/// The wait after the last key is over. The words are looked for in the index of home on a thread
/// of its own, since reading the index and asking Quasar for a vector both take a moment, and
/// nothing is said in the menu while it runs. A wait for words the field has moved on from is
/// dropped: the one for what is in it now is on its way.
fn search(state: &Lens, words: String) -> Task<Message> {
    if state
        .menu
        .as_ref()
        .is_none_or(|menu| menu.input.trim() != words)
    {
        return Task::none();
    }
    Task::perform(
        async move {
            let (sender, receiver) = iced::futures::channel::oneshot::channel();
            let asked = words.clone();
            std::thread::spawn(move || {
                let _ = sender.send(find::look(&asked));
            });
            let found = receiver
                .await
                .unwrap_or_else(|_| Err("The search stopped before it finished.".to_string()));
            (words, found)
        },
        |(words, found)| Message::Found(words, found),
    )
}

/// What the search came back with. Files go in a section of their own under the field; nothing
/// close to the words leaves the menu as it was, since the line is a question for Quasar as well.
/// A search that could not run at all says why on the line under the field, in the gray a notice
/// has: the shell asked by itself, so it is not a person's mistake.
fn found(state: &mut Lens, words: &str, result: Result<Vec<find::File>, String>) -> Task<Message> {
    let Some(menu) = state.menu.as_mut() else {
        return Task::none();
    };
    if menu.input.trim() != words {
        return Task::none();
    }
    match result {
        Ok(files) if files.is_empty() => Task::none(),
        Ok(files) => {
            menu.found(files);
            menu.scroll()
        }
        Err(why) => {
            eprintln!("lens: the search for {words:?} could not run: {why}");
            menu.notice = Some(why);
            Task::none()
        }
    }
}

/// A press on a file a search found: it opens with the app its kind opens with, in a scope of its
/// own, and the menu closes, the way it does when a place or an app row is pressed. A kind no app
/// on the machine opens leaves the menu up and says so.
fn pick_file(state: &mut Lens, at: usize) -> Task<Message> {
    let file = state
        .menu
        .as_ref()
        .and_then(|menu| menu.files().nth(at).cloned());
    let Some(file) = file else {
        return Task::none();
    };
    let Some(app) = find::opens(&file, &state.apps) else {
        if let Some(menu) = state.menu.as_mut() {
            menu.error = Some(format!("No app opens {}.", file.name));
        }
        return Task::none();
    };
    report(librift::apps::launch(
        &app,
        std::slice::from_ref(&file.path),
    ));
    close(state)
}

/// Ask the compositor for a taller or shorter menu when one changed shape: the Applications menu as
/// its list grows, the system menu as networks and devices come and go, the clock menu as
/// notifications come in and are cleared.
fn resize(state: &mut Lens) -> Task<Message> {
    let mut tasks = Vec::new();
    if let Some(menu) = state.menu.as_mut() {
        let wanted = menu.wanted_height();
        if wanted != menu.height {
            menu.height = wanted;
            tasks.push(Task::done(Message::Resize(menu.id, menu::WIDTH, wanted)));
        }
    }
    if let Some(menu) = state.system.as_mut() {
        let wanted = system::height(&system::parts(&state.status, Some(&*menu)));
        if wanted != menu.height {
            menu.height = wanted;
            tasks.push(Task::done(Message::Resize(menu.id, system::WIDTH, wanted)));
        }
    }
    if let Some(menu) = state.datemenu.as_mut() {
        let wanted = datemenu::height(state.notices.kept.len());
        if wanted != menu.height {
            menu.height = wanted;
            tasks.push(Task::done(Message::Resize(
                menu.id,
                datemenu::WIDTH,
                wanted,
            )));
        }
    }
    Task::batch(tasks)
}

/// What `lens --state` prints: one line per thing the bar shows.
fn remember(state: &Lens) {
    let mut lines = String::new();
    let mut line = |key: &str, value: &str| {
        lines.push_str(key);
        lines.push(' ');
        lines.push_str(value);
        lines.push('\n');
    };
    let status = &state.status;
    line("clock", &state.clock);
    line("ghost", if librift::ghost::on() { "on" } else { "off" });
    line("theme", state.theme.word());
    line("accent", state.accent.word());
    line("text", &SCALE.load(Ordering::Relaxed).to_string());
    line("apps", &state.apps.len().to_string());
    line("network", &status::network_word(status.network.as_ref()));
    line(
        "volume",
        &status
            .volume
            .map_or_else(|| "none".to_string(), Volume::word),
    );
    line(
        "battery",
        &status
            .battery
            .map_or_else(|| "none".to_string(), status::battery_word),
    );
    line(
        "brightness",
        &status
            .brightness
            .map_or_else(|| "none".to_string(), |level| level.to_string()),
    );
    line("wired", &status::wired_word(status.network.as_ref()));
    line("wifi", &status::wifi_word(status.network.as_ref()));
    line(
        "bluetooth",
        &status::bluetooth_word(status.bluetooth.as_ref()),
    );
    match &state.system {
        None => line("system", "closed"),
        Some(menu) => line("system", &format!("open {}x{}", system::WIDTH, menu.height)),
    }
    match &state.dialog {
        None => line("dialog", "closed"),
        Some(found) => line("dialog", &found.title()),
    }
    notices_lines(state, &mut line);
    match &state.datemenu {
        None => line("clock-menu", "closed"),
        Some(menu) => line(
            "clock-menu",
            &format!("open {}x{}", datemenu::WIDTH, menu.height),
        ),
    }
    match &state.popup {
        None => line("popup", "closed"),
        Some(shown) => line("popup", &shown.level.words()),
    }
    menu_lines(state, &mut line);
    line("dock", &state.dock.line());
    for setting in state.dock.options.lines() {
        let (key, value) = setting.split_once(' ').unwrap_or((&setting, ""));
        line(key, value);
    }
    line("dock-hidden", if state.dock.hidden { "yes" } else { "no" });
    line("dock-places", &state.dock.places_line());
    line("workspaces", &state.dock.spaces_line());
    line("layout", state.layout.as_deref().unwrap_or("none"));
    line(
        "listening",
        if state.listening.is_some() {
            "on"
        } else {
            "off"
        },
    );
    line(
        "said",
        &state
            .said
            .as_deref()
            .map_or_else(|| "none".to_string(), |said| one_line(said, SAID_SHOWN)),
    );
    // how the session came back: how many windows, and how many the journal named that nothing here
    // could open again. `off` is the owner's answer on the Owner page, `none` a login that had
    // nothing to bring back, and the numbers grow while it is going on
    let (restored, passed) = match (state.restore.as_ref(), state.back) {
        (Some(going), _) => (going.back().to_string(), going.passed().to_string()),
        (None, Back::Windows(back, over)) => (back.to_string(), over.to_string()),
        (None, Back::Nothing) => ("none".to_string(), "none".to_string()),
        (None, Back::Off) => ("off".to_string(), "none".to_string()),
    };
    line("restored", &restored);
    line("passed", &passed);
    if let Some(menu) = &state.dock.menu {
        line("item", &format!("{} {}", menu.key, menu.rows.len()));
    }
    if let Ok(mut kept) = kept().lock() {
        *kept = lines;
    }
}

/// The lines about the Applications menu: whether it is open, what is in the field, how many rows
/// are under it, the files a search of home found, the places over the apps, and the line under
/// them all.
fn menu_lines(state: &Lens, line: &mut impl FnMut(&str, &str)) {
    let Some(menu) = &state.menu else {
        line("menu", "closed");
        return;
    };
    line("menu", "open");
    line("field", &menu.input);
    line("rows", &menu.results.shown().to_string());
    let found: Vec<&str> = menu.files().map(|file| file.under.as_str()).collect();
    line("found", &words_or_none(&found));
    let places: Vec<&str> = menu.places.iter().map(|place| place.word).collect();
    line("places", &places.join(" "));
    if let Some((text, wrong)) = menu.line() {
        line(if wrong { "error" } else { "notice" }, text);
    }
}

/// How much of an answer the shell read out loud `lens --state` prints. It is one line of a state,
/// not the answer itself, which is in the menu.
const SAID_SHOWN: usize = 120;

/// A line of a state out of words that may hold newlines and runs of spaces, cut to a length, so
/// what the shell says about itself is always one key and one line.
fn one_line(words: &str, most: usize) -> String {
    let mut line: String = words.split_whitespace().collect::<Vec<&str>>().join(" ");
    if line.chars().count() > most {
        let end = line
            .char_indices()
            .nth(most)
            .map_or(line.len(), |(at, _)| at);
        line.truncate(end);
    }
    if line.is_empty() {
        "none".to_string()
    } else {
        line
    }
}

/// A line's words, or `none` when there are no words, so a line of a state is never empty.
fn words_or_none(words: &[&str]) -> String {
    if words.is_empty() {
        "none".to_string()
    } else {
        words.join(" ")
    }
}

/// The lines about notifications: how many are on screen and how many are kept, the size of each
/// on screen from the top, the summary of the newest kept, and Do not disturb.
fn notices_lines(state: &Lens, line: &mut impl FnMut(&str, &str)) {
    let notices = &state.notices;
    line(
        "notifications",
        &format!("{} {}", notices.banners.len(), notices.kept.len()),
    );
    let sizes: Vec<String> = notices
        .banners
        .iter()
        .map(|shown| format!("{}x{}", banner::WIDTH, shown.height))
        .collect();
    line(
        "banners",
        &if sizes.is_empty() {
            "none".to_string()
        } else {
            sizes.join(" ")
        },
    );
    line(
        "latest",
        notices
            .kept
            .first()
            .map_or("none", |kept| kept.notification.summary.as_str()),
    );
    line("do-not-disturb", if notices.quiet { "on" } else { "off" });
}

fn submit(state: &mut Lens) -> Task<Message> {
    let Lens { apps, menu, .. } = state;
    let Some(menu) = menu.as_mut() else {
        return Task::none();
    };
    if let Some(action) = menu.pending.take() {
        menu.taken();
        menu.notice = None;
        return start(action);
    }
    // the list is a menu: Enter takes the row that is selected, not always the first
    if let Some(app) = menu.selected_app().cloned() {
        launch(menu, &app);
        return Task::done(Message::Dismiss);
    }
    let reading = route::route(&menu.input, apps);
    eprintln!("lens: {:?} -> {reading:?}", menu.input);
    match reading {
        Interpretation::Nothing => {}
        Interpretation::Launch(app) => {
            launch(menu, &app);
            return Task::done(Message::Dismiss);
        }
        Interpretation::Os(action) => return propose(menu, action),
        Interpretation::Usage(usage) => {
            menu.results = Results::None;
            menu.error = Some(usage.to_string());
        }
        Interpretation::Shell(line) => {
            menu.taken();
            menu.results = Results::None;
            menu.error = None;
            return Task::perform(async move { nu::run(&line) }, Message::Done);
        }
        Interpretation::Ask(question) => {
            menu.taken();
            menu.results = Results::None;
            menu.error = None;
            menu.notice = Some("Asking Quasar".to_string());
            return ask(question);
        }
    }
    Task::none()
}

/// An OS command, typed or proposed by Quasar. One that changes something waits for a second
/// Enter, the rest runs at once.
fn propose(menu: &mut Menu, action: Action) -> Task<Message> {
    if action.mutating {
        menu.notice = Some(format!(
            "{}? Press Enter to confirm or Escape to cancel.",
            action.summary
        ));
        menu.pending = Some(action);
        return Task::none();
    }
    menu.taken();
    menu.notice = None;
    start(action)
}

/// The question goes to quasard on a thread of its own. An answer can take a minute, and the
/// executor's few threads also carry the socket the terminal types on.
fn ask(question: String) -> Task<Message> {
    Task::perform(
        async move {
            let (sender, receiver) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = sender.send(quasar::ask(&question));
            });
            receiver
                .await
                .unwrap_or_else(|_| Err("Quasar stopped before it answered.".to_string()))
        },
        Message::Answered,
    )
}

/// Quasar's reply: an answer goes in the list, a command is handled like a typed one, anything
/// else goes on the line under it.
fn answered(menu: &mut Menu, result: Result<(String, String), String>) -> Task<Message> {
    let reply = match result {
        Ok((kind, text)) => quasar::read(&kind, &text),
        Err(why) => quasar::Reply::Refused(why),
    };
    eprintln!("lens: quasar -> {reply:?}");
    menu.notice = None;
    menu.error = None;
    menu.results = Results::None;
    match reply {
        quasar::Reply::Answer(answer) => {
            menu.results = Results::Answer(answer::rows(&answer, menu::ROWS));
            // a question asked out loud is answered out loud, and one that was typed is not: a
            // machine that reads every answer back would be one nobody could work next to
            if menu.heard {
                return aloud(answer);
            }
            Task::none()
        }
        quasar::Reply::Action(action) => propose(menu, action),
        quasar::Reply::Refused(why) => {
            menu.error = Some(why);
            Task::none()
        }
    }
}

fn launch(menu: &mut Menu, app: &App) {
    match launcher::launch(app) {
        Ok(()) => {
            menu.notice = Some(format!("Starting {}", app.name));
            menu.error = None;
        }
        Err(why) => menu.error = Some(why),
    }
    menu.taken();
    menu.results = Results::None;
    menu.selected = None;
}

fn start(action: Action) -> Task<Message> {
    eprintln!("lens: running {} {}", action.program, action.args.join(" "));
    Task::perform(async move { os::run(&action) }, Message::Done)
}

/// What a command or a pipeline printed goes in the list, what it complained about goes on the
/// line under it.
fn finish(menu: &mut Menu, result: Result<String, String>) {
    match result {
        Ok(output) => {
            let rows = nu::rows(&output, menu::ROWS);
            menu.error = None;
            menu.results = if rows.is_empty() {
                menu.notice = Some("Done".to_string());
                Results::None
            } else {
                menu.notice = None;
                Results::Output(rows)
            };
        }
        Err(why) => {
            menu.notice = None;
            menu.results = Results::None;
            menu.error = Some(why);
        }
    }
}

fn view(state: &Lens, id: window::Id) -> Element<'_, Message> {
    if let Some(found) = state.dialog.as_ref().filter(|found| found.id == id) {
        return dialog::view(state.look, found);
    }
    if let Some(menu) = state.system.as_ref().filter(|menu| menu.id == id) {
        return system::view(state.look, &state.status, menu);
    }
    if let Some(menu) = state.datemenu.as_ref().filter(|menu| menu.id == id) {
        return datemenu::view(state.look, menu, state.today, state.first, &state.notices);
    }
    if let Some(shown) = state.notices.banners.iter().find(|shown| shown.id == id) {
        return banner::view(state.look, shown);
    }
    if let Some(shown) = state.popup.as_ref().filter(|shown| shown.id == id) {
        return popup::view(state.look, shown);
    }
    if let Some(menu) = state.menu.as_ref().filter(|menu| menu.id == id) {
        return menu::view(state.look, menu);
    }
    if let Some(menu) = state.dock.menu.as_ref().filter(|menu| menu.id == id) {
        return dock::menu_view(state.look, menu);
    }
    if id == state.dock.id {
        return dock::view(state.look, &state.dock);
    }
    container(bar::view(
        state.look,
        &state.clock,
        &state.status,
        bar::Open {
            applications: state.menu.is_some(),
            clock: state.datemenu.is_some(),
            system: state.system.is_some(),
        },
        bar::Marks {
            quiet: state.notices.quiet,
            recording: state.recording.is_some(),
            layout: state.layout.as_deref(),
            ghost: librift::ghost::on(),
        },
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
