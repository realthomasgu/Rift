//! Names, D-Bus addresses and paths shared by every Rift component, the OS commands Lens and
//! the rift command run, the client side of the services on the system bus (Rift's own, and
//! `NetworkManager`, `BlueZ`, `UPower` and logind for the system menu, timedated and localed for
//! the clock, the language and the keyboard layouts), the printers CUPS has a queue for, the apps
//! the desktop entries name and where their icons are, which app opens each kind of file, what
//! the camera portal and the firewall allow, what the dock keeps and where it stands, Do
//! not disturb and the apps whose banners stay off, whether this boot is a Ghost one, which of the
//! programs the shell's keys start is
//! running, the index for search by meaning, what the system calls itself, dark or light and the
//! wallpaper, the mouse and the touchpad, the sound `PipeWire` plays and hears, how the boot looks,
//! the owner's account, the apps Flatpak installs and the ones Rift suggests, what one of them
//! asks for before it is installed, what is in a folder
//! and what kind of file each thing is, the trash, and how a drive is written.
//!
//! Apache-2.0 so other people can embed it. Keep it dependency free: only the `bus` feature,
//! which the asking side of the bus turns on, brings in zbus, only the `disk` feature serde and
//! getrandom, and only the `models` and `suggested` features toml.

pub mod access;
pub mod airlock;
pub mod appearance;
// desktop entries and icon themes are how a freedesktop session finds its apps, which rift-flash
// has no use for when it is built for Windows
#[cfg(unix)]
pub mod apps;
pub mod battery;
pub mod bluetooth;
pub mod boot;
pub mod bus;
pub mod clock;
#[cfg(unix)]
pub mod defaults;
#[cfg(feature = "disk")]
pub mod disk;
pub mod dock;
// the disks a person plugs in and the exchange partition of the drive, which are a freedesktop
// session's too
#[cfg(unix)]
pub mod drives;
// folders, the kinds of file and the trash are a freedesktop session's, which rift-flash has no
// use for on Windows
#[cfg(unix)]
pub mod files;
// flatpak is how a freedesktop session installs apps, which rift-flash has no use for either
#[cfg(unix)]
pub mod flatpak;
pub mod ghost;
#[cfg(unix)]
pub mod icons;
pub mod keyboard;
#[cfg(feature = "models")]
pub mod models;
pub mod network;
pub mod notifications;
pub mod orbit;
pub mod os;
// the owner's account is a unix one, and so are its password files
#[cfg(unix)]
pub mod owner;
// what a flatpak app asks for is read out of its metadata, which is plain text wherever it is read
pub mod permissions;
pub mod pointer;
#[cfg(unix)]
pub mod printers;
// the camera portal, dconf and the firewall's unit belong to a freedesktop session too
#[cfg(unix)]
pub mod privacy;
pub mod quasar;
pub mod region;
pub mod release;
pub mod search;
// the journal of what was open is a journal of desktop entries, which is the apps module,
// and the lock is logind's, so neither means anything on Windows
#[cfg(unix)]
pub mod session;
pub mod sound;
#[cfg(feature = "suggested")]
pub mod suggested;
pub mod time;
pub mod update;
pub mod vault;
pub mod wallpaper;

/// Version of the Rift workspace this crate was built from.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Namespace every Rift D-Bus service lives under.
pub const DBUS_PREFIX: &str = "dev.rift";

/// The first-party components, in the order they come up during boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Component {
    /// The immutable system image and the boot sequence.
    Liftoff,
    /// Host detection, adaptation, and per-machine memory.
    Orbit,
    /// The Wayland compositor.
    Horizon,
    /// The omnibar shell.
    Lens,
    /// The local AI service.
    Quasar,
    /// App sandboxing and permissions.
    Airlock,
    /// Snapshots, backups, and drive cloning.
    Vault,
}

impl Component {
    /// Every component, in boot order.
    pub const ALL: [Component; 7] = [
        Component::Liftoff,
        Component::Orbit,
        Component::Horizon,
        Component::Lens,
        Component::Quasar,
        Component::Airlock,
        Component::Vault,
    ];

    /// Lower-case name, as used in paths, unit names, and logs.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Component::Liftoff => "liftoff",
            Component::Orbit => "orbit",
            Component::Horizon => "horizon",
            Component::Lens => "lens",
            Component::Quasar => "quasar",
            Component::Airlock => "airlock",
            Component::Vault => "vault",
        }
    }

    /// Capitalised name, as shown to people.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Component::Liftoff => "Liftoff",
            Component::Orbit => "Orbit",
            Component::Horizon => "Horizon",
            Component::Lens => "Lens",
            Component::Quasar => "Quasar",
            Component::Airlock => "Airlock",
            Component::Vault => "Vault",
        }
    }

    /// Well-known D-Bus bus name, for example `dev.rift.Quasar`.
    #[must_use]
    pub fn dbus_name(self) -> String {
        format!("{DBUS_PREFIX}.{}", self.display_name())
    }

    /// D-Bus object path, for example `/dev/rift/Quasar`.
    #[must_use]
    pub fn dbus_path(self) -> String {
        format!("/{}/{}", DBUS_PREFIX.replace('.', "/"), self.display_name())
    }
}

/// Where things live on a running Rift system. All of these are on the persist partition,
/// except what the image itself installs under /etc.
pub mod paths {
    /// The model manifest, installed by the image.
    pub const MODEL_MANIFEST: &str = "/etc/rift/models.toml";
    /// Top level of the persist volume.
    pub const PERSIST: &str = "/persist";
    /// GGUF weights and voices (`@models`).
    pub const MODELS: &str = "/var/lib/rift/models";
    /// Per-host profiles written by Orbit (`@hosts`).
    pub const HOSTS: &str = "/var/lib/rift/hosts";
    /// Quasar's index and action log.
    pub const QUASAR_STATE: &str = "/var/lib/rift/quasar";
    /// Which apps have their network off, kept by Airlock.
    pub const AIRLOCK_STATE: &str = "/var/lib/rift/airlock";
    /// The Flatpak installation every app the Store and Welcome install goes into. Its
    /// `overrides` folder is the other half of Airlock's network switch.
    pub const FLATPAK_SYSTEM: &str = "/var/lib/flatpak";
    /// Phase marker written by the image.
    pub const PHASE: &str = "/etc/rift/phase";
    /// The logo in characters, without its colours, installed by the image.
    pub const LOGO: &str = "/etc/rift/logo.txt";
    /// The same logo with its colours as terminal escape sequences.
    pub const LOGO_ANSI: &str = "/etc/rift/logo.ansi";
    /// The mark, the line drawing of the black hole, as a picture. The About page and Welcome
    /// draw it.
    pub const LOGO_MARK: &str = "/etc/rift/logo.png";
    /// The apps Rift suggests, installed by the image.
    pub const SUGGESTED_APPS: &str = "/etc/rift/apps.toml";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dbus_names_follow_the_prefix() {
        assert_eq!(Component::Quasar.dbus_name(), "dev.rift.Quasar");
        assert_eq!(Component::Quasar.dbus_path(), "/dev/rift/Quasar");
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = Component::ALL.iter().map(|c| c.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Component::ALL.len());
    }
}
