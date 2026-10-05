//! The Flatpak side of the network switch: which app ids the system installation has, and the
//! override file that takes the network away from one of them.
//!
//! A Flatpak app never asks Airlock as it starts, the way a sandbox does, so the only way a new
//! instance can come up without the network is for flatpak itself to unshare it. flatpak reads the
//! overrides of the installation an app is in and nowhere else, so this is the system
//! installation's `overrides` folder, which is where the Store and Welcome put every app they
//! install. `shared=!network` in the `[Context]` section is the whole of it, and it is what
//! `flatpak override --unshare=network` writes itself, byte for byte.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::{fs, io};

use librift::airlock::name_problem;

/// The section of an override that holds what an app shares with the machine.
const CONTEXT: &str = "[Context]";

/// The key in it that holds the network.
const SHARED: &str = "shared";

/// What the network reads as in that key when it has been taken away.
const NO_NETWORK: &str = "!network";

/// What it reads as when the app has it.
const NETWORK: &str = "network";

/// The override that holds for every app of an installation rather than one. Airlock never writes
/// it: it is not an app.
const GLOBAL: &str = "global";

/// The app ids the installation at `root` has: the folders under its `app`. An installation that
/// is not there yet has none.
///
/// # Errors
///
/// When the folder is there and cannot be read.
pub fn installed(root: &Path) -> io::Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    let apps = match fs::read_dir(root.join("app")) {
        Ok(apps) => apps,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(found),
        Err(error) => return Err(error),
    };
    for app in apps.flatten() {
        if let Some(id) = app.file_name().to_str() {
            if id != GLOBAL && name_problem(id).is_none() {
                found.insert(id.to_string());
            }
        }
    }
    Ok(found)
}

/// Where the override of one app of the installation at `root` is.
#[must_use]
pub fn override_file(root: &Path, app: &str) -> PathBuf {
    root.join("overrides").join(app)
}

/// The text of an override with the network unshared, or given back, and everything else it says
/// left as it was. `None` when the override would be left saying nothing at all, and so should be
/// taken away, which is what `flatpak override --reset` leaves behind.
#[must_use]
pub fn edited(text: &str, off: bool) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut section = String::new();
    let mut said = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // the context ends here, so this is the last place a line of its own can go into it
            if off && !said && section == CONTEXT {
                lines.push(format!("{SHARED}={NO_NETWORK};"));
                said = true;
            }
            section = trimmed.to_string();
        } else if !said && section == CONTEXT {
            if let Some(values) = shared(trimmed) {
                said = true;
                let mut kept: Vec<&str> = values
                    .split(';')
                    .map(str::trim)
                    .filter(|value| !value.is_empty() && *value != NETWORK && *value != NO_NETWORK)
                    .collect();
                if off {
                    kept.push(NO_NETWORK);
                }
                if !kept.is_empty() {
                    lines.push(format!("{SHARED}={};", kept.join(";")));
                }
                continue;
            }
        }
        lines.push(line.to_string());
    }
    if off && !said {
        if section != CONTEXT {
            lines.push(CONTEXT.to_string());
        }
        lines.push(format!("{SHARED}={NO_NETWORK};"));
    }
    // sections with no keys under them are an override that asks for nothing
    lines.iter().any(|line| line.contains('=')).then(|| {
        let mut out = lines.join("\n");
        out.push('\n');
        out
    })
}

/// What a `shared=` line holds, or `None` when the line is not that key.
fn shared(line: &str) -> Option<&str> {
    let (key, values) = line.split_once('=')?;
    (key.trim() == SHARED).then_some(values)
}

