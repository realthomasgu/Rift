//! The window: a header bar along the top, a sidebar of pages down the left, and one page beside
//! it, the way GNOME Settings and Windows Settings are laid out. Drawn with iced in software, in
//! the dark or light colours the owner has chosen.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use iced::futures::channel::mpsc;
use iced::widget::{button, column, container, row, space, text};
use iced::{
    Border, Center, Color, Element, Fill, Length, Size, Subscription, Task, Theme, theme, window,
};
use librift::access::Tool;
use librift::appearance::{Accent, Look, Scheme, Theme as Mode};
use librift::battery::Battery;
use librift::bluetooth as bluetooth_picture;
use librift::boot::Style;
use librift::clock::Zone;
use librift::defaults::Kind;
use librift::keyboard::Layout;
use librift::models::Tier;
use librift::network;
use librift::orbit::Host;
use librift::pointer::{Device, Pointer};
use librift::printers::Printers;
use librift::privacy::Security;
use librift::region::Region;
use librift::sound::{self as sound_picture, Side};

use crate::control::{self, Command};
use crate::ghost;
use crate::net::Joining;
use crate::page::Page;
use crate::theme::{Colors, colors};
pub use crate::widgets::fill;
use crate::widgets::{BOLD, FONT, TEXT_SIZE, scroll};
use crate::{
    about, accessibility, ai, appearance, apps, backups, bluetooth, datetime, displays, dock,
    icons, keyboard, net, notifications, owner, pointer, power, printers, privacy, region, search,
    sound, unlocking, updates, watch,
};

/// What the window calls itself: the name of its desktop entry, which the dock, the compositor and
/// the boot test all know it by.
const APP_ID: &str = "dev.rift.Settings";

/// How wide the sidebar is.
const SIDEBAR: f32 = 208.0;
/// How tall the header bar is. It is the title bar of the window, which the app draws itself.
const HEADER: f32 = 45.0;
/// How tall a row of the sidebar is.
const ROW: f32 = 34.0;

/// What the app is started with.
#[derive(Debug, Default)]
pub struct Start {
    /// The page to open on, when one was named.
    pub page: Option<Page>,
    /// Where to save a picture of the window once it has drawn, and then quit.
    pub screenshot: Option<PathBuf>,
}

/// The window's state.
// a window of switches holds a bool for each of them, which is what this one is
#[allow(clippy::struct_excessive_bools)]
pub struct Settings {
    /// The page that is up.
    pub page: Page,
    /// Dark or light, the accent, the gaps, the radius and the wallpaper.
    pub look: Look,
    /// Whether the terminal greets the first shell of a session.
    pub greeting: bool,
    /// Whether the apps that were open come back at the next login.
    pub restore: bool,
    /// How the next boot of this drive looks, once Vault has said. It is on the esp, not in home,
    /// so Vault is the one that reads and writes it.
    pub boot: Option<Result<Style, String>>,
    /// The wallpapers to choose from: the photographs Rift ships, then the flat colours.
    pub choices: Vec<librift::wallpaper::Choice>,
    /// What Orbit says about this machine, once it has answered.
    pub host: Option<Result<Host, String>>,
    /// What `NetworkManager` says about the cable and the wireless networks, once it has answered
    /// and after every change it makes while the window is open.
    pub network: Option<Result<network::Picture, String>>,
    /// What `BlueZ` says, the same way. `Ok(None)` is a machine with no adapter.
    pub bluetooth: Option<Result<Option<bluetooth_picture::Picture>, String>>,
    /// What `PipeWire` says about the sound, the same way.
    pub sound: Option<Result<sound_picture::Picture, String>>,
    /// The half of the sound whose slider is being dragged, and where it stands.
    pub moving: Option<(Side, u32)>,
    /// What `UPower` says about the battery. `Ok(None)` is a machine that runs on the mains.
    pub battery: Option<Result<Option<Battery>, String>>,
    /// What Quasar says about the models it runs, with the models the manifest declares, once it
    /// has answered and after every change it announces while the window is open.
    pub quasar: Option<Box<ai::Picture>>,
    /// What the front of the search index says, once it has been read.
    pub index: Option<Result<search::Look, String>>,
    /// Whether an update of the search index is running now.
    pub indexing: bool,
    /// The snapshots of home Vault keeps, once it has answered.
    pub snapshots: Option<Result<backups::Kept, String>>,
    /// Where the backups go and what is there, once Vault has answered. It travels behind a
    /// pointer, the way the pictures a service answers with do.
    pub disk: Option<Box<backups::Disk>>,
    /// Whether a snapshot or a backup is being made now.
    pub making: backups::Making,
    /// What the drive's two slots hold, once Vault has answered.
    pub slots: Option<Result<librift::update::Slots, String>>,
    /// What timedated says about the clock and what `date` says the time is, once the Date and
    /// time page has read them, and again every minute while it is up.
    pub clock: Option<datetime::Reading>,
    /// The time zones there are to choose from, out of the database the image carries.
    pub zones: Vec<Zone>,
    /// What has been typed into the search for a time zone, or for a keyboard layout.
    pub finding: String,
    /// The keyboard layouts there are to choose from, out of the list the image carries.
    pub layouts: Vec<Layout>,
    /// What localed says about the keyboard, once the Keyboard page has asked.
    pub keyboard: Option<Result<Region, String>>,
    /// How the owner wants the mouse and the touchpad to behave.
    pub pointer: Pointer,
    /// The mice and the touchpads plugged in, once the Mouse and touchpad page has looked, and again
    /// every two seconds while it is up.
    pub devices: Option<Result<Vec<Device>, String>>,
    /// What localed says the system is set to and what that means, once the Region and language
    /// page has asked.
    pub region: Option<Box<region::Picture>>,
    /// What CUPS says about the printers and the jobs, once the Printers page has asked, and again
    /// every two seconds while it is up.
    pub printers: Option<Result<Printers, String>>,
    /// Whether the screen reader and the on-screen keyboard are running, once the Accessibility
    /// page has looked, and again every second while it is up.
    pub access: Option<accessibility::Running>,
    /// The apps the desktop entries name, for the pages that name apps, read as one of them comes
    /// up.
    pub apps: Vec<librift::apps::App>,
    /// Which app opens each kind of file, once the Apps page has read the lists, and again every
    /// two seconds while it is up.
    pub defaults: Option<apps::Picture>,
    /// The kind whose list of apps is open on the Apps page.
    pub unfolded: Option<Kind>,
    /// The camera's answers, recent files, the firewall and the sandboxes, once the Privacy and
    /// security page has read them, and again every two seconds while it is up.
    pub privacy: Option<Box<privacy::Picture>>,
    /// What fwupd says about the firmware, once it has answered.
    pub security: Option<Result<Security, String>>,
    /// What Vault says about the owner, once the Owner page has asked.
    pub owner: Option<Result<librift::owner::Owner, String>>,
    /// What is typed on the Owner page.
    pub signing: owner::Form,
    /// What Vault says opens the drive besides its passphrase, once the Owner page has asked.
    pub unlocking: Option<Result<unlocking::Picture, String>>,
    /// What is typed in that part of the page.
    pub unlocking_form: unlocking::Form,
    /// What the dock keeps and where it stands, once the Dock page has read it, and again every
    /// second while it is up.
    pub dock: Option<dock::Picture>,
    /// Do not disturb and the apps that have sent a notification, once the Notifications page has
    /// read them, and again every second while it is up.
    pub notices: Option<notifications::Picture>,
    /// The network being joined that asks for a password, and what has been typed for it.
    pub joining: Option<Joining>,
    /// What is happening: a join, or a device being connected.
    pub doing: Option<String>,
    /// Whether the card has been asked to sweep since the Wi-Fi page came up.
    swept: bool,
    /// What the system calls itself, from os-release.
    pub release: String,
    /// The last setting that could not be written.
    pub problem: Option<String>,
    /// Where `--screenshot` saves a picture of the window.
    screenshot: Option<PathBuf>,
}

