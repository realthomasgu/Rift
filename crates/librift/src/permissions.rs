//! What a Flatpak app asks for, read out of the metadata that comes with it. An app in a sandbox
//! has nothing but what its metadata asks for, so this list is the whole answer to what installing
//! it allows, and the Store shows it before anything is downloaded.
//!
//! The metadata is a key file: a `[Context]` section with lists separated by semicolons, and a
//! section for each bus the app may talk on. The words here are the plain reading of those keys.
//! The ones that reach the owner's own files, the whole machine or another app come first, because
//! those are the ones worth reading.

/// One thing an app asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permission {
    /// The line the page shows.
    pub said: String,
    /// Whether it reaches the owner's files, the machine itself or another app.
    pub wide: bool,
}

impl Permission {
    fn plain(said: &str) -> Self {
        Self {
            said: said.to_string(),
            wide: false,
        }
    }

    fn wide(said: &str) -> Self {
        Self {
            said: said.to_string(),
            wide: true,
        }
    }
}

/// The keys of the Context section this reads, in the order the list shows them: the owner's files
/// first, then the machine, then the desktop, then the network.
const KINDS: [&str; 6] = [
    "filesystems",
    "devices",
    "sockets",
    "shared",
    "features",
    "persistent",
];

/// What an app's metadata asks for, the wide ones first, each one once. An app that asks for
/// nothing gives an empty list, which the page says in its own words.
#[must_use]
pub fn of(metadata: &str) -> Vec<Permission> {
    let read = keys(metadata);
    let mut asked: Vec<Permission> = Vec::new();
    for kind in KINDS {
        for (_, _, value) in read
            .iter()
            .filter(|(section, key, _)| section == "Context" && key == kind)
        {
            if kind == "persistent" {
                if !list(value).is_empty() {
                    asked.push(Permission::plain(
                        "Keeps files of its own under .var/app in your home folder",
                    ));
                }
                continue;
            }
            for one in list(value) {
                asked.push(match kind {
                    "filesystems" => folder(one),
                    "devices" => device(one),
                    "sockets" => socket(one),
                    "features" => feature(one),
                    _ => shared(one),
                });
            }
        }
    }
    for (section, name, value) in &read {
        match section.as_str() {
            "Session Bus Policy" => asked.push(bus(name, value, "session")),
            "System Bus Policy" => asked.push(bus(name, value, "system")),
            _ => {}
        }
    }
    // the same ask written twice is one ask, and the wide ones are read first
    let mut once: Vec<Permission> = Vec::new();
    for permission in asked {
        if !once.iter().any(|already| already.said == permission.said) {
            once.push(permission);
        }
    }
    once.sort_by_key(|permission| !permission.wide);
    once
}

/// Something of the machine the app shares rather than having its own.
fn shared(asked: &str) -> Permission {
    match asked {
        "network" => Permission::plain("Reaches the network"),
        "ipc" => Permission::plain("Talks to the programs of the desktop directly"),
        other => Permission::plain(&format!("Shares {other} with the machine")),
    }
}

/// A folder the app asks for. A name with `:ro` after it is a read, anything else is a read and a
/// write. A folder of the owner's own stays wide either way: reading all of it is reading all of
/// it.
fn folder(asked: &str) -> Permission {
    let (name, how) = asked.split_once(':').unwrap_or((asked, "rw"));
    let read_only = how == "ro";
    let (what, wide) = place(name);
    let said = if read_only {
        format!("Reads {what}")
    } else {
        format!("Reads and writes {what}")
    };
    Permission { said, wide }
}

/// What a name in the filesystems list stands for, and whether it is a wide one. A path is itself;
/// a name with a slash in it is one thing inside a folder that has a name of its own.
fn place(name: &str) -> (String, bool) {
    if name.starts_with('/') || name.starts_with('~') {
        return (name.to_string(), true);
    }
    if let Some((head, tail)) = name.split_once('/')
        && !tail.is_empty()
    {
        let (folder, _) = named(head);
        return (format!("{tail} in {folder}"), false);
    }
    named(name)
}

/// The plain words for one of the folders flatpak has a name for.
fn named(name: &str) -> (String, bool) {
    let (what, wide) = match name {
        "host" => ("every file on the machine", true),
        "host-os" => ("the files the system is made of", true),
        "host-etc" => ("the settings of the system", true),
        "home" => ("your home folder", true),
        "xdg-desktop" => ("your desktop folder", false),
        "xdg-documents" => ("your Documents folder", false),
        "xdg-download" => ("your Downloads folder", false),
        "xdg-music" => ("your Music folder", false),
        "xdg-pictures" => ("your Pictures folder", false),
        "xdg-videos" => ("your Videos folder", false),
        "xdg-public-share" => ("your public folder", false),
        "xdg-templates" => ("your templates folder", false),
        "xdg-config" => ("the folder every app keeps its settings in", true),
        "xdg-data" => ("the folder every app keeps its files in", true),
        "xdg-cache" => ("the folder every app keeps its cache in", true),
        "xdg-run" => ("the folder of this login", false),
        _ => (name, false),
    };
    (what.to_string(), wide)
}

