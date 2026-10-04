//! The AI page: which model Quasar runs, how big a model this machine is set to run, and which of
//! the models Rift knows about are on the drive.
//!
//! Quasar answers on the system bus, and the page follows it while it is open, so a model that
//! finishes loading turns up without anyone opening the page again. The size lives in the host
//! profile, which is root's, so Orbit writes it, the way the Displays page writes a screen's size.
//! Quasar reads it when it starts and picks a model from it once, so a size chosen here is the one
//! it picks the next time it starts, and the page says that rather than pretending the model
//! changes under it.

use std::path::Path;
use std::thread;

use iced::futures::channel::oneshot;
use iced::widget::{column, text};
use iced::{Element, Fill, Task};
use librift::models::{self, Manifest, Tier};
use librift::paths;
use librift::quasar::{self, Status};

use crate::ghost;
use crate::theme::Colors;
use crate::ui::{Message, Settings};
use crate::widgets::{GAP, TEXT_SIZE, choice, fact, group, heading, note, setting};

/// What the two pages over Quasar draw: what it answered on the bus, the chat models the manifest
/// declares, and what each size asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    /// What Quasar says about itself, or the sentence saying why it did not say.
    pub status: Result<Status, String>,
    /// The chat models worth showing, in the manifest's order.
    pub models: Vec<Model>,
    /// The three sizes, each with the sentence that says what a machine of that size is and which
    /// model it asks for.
    pub sizes: Vec<(Tier, String)>,
}

/// One chat model, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// Manifest id, which is the name Quasar gives the model it runs.
    pub id: String,
    /// What it is: how many parameters, how they are packed, how much room the file takes.
    pub about: String,
    /// Whether its weights are in the models directory.
    pub here: bool,
}

impl Picture {
    /// Ask Quasar, read the manifest and look in the models directory.
    fn read() -> Self {
        let manifest = Manifest::load(Path::new(paths::MODEL_MANIFEST)).unwrap_or_default();
        Self::of(&manifest, Path::new(paths::MODELS), quasar::status())
    }

    /// The same from a manifest and a models directory that are named, which the tests use.
    fn of(manifest: &Manifest, models_dir: &Path, status: Result<Status, String>) -> Self {
        let models = manifest
            .chat
            .iter()
            .filter_map(|chat| {
                let here = models::on_drive(models_dir, &chat.file);
                // the model that is only there for the boot test is not one to offer a person, and
                // it is on the drive of the machine that runs the test, where it is what answers
                (!chat.test || here).then(|| Model {
                    id: chat.id.clone(),
                    about: chat.about(),
                    here,
                })
            })
            .collect();
        let sizes = Tier::ALL
            .into_iter()
            .map(|tier| (tier, about_a_size(manifest, tier)))
            .collect();
        Self {
            status,
            models,
            sizes,
        }
    }

    /// What Quasar answered, when it answered.
    fn status(&self) -> Option<&Status> {
        self.status.as_ref().ok()
    }
}

/// What a machine of this size is, and the model the size asks for.
fn about_a_size(manifest: &Manifest, tier: Tier) -> String {
    let machine = format!("For a machine with {} GB of memory.", tier.ram_gb());
    match manifest.wanted(Some(tier)) {
        Some(chat) => format!("{machine} It runs {}.", chat.id),
        None => machine,
    }
}

/// Ask Quasar and read the manifest on a thread of its own, the way the window asks Orbit when it
/// opens. The watch reads it again after every change Quasar announces.
pub fn ask() -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(Picture::read());
    });
    Task::perform(receiver, |answered| match answered {
        Ok(picture) => Message::Quasar(Box::new(picture)),
        Err(_) => nothing("Quasar did not answer."),
    })
}

/// What Quasar says now, for the watch that follows it.
#[must_use]
pub fn reading() -> Message {
    Message::Quasar(Box::new(Picture::read()))
}

/// A picture of a machine whose Quasar said nothing at all.
fn nothing(why: &str) -> Message {
    Message::Quasar(Box::new(Picture::of(
        &Manifest::default(),
        Path::new(paths::MODELS),
        Err(why.to_string()),
    )))
}

/// Write the size this machine runs: Orbit puts it in the host profile, and the page asks Orbit
/// again afterwards, so the row with the mark on it is the one in the file.
pub fn set_size(tier: Tier) -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let said = librift::orbit::set("tier", tier.name()).and_then(|()| librift::orbit::host());
        let _ = sender.send(said);
    });
    Task::perform(receiver, |said| {
        Message::Orbit(said.unwrap_or_else(|_| Err("Orbit did not answer.".to_string())))
    })
}

