//! Debug builds only: what the pages draw when there is no flatpak to ask, so that every page of
//! the window can be drawn and looked at on a machine that is not Rift. `rift-store --pretend`
//! turns it on and nothing else does. The image is built in release, where this module and the flag
//! do not exist, so no page of the Store it ships can show anything but what flatpak said.

use librift::flatpak::{About, Found, Listed};
use librift::permissions::{self, Permission};
use librift::suggested;

use crate::catalog::Catalog;
use crate::ui::Store;

/// The suggested list in the source tree, which is the one the image installs.
const APPS: &str = include_str!("../../../nix/welcome/apps.toml");

/// What one app on Flathub really says about itself, taken from what the remote answered on
/// 2026-10-05.
const VLC: &str = "[Application]\n\
     name=org.videolan.VLC\n\
     runtime=org.kde.Platform/x86_64/5.15-25.08\n\
     command=vlc\n\
     \n\
     [Context]\n\
     shared=ipc;network;\n\
     sockets=pulseaudio;x11;\n\
     devices=all;\n\
     filesystems=xdg-config/kdeglobals:ro;host;xdg-run/gvfs;\n\
     \n\
     [Session Bus Policy]\n\
     org.mpris.MediaPlayer2.vlc=own\n\
     org.freedesktop.secrets=talk\n\
     org.freedesktop.ScreenSaver=talk\n";

/// The sizes Flathub gave for the suggested apps on 2026-10-05, so the rows are the right width.
const SIZES: [(&str, &str); 8] = [
    ("net.mullvad.MullvadBrowser", "284.6 MB"),
    ("org.videolan.VLC", "139.4 MB"),
    ("org.signal.Signal", "291.0 MB"),
    ("org.libreoffice.LibreOffice", "1.1 GB"),
    ("md.obsidian.Obsidian", "402.3 MB"),
    ("org.blender.Blender", "1.3 GB"),
    ("org.gimp.GIMP", "663.3 MB"),
    ("org.wireshark.Wireshark", "253.8 MB"),
];

/// Fill the window with it.
pub fn fill(state: &mut Store) {
    state.pretend = true;
    state.apps = suggested::parse(APPS);
    state.catalog = Some(Box::new(Catalog {
        remotes: Ok(vec![librift::flatpak::FLATHUB.to_string()]),
        sizes: vec![(
            librift::flatpak::FLATHUB.to_string(),
            Ok(SIZES
                .iter()
                .map(|(id, size)| ((*id).to_string(), (*size).to_string()))
                .collect()),
        )],
        installed: vec![Listed {
            id: "org.keepassxc.KeePassXC".to_string(),
            name: "KeePassXC".to_string(),
            size: "122.4 MB".to_string(),
            remote: librift::flatpak::FLATHUB.to_string(),
        }],
    }));
}

/// What a search finds.
#[must_use]
pub fn found(words: &str) -> Vec<Found> {
    let listed = suggested::parse(APPS).unwrap_or_default();
    let words = words.to_lowercase();
    listed
        .iter()
        .filter(|app| {
            app.name.to_lowercase().contains(&words) || app.about.to_lowercase().contains(&words)
        })
        .map(|app| Found {
            id: app.id.clone(),
            name: app.name.clone(),
            summary: app.about.clone(),
            version: String::new(),
            remote: app.remote.clone(),
        })
        .collect()
}

/// What a remote says about one app, and what that app asks for.
#[must_use]
pub fn about(id: &str, name: &str) -> (About, Vec<Permission>) {
    let size = SIZES
        .iter()
        .find(|(listed, _)| *listed == id)
        .map_or("139.4 MB", |(_, size)| *size);
    (
        About {
            id: id.to_string(),
            name: name.to_string(),
            summary: String::new(),
            version: "3.0.23".to_string(),
            licence: "GPL-2.0+".to_string(),
            download: "52.7 MB".to_string(),
            installed: size.to_string(),
            runtime: "org.kde.Platform/x86_64/5.15-25.08".to_string(),
        },
        permissions::of(VLC),
    )
}