/// What a press, a drag or a line on the socket asks for.
#[derive(Debug, Clone)]
pub enum Message {
    /// Show this page.
    Show(Page),
    /// Dark or light.
    Mode(Mode),
    /// One of the nine accents.
    Accent(Accent),
    /// The wallpaper at this place in the list.
    Wallpaper(usize),
    /// The gap slider moved, and where it was let go.
    Gaps(u32),
    /// The text size slider moved.
    Text(u32),
    /// One of the terminal colour schemes.
    Terminal(Scheme),
    /// One of the two boot styles.
    Boot(Style),
    /// The size one screen is drawn at, from the Displays page.
    Scale(String, u32),
    /// What `NetworkManager` says now. It is the biggest thing a message carries, so it travels
    /// behind a pointer.
    Network(Box<Result<network::Picture, String>>),
    /// What `BlueZ` says now.
    Bluetooth(Result<Option<bluetooth_picture::Picture>, String>),
    /// What `PipeWire` says now.
    Sound(Box<Result<sound_picture::Picture, String>>),
    /// A volume slider moved, on one half of the sound.
    Volume(Side, u32),
    /// A volume slider was let go, and the level it landed on, which is written.
    Volumed(Side, u32),
    /// The mute switch of one half of the sound.
    Muted(Side, bool),
    /// The device at this place in one half's list was pressed.
    Pick(Side, usize),
    /// What `UPower` says about the battery now.
    Battery(Result<Option<Battery>, String>),
    /// What Quasar says now, with the models on the drive. It travels behind a pointer, the way
    /// the other pictures a service answers with do.
    Quasar(Box<ai::Picture>),
    /// How big a model this machine runs, from the AI page.
    Tier(Tier),
    /// The search index was asked to come up to date.
    Index,
    /// What the front of the search index says now.
    Indexed(Result<search::Look, String>),
    /// How bringing the index up to date went.
    Updated(Result<(), String>),
    /// A snapshot of home was asked for.
    Snapshot,
    /// The snapshots Vault keeps now.
    Snapshots(Result<backups::Kept, String>),
    /// How taking a snapshot went.
    Took(Result<(), String>),
    /// A backup onto the backup disk was asked for.
    BackUp,
    /// Where the backups go and what is there now.
    Backups(Box<backups::Disk>),
    /// How making a backup went.
    BackedUp(Result<(), String>),
    /// What the drive's two slots hold now.
    Slots(Result<librift::update::Slots, String>),
    /// What timedated and `date` say about the clock now. It travels behind a pointer, the way the
    /// pictures a service answers with do.
    Clock(Box<datetime::Reading>),
    /// The search for a time zone was typed into.
    Find(String),
    /// A time zone was chosen, by its name in the database.
    Zone(String),
    /// What localed says now, and what it means.
    Region(Box<region::Picture>),
    /// What CUPS says about the printers and the jobs now.
    Printers(Result<Printers, String>),
    /// A printer or a job was pressed on the Printers page.
    Printer(printers::Asked),
    /// Which of the screen reader and the on-screen keyboard is running now.
    Access(accessibility::Running),
    /// What the dock keeps and where it stands now.
    Docked(dock::Picture),
    /// An app was pinned, moved or taken off the dock, or one of its settings changed.
    Dock(dock::Asked),
    /// Do not disturb and the apps that have sent a notification now.
    Noticed(notifications::Picture),
    /// Do not disturb, or an app's banners, from the Notifications page.
    Notices(notifications::Asked),
    /// Which app opens each kind of file now.
    Kinds(apps::Picture),
    /// A kind's list was opened or closed, or an app chosen for it, on the Apps page.
    Apps(apps::Asked),
    /// What the Privacy and security page reads now.
    Privacy(Box<privacy::Picture>),
    /// A switch on the Privacy and security page.
    Private(privacy::Asked),
    /// What fwupd says about the firmware.
    Security(Result<Security, String>),
    /// What Vault says about the owner now.
    Owner(Result<librift::owner::Owner, String>),
    /// A field or a button on the Owner page, or how a change there went.
    Owning(owner::Asked),
    /// What Vault says opens the drive now.
    Unlocking(Result<unlocking::Picture, String>),
    /// A field or a button in the Unlocking part of that page, or how a change there went.
    Unlock(unlocking::Asked),
    /// What localed says about the keyboard now.
    Layouts(Result<Region, String>),
    /// A layout was added, taken off or put first, or the shortcuts were asked for.
    Keyboard(keyboard::Asked),
    /// The mice and the touchpads plugged in now.
    Devices(Result<Vec<Device>, String>),
    /// A setting of the mouse or the touchpad changed, or one of its sliders moved.
    Pointer(pointer::Changed),
    /// The switch of the screen reader or the on-screen keyboard.
    Turn(Tool, bool),
    /// The Wi-Fi switch.
    Wifi(bool),
    /// The network at this place in the list was pressed.
    Join(usize),
    /// The password field was typed into.
    Password(String),
    /// The password was given.
    Joined,
    /// The card was asked to sweep for networks again.
    Scanned,
    /// The Bluetooth switch.
    Power(bool),
    /// The paired device at this place in the list was pressed.
    Device(usize),
    /// How a join, a switch or a connect went.
    Acted(Result<(), String>),
    /// What Orbit answered after something about this machine was written, a screen's size or the
    /// size of model it runs: the machine again, or why not.
    Orbit(Result<Host, String>),
    /// What Vault answered about the boot style, after reading it or after writing it.
    BootStyle(Result<Style, String>),
    /// The corner radius slider moved.
    Radius(u32),
    /// A slider was let go, so the look is written.
    Wrote,
    /// The terminal greeting.
    Greeting(bool),
    /// Whether the apps that were open come back at the next login.
    Restore(bool),
    /// What Orbit answered about this machine.
    Host(Result<Host, String>),
    /// A line from the socket.
    Said(Command),
    /// The picture of the window `--screenshot` asked for.
    Shot(window::Screenshot),
    /// The window was asked to close.
    Close,
}

