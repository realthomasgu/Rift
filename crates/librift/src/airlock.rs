//! Airlock from a client's side: what an app is called, the systemd scope each of its sandboxes
//! runs in, and the network switch on the system bus.
//!
//! Every sandbox runs in a scope of the owner's user manager named
//! `app-airlock-<app>-<number>.scope`, and flatpak starts each of its apps in one named
//! `app-flatpak-<id>-<number>.scope` beside it, so Airlock finds what an app runs in by its scopes
//! and cuts their network by their cgroups. An app id fits the name rule as it is.

#[cfg(feature = "bus")]
use std::time::Duration;

#[cfg(feature = "bus")]
use crate::{Component, bus};

/// How long turning an app's network off or on may take. Airlock runs nft, which is quick.
#[cfg(feature = "bus")]
const SWITCH_TIMEOUT: Duration = Duration::from_secs(30);

/// How the unit name of the scope of every sandbox starts.
pub const SCOPE_PREFIX: &str = "app-airlock-";

/// How the unit name of the scope flatpak starts each of its apps in begins.
pub const FLATPAK_PREFIX: &str = "app-flatpak-";

/// The longest name an app can have.
pub const NAME_MAX: usize = 64;

/// Why `name` cannot be the name of an app, or `None` when it can. A name is letters, digits, dots,
/// dashes and underscores, and starts with a letter or a digit, so it fits in a unit name as it is.
#[must_use]
pub fn name_problem(name: &str) -> Option<String> {
    let plain = name.len() <= NAME_MAX
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    (!plain).then(|| {
        format!(
            "\"{name}\" cannot be the name of an app. A name has up to {NAME_MAX} letters, digits, \
             dots, dashes and underscores, and starts with a letter or a digit."
        )
    })
}

/// The unit name of the scope one sandbox of `app` runs in. `number` tells two sandboxes of the
/// same app apart: `airlock run` gives its process id.
#[must_use]
pub fn scope_unit(app: &str, number: u32) -> String {
    format!("{SCOPE_PREFIX}{app}-{number}.scope")
}

/// The app that runs in the scope with this unit name and whether it is a Flatpak app rather than
/// a sandbox, or `None` when the scope is neither's.
#[must_use]
pub fn app_of_scope(unit: &str) -> Option<(&str, bool)> {
    let (rest, flatpak) = match unit.strip_prefix(SCOPE_PREFIX) {
        Some(rest) => (rest, false),
        None => (unit.strip_prefix(FLATPAK_PREFIX)?, true),
    };
    let (app, number) = rest.strip_suffix(".scope")?.rsplit_once('-')?;
    let numbered = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
    (numbered && name_problem(app).is_none()).then_some((app, flatpak))
}

/// An app as the network switch sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Its name, which for a Flatpak app is its id.
    pub name: String,
    /// Whether it has the network.
    pub network: bool,
    /// How many of its sandboxes run now.
    pub running: u32,
    /// Whether it is a Flatpak app: one the system installation has, or one running in a scope
    /// flatpak started. The others are named after the command `rift run --sandbox` was given.
    pub flatpak: bool,
}

/// Every app whose network is off or that runs in a sandbox now, by name.
///
/// # Errors
///
/// A sentence when the bus or Airlock is not there, or Airlock could not read the scopes.
#[cfg(feature = "bus")]
pub fn apps() -> Result<Vec<App>, String> {
    let airlock = Component::Airlock;
    let connection = bus::connect(bus::PROPERTY_TIMEOUT)?;
    let proxy = bus::proxy(&connection, airlock)?;
    let apps: Vec<(String, bool, u32, bool)> = proxy
        .call("List", &())
        .map_err(|e| bus::sentence(airlock, e))?;
    Ok(apps
        .into_iter()
        .map(|(name, network, running, flatpak)| App {
            name,
            network,
            running,
            flatpak,
        })
        .collect())
}

/// Turns the network of `app` on or off, in the sandboxes it runs in now and in every one it starts
/// later. Returns how many of its sandboxes run now.
///
/// # Errors
///
/// A sentence when the bus or Airlock is not there, the name is not an app's, or the switch could
/// not be changed.
#[cfg(feature = "bus")]
pub fn set_network(app: &str, on: bool) -> Result<u32, String> {
    let airlock = Component::Airlock;
    let connection = bus::connect(SWITCH_TIMEOUT)?;
    let proxy = bus::proxy(&connection, airlock)?;
    proxy
        .call("SetNetwork", &(app, on))
        .map_err(|e| bus::sentence(airlock, e))
}

/// What a new sandbox asks from inside its scope before anything runs in it. When the app's
/// network is off, Airlock cuts the scope's before it answers. Returns the app's name and whether
/// it has the network.
///
/// # Errors
///
/// A sentence when the bus or Airlock is not there, this process is not in the scope of a
/// sandbox, or its network could not be cut.
#[cfg(feature = "bus")]
pub fn starting() -> Result<(String, bool), String> {
    let airlock = Component::Airlock;
    let connection = bus::connect(SWITCH_TIMEOUT)?;
    let proxy = bus::proxy(&connection, airlock)?;
    proxy
        .call("Starting", &())
        .map_err(|e| bus::sentence(airlock, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_names_fit_in_a_unit_name() {
        for name in [
            "curl",
            "yt-dlp",
            "python3",
            "org.gnome.Nautilus",
            "a_b",
            "7zip",
        ] {
            assert_eq!(name_problem(name), None, "{name}");
        }
        for name in [
            "",
            "-x",
            ".hidden",
            "a b",
            "no/such",
            "g++",
            "caf\u{e9}",
            &"x".repeat(65),
        ] {
            assert!(name_problem(name).is_some(), "{name}");
        }
        assert_eq!(
            name_problem("a b").as_deref(),
            Some(
                "\"a b\" cannot be the name of an app. A name has up to 64 letters, digits, dots, \
                 dashes and underscores, and starts with a letter or a digit."
            )
        );
    }

    #[test]
    fn a_scope_names_its_app() {
        assert_eq!(scope_unit("yt-dlp", 4711), "app-airlock-yt-dlp-4711.scope");
        assert_eq!(
            app_of_scope("app-airlock-yt-dlp-4711.scope"),
            Some(("yt-dlp", false))
        );
        assert_eq!(
            app_of_scope("app-airlock-curl-1.scope"),
            Some(("curl", false))
        );
        for unit in [
            "app-airlock-curl.scope",
            "app-airlock-curl-.scope",
            "app-airlock-curl-12a.scope",
            "app-airlock--12.scope",
            "app-airlock-curl-12.service",
            "app-org.gnome.Nautilus-12.scope",
            "session-2.scope",
        ] {
            assert_eq!(app_of_scope(unit), None, "{unit}");
        }
    }

    #[test]
    fn a_flatpak_scope_names_its_app_id() {
        // the numbers flatpak gives are its own, not a process id, and the id can hold a dash
        assert_eq!(
            app_of_scope("app-flatpak-dev.rift.TestApp-1885090347.scope"),
            Some(("dev.rift.TestApp", true))
        );
        assert_eq!(
            app_of_scope("app-flatpak-org.kde.foo-bar-12.scope"),
            Some(("org.kde.foo-bar", true))
        );
        for unit in [
            "app-flatpak-dev.rift.TestApp.scope",
            "app-flatpak--12.scope",
            "app-flatpak-dev.rift.TestApp-12.service",
        ] {
            assert_eq!(app_of_scope(unit), None, "{unit}");
        }
    }
}