/// A device the app asks for.
fn device(asked: &str) -> Permission {
    match asked {
        "all" => Permission::wide("Reaches every device, the camera and the drives among them"),
        "dri" => Permission::plain("Uses the graphics card"),
        "input" => Permission::wide("Reads the keyboard and the mouse from the devices themselves"),
        "kvm" => Permission::plain("Runs virtual machines"),
        "shm" => Permission::plain("Shares memory with the programs of the desktop"),
        other => Permission::plain(&format!("Uses the {other} devices")),
    }
}

/// A socket the app asks for. These are the ways out of the sandbox that are a hole in it.
fn socket(asked: &str) -> Permission {
    match asked {
        "wayland" | "inherit-wayland-socket" => Permission::plain("Opens a window of its own"),
        "x11" => Permission::wide("Draws through X11, so it can read every other window"),
        "fallback-x11" => Permission::plain("Draws through X11 where there is no Wayland"),
        "pulseaudio" => Permission::plain("Plays sound, and can listen to the microphone"),
        "session-bus" => {
            Permission::wide("Reaches your whole session bus, so it can drive your other apps")
        }
        "system-bus" => {
            Permission::wide("Reaches the whole system bus, so it can drive the system")
        }
        "ssh-auth" => Permission::wide("Uses your ssh keys through the agent that holds them"),
        "gpg-agent" => Permission::wide("Uses your gpg keys through the agent that holds them"),
        "cups" => Permission::plain("Prints"),
        "pcsc" => Permission::plain("Uses smart cards"),
        other => Permission::plain(&format!("Uses the {other} socket of the desktop")),
    }
}

/// Something flatpak keeps off unless an app asks for it.
fn feature(asked: &str) -> Permission {
    match asked {
        "devel" => Permission::wide("Uses the calls one program needs to look inside another"),
        "multiarch" => Permission::plain("Runs programs built for other kinds of processor"),
        "bluetooth" => Permission::plain("Uses Bluetooth"),
        "canbus" => Permission::plain("Uses the bus of a vehicle"),
        "per-app-dev-shm" => Permission::plain("Keeps shared memory of its own"),
        other => Permission::plain(&format!("Uses {other}")),
    }
}

/// A name on a bus the app may talk to. A few names are worth saying in words, and the one that
/// allows an app to start programs outside its own sandbox is the one to say first.
fn bus(name: &str, how: &str, which: &str) -> Permission {
    let how = how.trim();
    if how == "none" {
        return Permission::plain(&format!("Does not talk to {name}"));
    }
    match name {
        "org.freedesktop.Flatpak" | "org.freedesktop.Flatpak.*" => {
            return Permission::wide(
                "Runs programs outside its own sandbox, which is the sandbox undone",
            );
        }
        "org.freedesktop.secrets" => {
            return Permission::wide("Reaches the passwords the desktop keeps");
        }
        "org.freedesktop.Notifications" => return Permission::plain("Sends notifications"),
        "ca.desrt.dconf" => {
            return Permission::wide("Reads and writes the settings of the desktop");
        }
        "org.freedesktop.ScreenSaver" => {
            return Permission::plain("Keeps the screen awake while it is playing");
        }
        _ => {}
    }
    let said = match how {
        "own" => format!("Takes the name {name} on the {which} bus"),
        "see" => format!("Sees whether {name} is on the {which} bus"),
        _ => format!("Talks to {name} on the {which} bus"),
    };
    Permission {
        said,
        wide: which == "system",
    }
}

/// The section, the key and the value of every line of a key file, in the order they are written.
/// A line that is neither a section nor a key is skipped, the way a key file reader skips it.
fn keys(metadata: &str) -> Vec<(String, String, String)> {
    let mut section = String::new();
    let mut found = Vec::new();
    for line in metadata.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = name.trim().to_string();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            found.push((
                section.clone(),
                key.trim().to_string(),
                value.trim().to_string(),
            ));
        }
    }
    found
}