/// Open the window and run until it is closed.
///
/// # Errors
///
/// When the window cannot be opened.
pub fn run(start: Start) -> iced::Result {
    let whole_page = start.screenshot.is_some();
    iced::application(move || boot(&start), update, view)
        .title("Settings")
        .theme(|state: &Settings| {
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
        .scale_factor(|state: &Settings| {
            f32::from(u16::try_from(state.look.text).unwrap_or(100)) / 100.0
        })
        .default_font(FONT)
        .settings(iced::Settings {
            id: Some(APP_ID.to_string()),
            default_font: FONT,
            default_text_size: TEXT_SIZE.into(),
            ..iced::Settings::default()
        })
        .window(window(whole_page))
        .run()
}

/// The window itself. On Wayland the app id comes from the platform settings and nowhere else, and
/// it is the name of the desktop entry, which is how the dock and the compositor know the window.
/// A window that is only there to have its picture taken is as tall as a whole page, so the
/// picture holds one without anyone scrolling it.
fn window(screenshot: bool) -> window::Settings {
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut settings = window::Settings {
        size: Size::new(920.0, if screenshot { 1200.0 } else { 660.0 }),
        min_size: Some(Size::new(600.0, 420.0)),
        exit_on_close_request: false,
        // the app draws its own title bar, the way the GTK apps of the session do. left to itself
        // winit draws an Adwaita frame of its own around the window, in its own colours
        decorations: false,
        ..window::Settings::default()
    };
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = APP_ID.to_string();
    }
    settings
}

fn boot(start: &Start) -> (Settings, Task<Message>) {
    let page = start.page.unwrap_or(Page::FIRST);
    // the pages that name apps read the desktop entries as they come up, and the Apps page the
    // lists of default apps, which are as small
    let entries = if matches!(
        page,
        Page::Dock | Page::Notifications | Page::Apps | Page::Privacy
    ) {
        librift::apps::load()
    } else {
        Vec::new()
    };
    let state = Settings {
        page,
        look: Look::read(),
        greeting: librift::appearance::greeting(),
        restore: librift::session::restores(),
        boot: None,
        choices: librift::wallpaper::choices(),
        zones: librift::clock::installed(),
        layouts: librift::keyboard::installed(),
        pointer: Pointer::read(),
        host: None,
        release: about::release(),
        // Lens's notes are read at once, so a switch is never drawn off while its program runs, and
        // the same for the dock's files and the notification settings, which are as small
        access: (page == Page::Accessibility).then(accessibility::running),
        defaults: (page == Page::Apps).then(|| apps::reading(&entries)),
        apps: entries,
        dock: (page == Page::Dock).then(dock::reading),
        notices: (page == Page::Notifications).then(notifications::reading),
        problem: None,
        screenshot: start.screenshot.clone(),
        ..Settings::bare()
    };
    let mut work = vec![about::ask_orbit(), ai::ask()];
    // in a Ghost boot persist stays locked and no part of the drive is mounted, so the pages that
    // would ask Vault for what the drive keeps say the mode instead and ask nothing (ADR-0084)
    if !ghost::on() {
        work.push(appearance::ask_vault());
    }
    if state.page == Page::Search {
        work.push(search::read());
    }
    if state.page == Page::Backups && !ghost::on() {
        work.push(backups::read());
    }
    if state.page == Page::Updates && !ghost::on() {
        work.push(updates::read());
    }
    if state.page == Page::Region {
        work.push(region::read());
    }
    if state.page == Page::Keyboard {
        work.push(keyboard::read());
    }
    if state.page == Page::Privacy {
        work.push(privacy::ask_fwupd());
    }
    if state.page == Page::Owner && !ghost::on() {
        work.push(owner::read());
        work.push(unlocking::read());
    }
    if start.screenshot.is_some() {
        work.push(shoot());
    }
    (state, Task::batch(work))
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

impl Settings {
    /// A window that has asked for nothing yet, which the pages' own tests build on.
    #[must_use]
    pub fn bare() -> Self {
        Self {
            page: Page::FIRST,
            look: Look::default(),
            greeting: false,
            restore: true,
            boot: None,
            choices: Vec::new(),
            host: None,
            network: None,
            bluetooth: None,
            sound: None,
            moving: None,
            battery: None,
            quasar: None,
            index: None,
            indexing: false,
            snapshots: None,
            disk: None,
            making: backups::Making::default(),
            slots: None,
            clock: None,
            zones: Vec::new(),
            finding: String::new(),
            layouts: Vec::new(),
            keyboard: None,
            pointer: Pointer::default(),
            devices: None,
            region: None,
            printers: None,
            access: None,
            apps: Vec::new(),
            defaults: None,
            unfolded: None,
            privacy: None,
            security: None,
            owner: None,
            signing: owner::Form::default(),
            unlocking: None,
            unlocking_form: unlocking::Form::default(),
            dock: None,
            notices: None,
            joining: None,
            doing: None,
            swept: false,
            release: String::new(),
            problem: None,
            screenshot: None,
        }
    }

    /// The colours the window is drawn in.
    pub fn colors(&self) -> Colors {
        colors(self.look.theme, self.look.accent)
    }

    /// The line `rift-settings --state` prints for each setting.
    fn state(&self) -> String {
        let look = &self.look;
        [
            format!("page {}", self.page.word()),
            format!("theme {}", look.theme.word()),
            format!("accent {}", look.accent.word()),
            format!("wallpaper {}", look.wallpaper),
            format!("gaps {}", look.gaps),
            format!("radius {}", look.radius),
            format!("text {}", look.text),
            format!("terminal {}", look.terminal.word()),
            format!("greeting {}", if self.greeting { "on" } else { "off" }),
            format!(
                "session-restore {}",
                if self.restore { "on" } else { "off" }
            ),
        ]
        .into_iter()
        // the boot style is Vault's to answer, and the screens are Orbit's, so each is printed
        // once it has
        .chain(
            self.boot
                .as_ref()
                .and_then(|answered| answered.as_ref().ok())
                .map(|style| format!("boot {}", style.word())),
        )
        .chain(
            self.host
                .as_ref()
                .and_then(|answered| answered.as_ref().ok())
                .into_iter()
                .flat_map(|host| {
                    host.outputs.iter().map(|output| {
                        format!(
                            "screen {} {}x{} scale {}",
                            output.connector, output.width, output.height, output.scale
                        )
                    })
                }),
        )
        // and the same for the network and Bluetooth, which the window follows while it is open
        .chain(net::state(self))
        .chain(bluetooth::state(self))
        .chain(sound::state(self))
        .chain(power::state(self))
        .chain(ai::state(self))
        .chain(search::state(self))
        .chain(backups::state(self))
        .chain(updates::state(self))
        .chain(datetime::state(self))
        .chain(region::state(self))
        .chain(printers::state(self))
        .chain(accessibility::state(self))
        .chain(keyboard::state(self))
        .chain(pointer::state(self))
        .chain(dock::state(self))
        .chain(notifications::state(self))
        .chain(apps::state(self))
        .chain(privacy::state(self))
        .chain(owner::state(self))
        .chain(unlocking::state(self))
        // what went wrong last, which the page shows in red under everything else
        .chain(self.problem.as_ref().map(|why| format!("problem {why}")))
        .collect::<Vec<_>>()
        .join("\n")
            + "\n"
    }

    /// Write the look the owner has just changed, hand it to the apps and the desktop, and tell
    /// the shell to draw with it.
    fn wrote(&mut self) {
        self.problem = self.look.save().err();
        poke_the_shell();
    }
}

/// Tell the shell that is running to read the appearance settings again. It draws the bar, the
/// dock and the menus, so the accent and the theme reach it this way rather than through a file it
/// would have to watch.
fn poke_the_shell() {
    let _ = std::process::Command::new("lens")
        .arg("--look")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// What the owner asked for: a page, a press, a switch or a slider, from the window or from the
/// socket. What a service answers back is in `answered`.
fn update(state: &mut Settings, message: Message) -> Task<Message> {
    match message {
        Message::Show(page) => return show(state, page),
        Message::Mode(mode) => {
            state.look.theme = mode;
            state.wrote();
        }
        Message::Accent(accent) => {
            state.look.accent = accent;
            state.wrote();
        }
        Message::Wallpaper(at) => {
            if let Some(choice) = state.choices.get(at) {
                state.look.wallpaper = choice.wallpaper.clone();
                state.wrote();
            }
        }
        Message::Gaps(gaps) => state.look.gaps = gaps,
        Message::Radius(radius) => state.look.radius = radius,
        Message::Text(text) => state.look.text = text,
        Message::Terminal(scheme) => {
            state.look.terminal = scheme;
            state.wrote();
        }
        Message::Boot(style) => return appearance::write_style(style),
        Message::Scale(connector, scale) => return displays::set_scale(connector, scale),
        Message::Tier(tier) => {
            state.problem = None;
            return ai::set_size(tier);
        }
        Message::Index => return search::start(state),
        Message::Snapshot => {
            if state.making.snapshot {
                return Task::none();
            }
            state.problem = None;
            state.making.snapshot = true;
            return backups::take();
        }
        Message::BackUp => {
            if state.making.backup {
                return Task::none();
            }
            state.problem = None;
            state.making.backup = true;
            return backups::back_up();
        }
        Message::Volume(side, level) => state.moving = Some((side, level)),
        Message::Volumed(side, level) => {
            state.problem = None;
            return sound::set_volume(state, side, level);
        }
        Message::Muted(side, muted) => {
            state.problem = None;
            return sound::set_muted(state, side, muted);
        }
        Message::Pick(side, at) => {
            state.problem = None;
            return sound::pick(state, side, at);
        }
        Message::Wifi(on) => {
            state.problem = None;
            return net::set_wifi(state, on);
        }
        Message::Join(at) => return net::join(state, at),
        Message::Password(typed) => {
            if let Some(asked) = state.joining.as_mut() {
                asked.password = typed;
                state.problem = None;
            }
        }
        Message::Joined => return net::joined(state),
        Message::Scanned => {}
        Message::Power(on) => {
            state.problem = None;
            return bluetooth::set_powered(state, on);
        }
        Message::Device(at) => return bluetooth::connect(state, at),
        Message::Find(typed) => state.finding = typed,
        Message::Zone(zone) => return datetime::choose(state, zone),
        Message::Printer(asked) => return printers::asked(state, asked),
        Message::Turn(tool, on) => return accessibility::turn(state, tool, on),
        Message::Keyboard(asked) => return keyboard::asked(state, &asked),
        Message::Dock(asked) => return dock::asked(state, asked),
        Message::Notices(asked) => return notifications::asked(state, &asked),
        Message::Apps(asked) => return apps::asked(state, asked),
        Message::Private(asked) => return privacy::asked(state, asked),
        Message::Owning(asked) => return owner::asked(state, asked),
        Message::Unlock(asked) => return unlocking::asked(state, asked),
        Message::Pointer(changed) => pointer::update(state, changed),
        Message::Wrote => state.wrote(),
        Message::Greeting(_) | Message::Restore(_) => switched(state, &message),
        Message::Close => return iced::exit(),
        answer => return answered(state, answer),
    }
    Task::none()
}

/// A switch of a page whose setting is one small file under home, written where it is turned.
fn switched(state: &mut Settings, message: &Message) {
    match *message {
        Message::Greeting(on) => {
            state.greeting = on;
            state.problem = librift::appearance::set_greeting(on).err();
        }
        Message::Restore(on) => {
            state.restore = on;
            state.problem = librift::session::keep_restores(on).err();
        }
        _ => {}
    }
}

/// What a service, the socket or a write that has finished answered. It is the other half of
/// `update`: what the owner does is above, what the machine says back is here.
fn answered(state: &mut Settings, message: Message) -> Task<Message> {
    match message {
        Message::Network(picture) => {
            state.network = Some(*picture);
            // a network that has come up is no longer one waiting for a password
            if state
                .joining
                .as_ref()
                .is_some_and(|asked| on_network(state, asked.network.ssid.as_slice()))
            {
                state.joining = None;
                state.doing = None;
            }
            if state.page == Page::Wifi && !state.swept {
                state.swept = true;
                return net::scan(state);
            }
        }
        Message::Bluetooth(answer) => state.bluetooth = Some(answer),
        Message::Sound(answer) => {
            state.sound = Some(*answer);
            // the slider follows PipeWire again, now that it says what the level is
            state.moving = None;
        }
        Message::Battery(answer) => state.battery = Some(answer),
        Message::Quasar(answer) => state.quasar = Some(answer),
        Message::Indexed(answer) => state.index = Some(answer),
        Message::Updated(said) => {
            state.indexing = false;
            state.problem = said.err();
        }
        Message::Snapshots(answer) => state.snapshots = Some(answer),
        Message::Slots(answer) => state.slots = Some(answer),
        Message::Clock(reading) => state.clock = Some(*reading),
        Message::Region(picture) => state.region = Some(picture),
        Message::Printers(answer) => state.printers = Some(answer),
        Message::Access(running) => state.access = Some(running),
        Message::Docked(picture) => state.dock = Some(picture),
        Message::Noticed(picture) => state.notices = Some(picture),
        Message::Kinds(picture) => state.defaults = Some(picture),
        Message::Privacy(picture) => state.privacy = Some(picture),
        Message::Security(answer) => state.security = Some(answer),
        Message::Owner(answer) => state.owner = Some(answer),
        Message::Unlocking(answer) => state.unlocking = Some(answer),
        Message::Layouts(answer) => state.keyboard = Some(answer),
        Message::Devices(answer) => state.devices = Some(answer),
        Message::Backups(answer) => state.disk = Some(answer),
        Message::Took(said) => {
            state.making.snapshot = false;
            state.problem = said.err();
        }
        Message::BackedUp(said) => {
            state.making.backup = false;
            state.problem = said.err();
        }
        Message::Acted(Ok(())) => {
            state.problem = None;
            state.doing = None;
            if let Some(asked) = state.joining.as_mut() {
                asked.busy = false;
            }
        }
        Message::Acted(Err(why)) => {
            state.doing = None;
            state.problem = Some(why);
            if let Some(asked) = state.joining.as_mut() {
                asked.busy = false;
                return crate::widgets::focus(net::FIELD);
            }
        }
        Message::Orbit(Ok(host)) => {
            state.problem = None;
            state.host = Some(Ok(host));
        }
        Message::Orbit(Err(why)) => state.problem = Some(why),
        Message::BootStyle(answer) => state.boot = Some(answer),
        Message::Host(host) => state.host = Some(host),
        Message::Said(command) => return said(state, command),
        Message::Shot(shot) => {
            if let Some(path) = &state.screenshot
                && let Err(why) = save(path, &shot)
            {
                eprintln!("rift-settings: {why}");
            }
            return iced::exit();
        }
        // everything else is the owner's, and `update` has it
        _ => {}
    }
    Task::none()
}

/// Show a page. What went wrong on the page that was up, and what it was doing, belong to that
/// page and are left behind. The Wi-Fi page asks the card to sweep as it comes up, so the list is
/// what is around now rather than what was around when the window opened; leaving it drops a
/// password half typed, a search for a time zone, and what was typed on the Owner page.
fn show(state: &mut Settings, page: Page) -> Task<Message> {
    if state.page == page {
        return Task::none();
    }
    state.page = page;
    state.problem = None;
    state.doing = None;
    state.swept = false;
    state.joining = None;
    state.finding.clear();
    state.unfolded = None;
    state.signing = owner::Form::default();
    match page {
        Page::Wifi => {
            state.swept = true;
            net::scan(state)
        }
        // the index is a file on the drive that a timer writes too, so it is read again every time
        // the page comes up rather than once when the window opened
        Page::Search => search::read(),
        // the same for the snapshots, which a timer takes every hour, and the backups, which are
        // on a disk the window would rather not mount before anyone asks for them
        Page::Backups if !ghost::on() => backups::read(),
        // and for the slots, since reading them mounts the esp
        Page::Updates if !ghost::on() => updates::read(),
        // the language and the formats are part of the image, and asking for them runs programs.
        // the clock and the printers are read by subscriptions of their own, which ask again while
        // their page is up
        Page::Region => region::read(),
        // the layouts are read again after every change the page makes
        Page::Keyboard => keyboard::read(),
        // Lens's notes are two small files, read at once so a switch is never drawn off while its
        // program runs, and again every second by a subscription while the page is up
        Page::Accessibility => {
            state.access = Some(accessibility::running());
            Task::none()
        }
        // the same for the dock's files and the notification settings, and the apps the two pages
        // name, which are a walk of a few folders
        Page::Dock => {
            state.apps = librift::apps::load();
            state.dock = Some(dock::reading());
            Task::none()
        }
        Page::Notifications => {
            state.apps = librift::apps::load();
            state.notices = Some(notifications::reading());
            Task::none()
        }
        // the lists of default apps are a few small files; the camera's answers, the firewall and
        // the sandboxes are asked on the subscription's thread, and fwupd once on a thread of its
        // own, since it starts for the question
        Page::Apps => {
            state.apps = librift::apps::load();
            state.defaults = Some(apps::reading(&state.apps));
            Task::none()
        }
        Page::Privacy => {
            state.apps = librift::apps::load();
            privacy::ask_fwupd()
        }
        // the owner's name and password are Vault's to answer, and nothing changes them while the
        // page is up but the page itself, which asks again after each change
        Page::Owner if !ghost::on() => Task::batch([owner::read(), unlocking::read()]),
        _ => Task::none(),
    }
}

/// Whether the machine is on the network with this name as the radio sends it.
fn on_network(state: &Settings, ssid: &[u8]) -> bool {
    state
        .network
        .as_ref()
        .and_then(|answered| answered.as_ref().ok())
        .and_then(|picture| picture.wireless.as_ref())
        .and_then(librift::network::Wireless::active)
        .is_some_and(|network| network.ssid == ssid)
}

/// A line from the socket. Setting something over it does what pressing it on the page does.
fn said(state: &mut Settings, command: Command) -> Task<Message> {
    match command {
        Command::Page(word) => {
            if let Some(page) = Page::from_word(&word) {
                return show(state, page);
            }
        }
        Command::Set(name, value) => return set(state, &name, &value),
        // answered on the socket's own thread
        Command::State => {}
    }
    Task::none()
}

fn set(state: &mut Settings, name: &str, value: &str) -> Task<Message> {
    let number = |most: u32| value.trim().parse::<u32>().ok().map(|n| n.min(most));
    let on = |value: &str| !value.trim().eq_ignore_ascii_case("off");
    match name {
        "theme" => Task::done(Message::Mode(Mode::from_setting(value))),
        "accent" => Task::done(Message::Accent(Accent::from_setting(value))),
        "gaps" => number(librift::appearance::GAPS_MOST).map_or_else(Task::none, |gaps| {
            state.look.gaps = gaps;
            Task::done(Message::Wrote)
        }),
        "radius" => number(librift::appearance::RADIUS_MOST).map_or_else(Task::none, |radius| {
            state.look.radius = radius;
            Task::done(Message::Wrote)
        }),
        "text" => number(librift::appearance::TEXT_MOST).map_or_else(Task::none, |text| {
            state.look.text = text.max(librift::appearance::TEXT_LEAST);
            Task::done(Message::Wrote)
        }),
        "terminal" => Task::done(Message::Terminal(Scheme::from_setting(value))),
        "boot" => Task::done(Message::Boot(Style::from_setting(value))),
        // the screen is named first, then the size: `--set scale eDP-1 2`
        "scale" => value
            .trim()
            .rsplit_once(' ')
            .and_then(|(screen, size)| Some((screen.trim().to_string(), size.trim().parse().ok()?)))
            .map_or_else(Task::none, |(screen, scale)| {
                Task::done(Message::Scale(screen, scale))
            }),
        "greeting" => Task::done(Message::Greeting(!value.trim().eq_ignore_ascii_case("off"))),
        "tier" => ai::named(value).map_or_else(Task::none, |tier| Task::done(Message::Tier(tier))),
        // a zone by its name in the database, which timedated checks: `--set timezone Europe/London`
        "timezone" => Task::done(Message::Zone(value.trim().to_string())),
        // a printer by the name of its queue and a job by its number, which CUPS checks
        name if printers::NAMES.contains(&name) => printers::named(name, value)
            .map_or_else(Task::none, |asked| Task::done(Message::Printer(asked))),
        "screen-reader" => Task::done(Message::Turn(Tool::Reader, on(value))),
        "on-screen-keyboard" => Task::done(Message::Turn(Tool::Keyboard, on(value))),
        // a layout by its word, `gb` or `us(dvorak)`, which the list of layouts checks
        "add-layout" => Task::done(Message::Keyboard(keyboard::Asked::Add(
            value.trim().to_string(),
        ))),
        "remove-layout" => Task::done(Message::Keyboard(keyboard::Asked::Remove(
            value.trim().to_string(),
        ))),
        "first-layout" => Task::done(Message::Keyboard(keyboard::Asked::First(
            value.trim().to_string(),
        ))),
        "shortcuts" => Task::done(Message::Keyboard(keyboard::Asked::Shortcuts)),
        // the dock's settings by their words and its apps by their ids, and Do not disturb and an
        // app's banners, which the pages check
        name if dock::NAMES.contains(&name) => dock::named(state, name, value)
            .map_or_else(Task::none, |asked| Task::done(Message::Dock(asked))),
        "do-not-disturb" | "app-banners" => notifications::named(state, name, value)
            .map_or_else(Task::none, |asked| Task::done(Message::Notices(asked))),
        // a kind's default app, and the camera's answers, recent files and the sandboxes' network
        name if apps::NAMES.contains(&name) => apps::named(state, name, value)
            .map_or_else(Task::none, |asked| Task::done(Message::Apps(asked))),
        name if privacy::NAMES.contains(&name) => privacy::named(state, name, value)
            .map_or_else(Task::none, |asked| Task::done(Message::Private(asked))),
        // the owner's name, and the current password and a new one, each typed and pressed
        name if owner::NAMES.contains(&name) => owner::named(name, value),
        // the drive's passphrase for a key sealed to this machine's tpm, and a security key by the
        // keyslot the page prints
        "auto-unlock" | "remove-key" => unlocking::named(name, value),
        // the mouse and the touchpad, by the names their file has, for a device this machine has
        name if librift::pointer::NAMES.contains(&name) => pointer::named(state, name, value)
            .map_or_else(Task::none, |chosen| {
                Task::done(Message::Pointer(pointer::Changed::Chose(chosen)))
            }),
        // the value is there to be typed, the way a switch takes on or off: there is one thing to do
        "index" => Task::done(Message::Index),
        "snapshot" => Task::done(Message::Snapshot),
        "backup" => Task::done(Message::BackUp),
        "wifi" => Task::done(Message::Wifi(on(value))),
        "bluetooth" => Task::done(Message::Power(on(value))),
        // moved, then let go, in that order, the way the slider itself does it
        "volume" | "input-volume" => {
            let side = if name == "volume" {
                Side::Output
            } else {
                Side::Input
            };
            number(100).map_or_else(Task::none, |level| {
                Task::done(Message::Volume(side, level))
                    .chain(Task::done(Message::Volumed(side, level)))
            })
        }
        "mute" => Task::done(Message::Muted(Side::Output, on(value))),
        "input-mute" => Task::done(Message::Muted(Side::Input, on(value))),
        "output" => sound::named(state, Side::Output, value)
            .map_or_else(Task::none, |at| Task::done(Message::Pick(Side::Output, at))),
        "input" => sound::named(state, Side::Input, value)
            .map_or_else(Task::none, |at| Task::done(Message::Pick(Side::Input, at))),
        // a network and a paired device are named, since neither list is in an order anyone typed
        "join" => {
            net::named(state, value).map_or_else(Task::none, |at| Task::done(Message::Join(at)))
        }
        "connect" => bluetooth::named(state, value)
            .map_or_else(Task::none, |at| Task::done(Message::Device(at))),
        // typed, then given, in that order, which a batch does not promise
        "password" => {
            Task::done(Message::Password(value.to_string())).chain(Task::done(Message::Joined))
        }
        "wallpaper" => state
            .choices
            .iter()
            .position(|choice| choice.names(value))
            .map_or_else(Task::none, |at| Task::done(Message::Wallpaper(at))),
        _ => Task::none(),
    }
}

fn subscription(state: &Settings) -> Subscription<Message> {
    let mut followed = vec![
        window::close_requests().map(|_| Message::Close),
        terminal(),
        watch::network(),
        watch::bluetooth(),
        watch::sound(),
        watch::battery(),
        watch::quasar(),
    ];
    // the clock turns with the minute, the printers and the jobs are asked for every two seconds,
    // whether the screen reader and the keyboard are running every second, which mice and touchpads
    // are plugged in every two seconds, the dock's files and the notification settings every
    // second, and the default apps and the privacy settings every two seconds, each only while its
    // page is up
    match state.page {
        Page::DateTime => followed.push(datetime::ticking()),
        Page::Printers => followed.push(printers::following()),
        Page::Accessibility => followed.push(accessibility::following()),
        Page::Pointer => followed.push(pointer::following()),
        Page::Dock => followed.push(dock::following()),
        Page::Notifications => followed.push(notifications::following()),
        Page::Apps => followed.push(apps::following()),
        Page::Privacy => followed.push(privacy::following()),
        _ => {}
    }
    Subscription::batch(followed)
}

/// The socket in the runtime directory, read on a thread of its own. The state query is answered
/// there, from the lines the window keeps up to date. The subscription is named, because iced tells
/// two of them apart by the type of the stream and the address of the function that makes it.
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
                eprintln!("rift-settings: {why}");
            }
        });
        receiver
    })
}