/// The size this machine is set to, which Orbit answers with once it has.
fn size_now(state: &Settings) -> Option<Tier> {
    state
        .host
        .as_ref()
        .and_then(|answered| answered.as_ref().ok())
        .and_then(|host| Tier::parse(&host.ai_tier))
}

/// The size a word names, for `--set tier`.
#[must_use]
pub fn named(value: &str) -> Option<Tier> {
    Tier::parse(value.trim().to_ascii_lowercase().as_str())
}

/// The lines `rift-settings --state` prints about the local AI: what Quasar says, the model it
/// runs, how many models are on the drive and the size this machine is set to.
#[must_use]
pub fn state(state: &Settings) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(picture) = state.quasar.as_deref() {
        if let Some(status) = picture.status() {
            lines.push(format!("ai {}", status.state));
            lines.push(format!("model {}", or_none(&status.model)));
        }
        lines.push(format!(
            "models {}",
            picture.models.iter().filter(|model| model.here).count()
        ));
    }
    if let Some(tier) = size_now(state) {
        lines.push(format!("tier {}", tier.name()));
    }
    lines
}

/// A value that is there, or the word for one that is not.
fn or_none(value: &str) -> String {
    if value.trim().is_empty() {
        "none".to_string()
    } else {
        value.trim().to_string()
    }
}

/// What a state word means, in the words a page says it in. The Search page says the same about the
/// model it reads files with.
#[must_use]
pub fn word(state: &str) -> &'static str {
    match state {
        "ready" => "Ready",
        "loading" => "Loading",
        "failed" => "Failed",
        _ => "Not running",
    }
}

/// What a row is set to, at the right of it.
pub fn said<'a, M: 'a>(look: Colors, value: &str) -> Element<'a, M> {
    text(value.to_string())
        .size(TEXT_SIZE)
        .color(look.dim)
        .into()
}

/// The page.
pub fn view(state: &Settings, look: Colors) -> Element<'_, Message> {
    let Some(picture) = state.quasar.as_deref() else {
        return note(look, "Asking Quasar which model it runs.");
    };
    let mut page = column![].spacing(GAP).width(Fill);
    match &picture.status {
        Err(why) => {
            page = page.push(note(
                look,
                "Quasar is not answering, so the model it runs is not here.",
            ));
            page = page.push(note(look, why));
        }
        Ok(status) => page = page.push(answers(look, status)),
    }
    if let Some(why) = &state.problem {
        page = page.push(text(why).size(TEXT_SIZE).color(look.error));
    }
    if !picture.sizes.is_empty() {
        page = page.push(sizes(state, look, picture));
    }
    if !picture.models.is_empty() {
        page = page.push(models(look, picture));
    }
    page.push(note(
        look,
        "rift ai asks the model a question from a terminal, and so does the field in the shell. \
         The model runs on this machine, and the question goes nowhere else.",
    ))
    .into()
}

/// The model that answers: what Quasar is doing, and which model it is doing it with.
fn answers(look: Colors, status: &Status) -> Element<'_, Message> {
    let mut rows = vec![
        setting(look, "State", None, said(look, word(&status.state))),
        setting(look, "Model", None, said(look, &or_none(&status.model))),
    ];
    if !status.error.is_empty() {
        rows.push(fact(look, "Why", status.error.trim().to_string()));
    }
    column![heading(look, "The model that answers"), group(look, rows)]
        .spacing(8)
        .into()
}

/// How big a model this machine runs: the three sizes, the one in the host profile marked.
fn sizes<'a>(state: &'a Settings, look: Colors, picture: &'a Picture) -> Element<'a, Message> {
    let set = size_now(state);
    let mut rows = Vec::new();
    for (tier, about) in &picture.sizes {
        rows.push(choice(
            look,
            label(*tier),
            Some(about.as_str()),
            None,
            set == Some(*tier),
            Message::Tier(*tier),
        ));
    }
    let mut section = column![heading(look, "How big a model this machine runs")].spacing(8);
    if set.is_none() {
        section = section.push(note(
            look,
            "Orbit has not said how big a model this machine can carry, so no size is marked.",
        ));
    }
    section = section.push(group(look, rows));
    if ghost::on() {
        // the size is written into the host profile under /var/lib, which is memory here, and
        // Quasar picks a model from it the next time it starts, which this session never has
        // (ADR-0084)
        return section.push(ghost::only_now(look, FOR_THIS_SESSION)).into();
    }
    for line in told(picture, set) {
        section = section.push(note(look, line));
    }
    section.into()
}

