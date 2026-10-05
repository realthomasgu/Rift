//! What flatpak says about the apps: the remotes the system installation has, how much room each
//! app on the suggested list takes, which needs the network, and what is installed already. It is
//! asked on a thread of its own as the window opens, because reading a remote reads its summary
//! over the network.

use std::thread;

use iced::Task;
use iced::futures::channel::oneshot;
use librift::flatpak::{self, Listed};
use librift::suggested::{self, App};

use crate::ui::{Message, Store};

/// What one remote said about the size of every app on it, or why it could not say.
pub type Sizes = Result<Vec<(String, String)>, String>;

/// What flatpak said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    /// The system installation's remotes, or why flatpak could not say.
    pub remotes: Result<Vec<String>, String>,
    /// The size of every app on each remote the suggested list names, or why the remote did not
    /// say.
    pub sizes: Vec<(String, Sizes)>,
    /// The apps installed, in either installation.
    pub installed: Vec<Listed>,
}

impl Catalog {
    /// A question to flatpak that got no answer at all.
    fn failed(why: &str) -> Self {
        Self {
            remotes: Err(why.to_string()),
            sizes: Vec::new(),
            installed: Vec::new(),
        }
    }

    /// The remotes the system installation has, or none when flatpak could not say.
    #[must_use]
    pub fn remotes(&self) -> &[String] {
        self.remotes.as_deref().unwrap_or_default()
    }

    /// How much room a suggested app takes, when its remote said.
    #[must_use]
    pub fn size(&self, app: &App) -> Option<&str> {
        self.on(&app.remote, &app.id)
    }

    /// How much room an app on a remote takes, when that remote said.
    #[must_use]
    pub fn on(&self, remote: &str, id: &str) -> Option<&str> {
        self.sizes
            .iter()
            .find(|(named, _)| named == remote)
            .and_then(|(_, answer)| answer.as_ref().ok())
            .and_then(|sizes| sizes.iter().find(|(named, _)| named == id))
            .map(|(_, size)| size.as_str())
    }

    /// Whether an app is installed.
    #[must_use]
    pub fn has(&self, id: &str) -> bool {
        self.installed.iter().any(|one| one.id == id)
    }

    /// Why a remote did not say how big its apps are, when it did not, or why flatpak could not
    /// list the remotes at all.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        if let Err(why) = &self.remotes {
            return Some(why);
        }
        self.sizes
            .iter()
            .find_map(|(_, answer)| answer.as_ref().err())
            .map(String::as_str)
    }
}

/// Ask flatpak on a thread of its own.
pub fn ask(state: &mut Store) -> Task<Message> {
    if state.asking {
        return Task::none();
    }
    state.asking = true;
    let apps = state.apps.clone().unwrap_or_default();
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(read(&apps));
    });
    Task::perform(receiver, |answered| {
        Message::Catalog(Box::new(answered.unwrap_or_else(|_| {
            Catalog::failed("The question to flatpak stopped before it was answered.")
        })))
    })
}

/// Ask again unless flatpak has answered it all already.
pub fn ask_once(state: &mut Store) -> Task<Message> {
    let answered = state
        .catalog
        .as_ref()
        .is_some_and(|catalog| catalog.problem().is_none() && !catalog.sizes.is_empty());
    if answered { Task::none() } else { ask(state) }
}

/// What flatpak says: the remotes, the sizes on each remote a suggested app comes from, and what is
/// installed.
fn read(apps: &[App]) -> Catalog {
    let remotes = flatpak::remotes();
    let mut wanted: Vec<String> = Vec::new();
    for app in suggested::offered(apps, remotes.as_deref().unwrap_or_default()) {
        if !wanted.contains(&app.remote) {
            wanted.push(app.remote.clone());
        }
    }
    // each remote on a thread of its own, so a slow one does not hold back the others
    let sizes = thread::scope(|scope| {
        let asked: Vec<_> = wanted
            .into_iter()
            .map(|remote| {
                scope.spawn(move || {
                    let answer = flatpak::sizes(&remote);
                    (remote, answer)
                })
            })
            .collect();
        asked
            .into_iter()
            .filter_map(|one| one.join().ok())
            .collect()
    });
    Catalog {
        remotes,
        sizes,
        installed: flatpak::apps().unwrap_or_default(),
    }
}

/// The suggested apps to list: every app from Flathub, and one from another remote where the system
/// installation has that remote.
#[must_use]
pub fn offered(state: &Store) -> Vec<&App> {
    let remotes = state
        .catalog
        .as_ref()
        .map(|catalog| catalog.remotes())
        .unwrap_or_default();
    state
        .apps
        .as_deref()
        .map(|apps| suggested::offered(apps, remotes))
        .unwrap_or_default()
}