/// What the socket answers a state query with. The view writes it on every frame, so the thread
/// that answers never touches the window's own state.
fn kept() -> String {
    STATE.lock().map_or_else(
        |_| "the window is busy\n".to_string(),
        |state| state.clone(),
    )
}

static STATE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

fn view(state: &Settings) -> Element<'_, Message> {
    if let Ok(mut kept) = STATE.lock() {
        *kept = state.state();
    }
    let look = state.colors();
    let body = row![
        sidebar(state, look),
        container(page(state, look))
            .width(Fill)
            .height(Fill)
            .style(move |_: &Theme| fill(look.page)),
    ]
    .height(Fill);
    column![header(state, look), body].into()
}

/// The title bar the app draws for itself: the name of the app over the sidebar, the name of the
/// page over the page, and the close button at the right end, the way every GTK app of the session
/// draws its own.
fn header(state: &Settings, look: Colors) -> Element<'_, Message> {
    let name = container(text("Settings").size(TEXT_SIZE).font(BOLD).color(look.text))
        .width(Length::Fixed(SIDEBAR))
        .padding([0, 14])
        .center_y(Fill);
    let title = container(
        text(state.page.label())
            .size(TEXT_SIZE)
            .font(BOLD)
            .color(look.text),
    )
    .width(Fill)
    .center_x(Fill)
    .center_y(Fill);
    let close = button(icons::symbolic(look.text, "window-close-symbolic", 16.0))
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
        container(row![name, title, close].align_y(Center).padding([0, 8]))
            .width(Fill)
            .height(Length::Fixed(HEADER))
            .style(move |_: &Theme| fill(look.header)),
        line,
    ]
    .into()
}