/// Takes the network away from `app` in the installation at `root`, or gives it back, leaving
/// whatever else its override says alone. An override that is left saying nothing is removed.
///
/// An installation with no `overrides` folder is one the image did not make, which is an image with
/// no Flatpak at all, so there is nothing to write and nothing to take away. The folder is never
/// made here: the only thing outside its own state that Airlock may write is that one folder.
///
/// # Errors
///
/// A sentence when the file cannot be read, written or removed.
pub fn set_override(root: &Path, app: &str, off: bool) -> Result<(), String> {
    let file = override_file(root, app);
    if !file.parent().is_some_and(Path::is_dir) {
        return Ok(());
    }
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("Could not read {}: {error}.", file.display())),
    };
    match edited(&text, off) {
        Some(new) if new == text => Ok(()),
        Some(new) => fs::write(&file, new)
            .map_err(|error| format!("Could not write {}: {error}.", file.display())),
        None => match fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("Could not remove {}: {error}.", file.display())),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_override_is_what_flatpak_writes_itself() {
        // the 27 bytes flatpak override --unshare=network leaves behind
        assert_eq!(
            edited("", true).as_deref(),
            Some("[Context]\nshared=!network;\n")
        );
        assert_eq!(edited("[Context]\nshared=!network;\n", false), None);
        assert_eq!(
            edited("[Context]\nshared=!network;\n", true).as_deref(),
            Some("[Context]\nshared=!network;\n")
        );
    }

    #[test]
    fn the_rest_of_an_override_is_left_as_it_was() {
        let kept = "[Context]\nfilesystems=/data;\nshared=ipc;\n\n[Environment]\nLANG=C\n";
        let off = edited(kept, true).unwrap();
        assert_eq!(
            off,
            "[Context]\nfilesystems=/data;\nshared=ipc;!network;\n\n[Environment]\nLANG=C\n"
        );
        assert_eq!(edited(&off, false).as_deref(), Some(kept));
        // a context with nothing in it yet, and one that is not the first section
        assert_eq!(
            edited("[Session Bus Policy]\norg.gnome.Shell=talk\n", true).as_deref(),
            Some("[Session Bus Policy]\norg.gnome.Shell=talk\n[Context]\nshared=!network;\n")
        );
        assert_eq!(
            edited("[Context]\n[Environment]\nLANG=C\n", true).as_deref(),
            Some("[Context]\nshared=!network;\n[Environment]\nLANG=C\n")
        );
    }

    #[test]
    fn the_network_is_taken_out_of_a_shared_line_either_way_round() {
        // the app's own metadata says shared=network, so an override that says it as well has to go
        assert_eq!(
            edited("[Context]\nshared=network;ipc;\n", true).as_deref(),
            Some("[Context]\nshared=ipc;!network;\n")
        );
        assert_eq!(
            edited("[Context]\nshared=network;\n", true).as_deref(),
            Some("[Context]\nshared=!network;\n")
        );
        assert_eq!(edited("[Context]\nshared=network;\n", false), None);
        // only the context's own key, and only the first one
        assert_eq!(
            edited("[Other]\nshared=network;\n", true).as_deref(),
            Some("[Other]\nshared=network;\n[Context]\nshared=!network;\n")
        );
    }

    #[test]
    fn an_override_is_written_read_and_taken_away() {
        let root = std::env::temp_dir().join(format!("airlock-flatpak-{}", std::process::id()));
        let file = override_file(&root, "dev.rift.TestApp");
        // an installation with no overrides folder is an image with no flatpak: nothing happens
        assert_eq!(set_override(&root, "dev.rift.TestApp", true), Ok(()));
        assert!(!file.exists());
        fs::create_dir_all(root.join("overrides")).unwrap();
        assert_eq!(set_override(&root, "dev.rift.TestApp", false), Ok(()));
        assert!(!file.exists());
        assert_eq!(set_override(&root, "dev.rift.TestApp", true), Ok(()));
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "[Context]\nshared=!network;\n"
        );
        assert_eq!(installed(&root).unwrap(), BTreeSet::new());
        for folder in ["app/dev.rift.TestApp", "app/org.videolan.VLC", "runtime/x"] {
            fs::create_dir_all(root.join(folder)).unwrap();
        }
        assert_eq!(
            installed(&root).unwrap(),
            [
                "dev.rift.TestApp".to_string(),
                "org.videolan.VLC".to_string()
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(set_override(&root, "dev.rift.TestApp", false), Ok(()));
        assert!(!file.exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