/// The entries of one of the lists in the Context section. An entry with an exclamation mark in
/// front of it takes a permission away rather than asking for one, so it is not listed.
fn list(value: &str) -> Vec<&str> {
    value
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty() && !entry.starts_with('!'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What flatpak prints for an app that asks for a good deal: the metadata of the boot test's
    /// own app, with the wide asks among the ordinary ones.
    const EDITOR: &str = "[Application]\n\
         name=dev.rift.TestEditor\n\
         runtime=dev.rift.TestPlatform/x86_64/test\n\
         command=probe\n\
         \n\
         [Context]\n\
         shared=network;ipc;\n\
         sockets=wayland;pulseaudio;\n\
         devices=dri;\n\
         filesystems=home;xdg-download:ro;\n\
         \n\
         [Session Bus Policy]\n\
         org.freedesktop.Notifications=talk\n";

    fn lines(metadata: &str) -> Vec<String> {
        of(metadata)
            .into_iter()
            .map(|permission| permission.said)
            .collect()
    }

    #[test]
    fn the_wide_asks_come_first_and_the_ordinary_ones_after() {
        assert_eq!(
            lines(EDITOR),
            [
                "Reads and writes your home folder",
                "Reads your Downloads folder",
                "Uses the graphics card",
                "Opens a window of its own",
                "Plays sound, and can listen to the microphone",
                "Reaches the network",
                "Talks to the programs of the desktop directly",
                "Sends notifications",
            ]
        );
        let asked = of(EDITOR);
        assert_eq!(asked.iter().filter(|one| one.wide).count(), 1);
        assert!(asked[0].wide);
    }

    #[test]
    fn an_app_that_asks_for_nothing_asks_for_nothing() {
        assert!(of("").is_empty());
        assert!(of("[Application]\nname=dev.rift.TestApp\n").is_empty());
        // and one that only gives a permission back asks for nothing either
        assert!(of("[Context]\nfilesystems=!home;\nshared=\n").is_empty());
    }

    #[test]
    fn the_whole_machine_and_the_sandbox_undone_are_the_wide_ones() {
        let wide = |metadata: &str| -> Vec<String> {
            of(metadata)
                .into_iter()
                .filter(|one| one.wide)
                .map(|one| one.said)
                .collect()
        };
        assert_eq!(
            wide("[Context]\nfilesystems=host;\n"),
            ["Reads and writes every file on the machine"]
        );
        assert_eq!(
            wide("[Context]\nfilesystems=host:ro;\n"),
            ["Reads every file on the machine"]
        );
        assert_eq!(
            wide("[Context]\nsockets=x11;session-bus;\ndevices=all;\n"),
            [
                "Reaches every device, the camera and the drives among them",
                "Draws through X11, so it can read every other window",
                "Reaches your whole session bus, so it can drive your other apps",
            ]
        );
        assert_eq!(
            wide("[Session Bus Policy]\norg.freedesktop.Flatpak=talk\n"),
            ["Runs programs outside its own sandbox, which is the sandbox undone"]
        );
        assert_eq!(
            wide("[System Bus Policy]\norg.freedesktop.UDisks2=talk\n"),
            ["Talks to org.freedesktop.UDisks2 on the system bus"]
        );
        // a folder of its own is read and written, and a name only seen is not wide
        assert_eq!(
            wide("[Context]\nfilesystems=/srv/work;~/Notes;xdg-music;\n"),
            [
                "Reads and writes /srv/work".to_string(),
                "Reads and writes ~/Notes".to_string()
            ]
        );
        assert!(wide("[Session Bus Policy]\norg.gnome.Shell=see\n").is_empty());
        // and one thing inside a folder is said as one thing inside a folder
        assert_eq!(
            lines("[Context]\nfilesystems=xdg-config/kdeglobals:ro;xdg-run/gvfs;\n"),
            [
                "Reads kdeglobals in the folder every app keeps its settings in",
                "Reads and writes gvfs in the folder of this login",
            ]
        );
    }

    #[test]
    fn a_key_file_is_read_the_way_a_key_file_reader_reads_it() {
        let read = keys("# a comment\n[One]\n a = b \n\nnonsense\n[Two]\nc=d=e\n");
        assert_eq!(
            read,
            vec![
                ("One".to_string(), "a".to_string(), "b".to_string()),
                ("Two".to_string(), "c".to_string(), "d=e".to_string()),
            ]
        );
        assert!(keys("").is_empty());
        assert_eq!(list("a;;b ; !c;"), ["a", "b"]);
        // a word nobody here knows is still said, and said plainly
        assert_eq!(
            lines("[Context]\nsockets=something;\ndevices=odd;\nfeatures=new;\nshared=other;\n"),
            [
                "Uses the odd devices",
                "Uses the something socket of the desktop",
                "Shares other with the machine",
                "Uses new",
            ]
        );
    }

    #[test]
    fn the_names_worth_saying_in_words_are_said_in_words() {
        assert_eq!(
            lines("[Session Bus Policy]\norg.freedesktop.secrets=talk\nca.desrt.dconf=own\n"),
            [
                "Reaches the passwords the desktop keeps",
                "Reads and writes the settings of the desktop",
            ]
        );
        assert_eq!(
            lines(
                "[Session Bus Policy]\norg.freedesktop.ScreenSaver=talk\norg.example.Thing=own\n"
            ),
            [
                "Keeps the screen awake while it is playing",
                "Takes the name org.example.Thing on the session bus",
            ]
        );
        assert_eq!(
            lines("[Context]\npersistent=.config/thing;\nfeatures=devel;bluetooth;\n"),
            [
                "Uses the calls one program needs to look inside another",
                "Uses Bluetooth",
                "Keeps files of its own under .var/app in your home folder",
            ]
        );
    }
}