/// The name of a size.
fn label(tier: Tier) -> &'static str {
    match tier {
        Tier::Small => "Small",
        Tier::Medium => "Medium",
        Tier::Large => "Large",
    }
}

/// The sentences under the sizes: when a change takes hold, and whether Quasar is still running the
/// model for another size.
fn told(picture: &Picture, set: Option<Tier>) -> Vec<&'static str> {
    let mut lines = vec![WHEN];
    let started_with = picture.status().map(|status| status.tier.as_str());
    if let (Some(set), Some(started_with)) = (set, started_with)
        && !started_with.is_empty()
        && started_with != set.name()
    {
        lines.push(UNTIL);
    }
    lines
}

/// What a size chosen in a Ghost boot is, and what the models there are read as.
const FOR_THIS_SESSION: &str = "A size can be set for this session";
const NOT_MOUNTED: &str = "The models on the drive cannot be read";

/// What happens when the size changes.
const WHEN: &str = "Quasar picks a model when it starts, so a size chosen here is the one it picks \
                    the next time it starts. Orbit works a size out from how much memory this \
                    machine has, and what is chosen here takes its place on this machine.";
/// What is true while Quasar is still running the model for the size it started with.
const UNTIL: &str = "Quasar is still running the model for the size it started with. It picks one \
                     for this size when it next starts, which is when this drive next boots.";

/// Every model worth showing, with what it is and whether it is on the drive.
fn models(look: Colors, picture: &Picture) -> Element<'_, Message> {
    if ghost::on() {
        // the models are a mount off persist, so every row would say the model is not on the drive
        // when it may well be: the drive is locked and the weights are out of reach
        return column![
            heading(look, "Models"),
            group(look, vec![ghost::row(look, "Models", NOT_MOUNTED)]),
        ]
        .spacing(8)
        .into();
    }
    let running = picture
        .status()
        .map(|status| status.model.trim())
        .unwrap_or_default();
    let mut rows = Vec::new();
    for model in &picture.models {
        let about = Some(model.about.as_str()).filter(|about| !about.is_empty());
        let here = if !running.is_empty() && model.id == running {
            "Running"
        } else if model.here {
            "On the drive"
        } else {
            "Not on the drive"
        };
        rows.push(setting(look, &model.id, about, said(look, here)));
    }
    column![
        heading(look, "Models"),
        group(look, rows),
        note(
            look,
            "A model that is not on the drive is not downloaded from Settings yet. Quasar runs the \
             largest model on the drive that fits the size above.",
        ),
    ]
    .spacing(8)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[[chat]]
id = "tiny"
file = "tiny.gguf"
parameters = "0.6B"
quant = "Q8_0"
size_gb = 0.64
test = true
tier = { min_ram_gb = 2 }

[[chat]]
id = "small"
file = "small.gguf"
parameters = "1.7B"
quant = "Q4_K_M"
size_gb = 1.1
tier = { min_ram_gb = 4 }

[[chat]]
id = "medium"
file = "medium.gguf"
parameters = "4B"
quant = "Q4_K_M"
size_gb = 2.5
default = true
tier = { min_ram_gb = 8 }