/// Whether an app is installed, as far as the Store knows.
#[must_use]
pub fn is_installed(state: &Store, id: &str) -> bool {
    state
        .catalog
        .as_ref()
        .is_some_and(|catalog| catalog.has(id))
}

/// Whether an install or a remove of an app is in the queue or running now.
#[must_use]
pub fn is_busy(state: &Store, id: &str) -> bool {
    state
        .work
        .iter()
        .any(|work| work.id == id && work.doing.pending())
}

/// An app has finished installing, or been taken off: what the Store knows about it changes without
/// asking flatpak everything again.
pub fn settled(state: &mut Store, id: &str, removed: bool) {
    let Some(catalog) = state.catalog.as_mut() else {
        return;
    };
    if removed {
        catalog.installed.retain(|one| one.id != id);
    } else if !catalog.has(id) {
        catalog.installed.push(Listed {
            name: state
                .shown
                .as_ref()
                .filter(|shown| shown.id == id)
                .map_or_else(|| flatpak::tail(id), |shown| shown.name.clone()),
            id: id.to_string(),
            size: String::new(),
            remote: String::new(),
        });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use librift::flatpak::FLATHUB;

    pub(crate) fn listed() -> Vec<App> {
        suggested::parse(
            "[[app]]\nid = \"org.videolan.VLC\"\nname = \"VLC\"\nabout = \"Video\"\ngroup = \"Media\"\n\n\
             [[app]]\nid = \"org.gimp.GIMP\"\nname = \"GIMP\"\nabout = \"Pictures\"\ngroup = \"Creative\"\n\n\
             [[app]]\nid = \"dev.rift.TestEditor\"\nname = \"Rift test editor\"\nabout = \"The test's\"\n\
             group = \"Testing\"\nremote = \"rift-test\"\n",
        )
        .unwrap()
    }

    pub(crate) fn answered(remotes: &[&str], installed: &[&str]) -> Catalog {
        Catalog {
            remotes: Ok(remotes.iter().map(|name| (*name).to_string()).collect()),
            sizes: vec![(
                FLATHUB.to_string(),
                Ok(vec![(
                    "org.videolan.VLC".to_string(),
                    "139.4 MB".to_string(),
                )]),
            )],
            installed: installed
                .iter()
                .map(|id| Listed {
                    id: (*id).to_string(),
                    name: flatpak::tail(id),
                    size: "1.0 MB".to_string(),
                    remote: FLATHUB.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn an_app_from_another_remote_is_listed_once_that_remote_is_there() {
        let mut state = Store::bare();
        state.apps = Ok(listed());
        state.catalog = Some(Box::new(answered(&[FLATHUB], &[])));
        let ids = |state: &Store| -> Vec<String> {
            offered(state).iter().map(|app| app.id.clone()).collect()
        };
        assert_eq!(ids(&state), ["org.videolan.VLC", "org.gimp.GIMP"]);
        state.catalog = Some(Box::new(answered(&[FLATHUB, "rift-test"], &[])));
        assert_eq!(
            ids(&state),
            ["org.videolan.VLC", "org.gimp.GIMP", "dev.rift.TestEditor"]
        );
    }

    #[test]
    fn what_flatpak_said_is_what_the_page_shows() {
        let catalog = answered(&[FLATHUB], &["org.gimp.GIMP"]);
        assert_eq!(catalog.on(FLATHUB, "org.videolan.VLC"), Some("139.4 MB"));
        assert_eq!(catalog.on("rift-test", "org.videolan.VLC"), None);
        assert!(catalog.has("org.gimp.GIMP") && !catalog.has("org.videolan.VLC"));
        assert_eq!(catalog.problem(), None);
        assert_eq!(
            Catalog::failed("No flatpak.").problem(),
            Some("No flatpak.")
        );
        let mut asked = catalog.clone();
        asked.sizes = vec![(FLATHUB.to_string(), Err("No network.".to_string()))];
        assert_eq!(asked.problem(), Some("No network."));
    }

    #[test]
    fn an_app_that_is_installed_or_taken_off_changes_what_is_known() {
        let mut state = Store::bare();
        state.apps = Ok(listed());
        state.catalog = Some(Box::new(answered(&[FLATHUB], &[])));
        settled(&mut state, "org.videolan.VLC", false);
        assert!(is_installed(&state, "org.videolan.VLC"));
        // twice over changes nothing
        settled(&mut state, "org.videolan.VLC", false);
        assert_eq!(state.catalog.as_ref().unwrap().installed.len(), 1);
        settled(&mut state, "org.videolan.VLC", true);
        assert!(!is_installed(&state, "org.videolan.VLC"));
    }
}
