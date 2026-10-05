//! rift-store: where the owner finds apps. The front page lists the apps Rift suggests, the field
//! searches every remote the drive has, and an app's own page says how big it is and what it asks
//! for before anything is downloaded. Apps go into the system installation, so the snapshots and the
//! backups of home do not carry them.
//!
//! `rift-store` opens the window, or brings up the one that is open. `rift-store --app <id>` opens
//! one app's page, `rift-store --search <words>` searches for them, `rift-store --page <name>` shows
//! one page, `rift-store --set <name> <value>` does what pressing it on its page would, and
//! `rift-store --state` prints the page that is up and what the window knows.

mod app;
mod catalog;
mod control;
mod found;
mod front;
mod installed;
mod jobs;
mod page;
// debug builds only: what the pages draw where there is no flatpak to ask
#[cfg(debug_assertions)]
mod pretend;
mod ui;

use std::path::PathBuf;
use std::process::ExitCode;

use control::Command;
use page::Page;
// the colours, the rows and the icons, which Settings draws with too
use rift_ui::{icons, theme, widgets};

const USAGE: &str = "Usage: rift-store [--page <name> | --app <id> | --search <words>] [--screenshot <png>]\n       rift-store [--set <name> <value> | --state]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--help" | "-h"] => {
            println!("{USAGE}\n\nThe pages are {}.", words());
            ExitCode::SUCCESS
        }
        ["--version"] => {
            println!("rift-store {}", librift::VERSION);
            ExitCode::SUCCESS
        }
        ["--state"] => match control::ask(&Command::State) {
            Ok(lines) => {
                print!("{lines}");
                ExitCode::SUCCESS
            }
            Err(why) => fail(&why),
        },
        ["--set", name, rest @ ..] if !rest.is_empty() => {
            tell(&Command::Set((*name).to_string(), rest.join(" ")))
        }
        // the pages drawn with what a remote said once, for a picture of the window on a machine
        // that is not Rift. debug builds only, and the image is built in release
        #[cfg(debug_assertions)]
        ["--pretend", rest @ ..] => match asked(rest) {
            Ok(start) => run(ui::Start {
                pretend: true,
                ..start
            }),
            Err(why) => fail(&why),
        },
        [] => open(ui::Start::default()),
        rest => match asked(rest) {
            Ok(start) => open(start),
            Err(why) => fail(&why),
        },
    }
}

/// What the options ask for: a page, an app, words to search for, and where to save a picture of
/// the window.
fn asked(args: &[&str]) -> Result<ui::Start, String> {
    let mut start = ui::Start::default();
    let mut at = 0;
    while at < args.len() {
        let value = args.get(at + 1).copied();
        match (args[at], value) {
            ("--page", Some(word)) => {
                start.page = Some(Page::from_word(word).ok_or_else(|| unknown(word))?);
            }
            ("--app", Some(id)) => start.app = Some(id.to_string()),
            ("--search", Some(said)) => start.words = Some(said.to_string()),
            ("--screenshot", Some(png)) => start.screenshot = Some(PathBuf::from(png)),
            (option, _) => return Err(format!("rift-store: unknown option {option}\n{USAGE}")),
        }
        at += 2;
    }
    Ok(start)
}

/// Open the window, or ask the Store that is running to open it, show a page, search or open an app.
/// A session has one Store. A window that is only there to have its picture taken opens whatever
/// else is running.
fn open(start: ui::Start) -> ExitCode {
    if start.screenshot.is_none() && control::already_open() {
        if let Some(said) = start.words {
            return tell(&Command::Set("search".to_string(), said));
        }
        if let Some(id) = start.app {
            return tell(&Command::Set("open".to_string(), id));
        }
        return tell(
            &start
                .page
                .map_or(Command::Open, |page| Command::Page(page.word().to_string())),
        );
    }
    run(start)
}

fn run(start: ui::Start) -> ExitCode {
    match ui::run(start) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => fail(&format!("rift-store: {why}")),
    }
}

/// Send one line to the Store that is running.
fn tell(command: &Command) -> ExitCode {
    match control::ask(command) {
        Ok(_) => ExitCode::SUCCESS,
        Err(why) => fail(&why),
    }
}

fn fail(why: &str) -> ExitCode {
    eprintln!("{why}");
    ExitCode::FAILURE
}

fn unknown(word: &str) -> String {
    format!(
        "rift-store: there is no page called {word}. The pages are {}.",
        words()
    )
}

/// Every page's word, for the help and for a word that is not one.
fn words() -> String {
    Page::ALL
        .iter()
        .map(|page| page.word())
        .collect::<Vec<_>>()
        .join(", ")
}