[[embedding]]
id = "embed"
file = "embed.gguf"
"#;

    fn ready() -> Status {
        Status {
            state: "ready".into(),
            model: "small".into(),
            tier: "small".into(),
            embedding_state: "ready".into(),
            embedding_model: "embed".into(),
            ..Status::default()
        }
    }

    /// A models directory with the files that are named in it, under a name of this test's own.
    fn picture(at: &str, files: &[&str], status: Result<Status, String>) -> Picture {
        let dir = std::env::temp_dir().join(format!("rift-ai-{}-{at}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the models directory");
        for file in files {
            std::fs::write(dir.join(file), b"weights").expect("a model file");
        }
        let manifest = Manifest::parse(SAMPLE).expect("the manifest");
        let picture = Picture::of(&manifest, &dir, status);
        std::fs::remove_dir_all(&dir).expect("the models directory");
        picture
    }

    fn settings(picture: Picture, tier: &str) -> Settings {
        let mut state = Settings::bare();
        state.quasar = Some(Box::new(picture));
        state.host = Some(Ok(librift::orbit::Host {
            fingerprint: String::new(),
            class: "owned".to_string(),
            outputs: Vec::new(),
            gpu_path: "none".to_string(),
            ai_tier: tier.to_string(),
        }));
        state
    }

    fn ids(picture: &Picture) -> Vec<&str> {
        picture.models.iter().map(|one| one.id.as_str()).collect()
    }

    #[test]
    fn every_size_says_what_it_is_and_which_model_it_asks_for() {
        let picture = picture("sizes", &["small.gguf"], Ok(ready()));
        assert_eq!(
            picture.sizes,
            [
                (
                    Tier::Small,
                    "For a machine with 4 GB of memory. It runs small.".to_string()
                ),
                (
                    Tier::Medium,
                    "For a machine with 8 GB of memory. It runs medium.".to_string()
                ),
                (
                    Tier::Large,
                    "For a machine with 16 GB of memory. It runs medium.".to_string()
                ),
            ]
        );
        // a manifest with nothing small enough says what the machine is and no more
        let empty = Manifest::default();
        assert_eq!(
            about_a_size(&empty, Tier::Small),
            "For a machine with 4 GB of memory."
        );
    }

    #[test]
    fn the_test_model_is_listed_only_where_it_is_the_one_on_the_drive() {
        let real = picture("real", &["small.gguf", "medium.gguf"], Ok(ready()));
        assert_eq!(ids(&real), ["small", "medium"]);
        assert_eq!(real.models[0].about, "1.7B, Q4_K_M, 1.1 GB");
        assert!(real.models[0].here && real.models[1].here);
        // the machine the boot test runs on: the test model is the only one there
        let vm = picture("vm", &["tiny.gguf"], Ok(ready()));
        assert_eq!(ids(&vm), ["tiny", "small", "medium"]);
        assert!(vm.models[0].here && !vm.models[1].here);
    }

    #[test]
    fn the_state_says_what_quasar_says_and_which_size_is_set() {
        let kept = settings(picture("state", &["small.gguf"], Ok(ready())), "small");
        assert_eq!(
            state(&kept),
            ["ai ready", "model small", "models 1", "tier small"]
        );
        // nothing has answered yet, and a failure is on the page, not in the state
        assert!(state(&Settings::bare()).is_empty());
        let quiet = settings(
            picture("quiet", &[], Err("Quasar is not running.".into())),
            "medium",
        );
        assert_eq!(state(&quiet), ["models 0", "tier medium"]);
    }

    #[test]
    fn a_model_that_has_not_loaded_says_none() {
        let loading = Status {
            state: "loading".into(),
            ..ready()
        };
        let kept = settings(picture("loading", &["small.gguf"], Ok(loading)), "small");
        assert_eq!(state(&kept)[0], "ai loading");
        let nothing = Status {
            state: "none".into(),
            model: String::new(),
            error: "No chat model that fits this machine is on the drive.".into(),
            ..ready()
        };
        let bare = settings(picture("none", &[], Ok(nothing)), "small");
        assert_eq!(state(&bare)[..2], ["ai none", "model none"]);
    }

    #[test]
    fn a_state_word_reads_as_words() {
        assert_eq!(word("ready"), "Ready");
        assert_eq!(word("loading"), "Loading");
        assert_eq!(word("failed"), "Failed");
        assert_eq!(word("none"), "Not running");
        assert_eq!(word(""), "Not running");
    }

    #[test]
    fn a_size_is_named_by_its_word_however_it_was_typed() {
        assert_eq!(named(" Medium \n"), Some(Tier::Medium));
        assert_eq!(named("LARGE"), Some(Tier::Large));
        assert_eq!(named("huge"), None);
    }

    #[test]
    fn the_page_says_when_quasar_is_running_the_model_for_another_size() {
        let same = picture("told", &["small.gguf"], Ok(ready()));
        assert_eq!(told(&same, Some(Tier::Small)), [WHEN]);
        // the size was set to another one since Quasar started
        assert_eq!(told(&same, Some(Tier::Large)), [WHEN, UNTIL]);
        // and nothing is claimed while Quasar has not answered
        let quiet = picture("silent", &[], Err("Quasar is not running.".into()));
        assert_eq!(told(&quiet, Some(Tier::Large)), [WHEN]);
    }

    #[test]
    fn every_size_has_a_name_and_the_sentences_are_sentences() {
        let labels = Tier::ALL.map(label);
        assert_eq!(labels, ["Small", "Medium", "Large"]);
        for sentence in [WHEN, UNTIL] {
            assert!(sentence.ends_with('.'), "{sentence}");
            assert!(sentence.is_ascii(), "{sentence}");
        }
    }
}