/// The pages, one row each, the one that is up in the accent.
fn sidebar(state: &Settings, look: Colors) -> Element<'_, Message> {
    let mut rows = column![].width(Fill).spacing(2).padding([8, 8]);
    for page in Page::ALL {
        let here = page == state.page;
        let colour = if here { look.on_accent } else { look.text };
        // a button lays its content out at the top of its box, so the row is centred by hand, the
        // way the shell's bar centres its buttons
        rows = rows.push(
            button(
                container(
                    row![
                        icons::symbolic(colour, page.icon(), 16.0),
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
    // in a Ghost boot every setting the window shows is the image's own, not the owner's, because
    // what the owner chose is under home and home is memory here. That is one fact about the
    // window and not about a page, so it is said once, at the foot of the sidebar, in the two
    // words the bar uses and the short of what they mean (ADR-0084)
    let side: Element<'_, Message> = if ghost::on() {
        column![
            scroll(look, rows).height(Fill),
            container(
                column![
                    text(librift::ghost::NAME).size(TEXT_SIZE).color(look.text),
                    text(librift::ghost::SHORT).size(TEXT_SIZE).color(look.dim),
                ]
                .spacing(2)
            )
            .padding([10, 18]),
        ]
        .into()
    } else {
        scroll(look, rows).height(Fill).into()
    };
    row![
        container(side)
            .width(Length::Fixed(SIDEBAR))
            .height(Fill)
            .style(move |_: &Theme| fill(look.side)),
        container(space().width(1.0).height(Fill)).style(move |_: &Theme| fill(look.line)),
    ]
    .into()
}

/// The page that is up.
fn page(state: &Settings, look: Colors) -> Element<'_, Message> {
    let inside = match state.page {
        Page::Wifi => net::wifi(state, look),
        Page::Network => net::wired(state, look),
        Page::Bluetooth => bluetooth::view(state, look),
        Page::Sound => sound::view(state, look),
        Page::Power => power::view(state, look),
        Page::Ai => ai::view(state, look),
        Page::Search => search::view(state, look),
        Page::Backups => backups::view(state, look),
        Page::Updates => updates::view(state, look),
        Page::DateTime => datetime::view(state, look),
        Page::Region => region::view(state, look),
        Page::Printers => printers::view(state, look),
        Page::Accessibility => accessibility::view(state, look),
        Page::Keyboard => keyboard::view(state, look),
        Page::Pointer => pointer::view(state, look),
        Page::Dock => dock::view(state, look),
        Page::Notifications => notifications::view(state, look),
        Page::Apps => apps::view(state, look),
        Page::Privacy => privacy::view(state, look),
        Page::Appearance => appearance::view(state, look),
        Page::Displays => displays::view(state, look),
        Page::About => about::view(state, look),
        Page::Owner => owner::view(state, look),
    };
    scroll(
        look,
        container(inside).width(Fill).padding(crate::widgets::PAD),
    )
    .height(Fill)
    .into()
}
