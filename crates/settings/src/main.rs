//! rift-settings: the Settings app. A window with a sidebar of pages and one page beside it, in
//! the style of the rest of the desktop.
//!
//! `rift-settings` opens the window, or brings the one that is already open to the page it is on.
//! `rift-settings --page <name>` opens or shows one page by name, `rift-settings --set <name>
//! <value>` changes one setting the way pressing it on its page would, and
//! `rift-settings --state` prints the page that is up and every setting it writes.

mod about;
mod accessibility;
mod ai;
mod appearance;
mod apps;
mod backups;
mod bluetooth;
mod control;
mod datetime;
mod displays;
mod dock;
mod keyboard;
mod net;
mod notifications;
mod owner;
mod page;
mod pointer;
mod power;
mod printers;
mod privacy;
mod region;
mod search;
mod sound;
mod ui;
mod unlocking;
mod updates;
mod watch;

use std::path::PathBuf;
use std::process::ExitCode;

use control::Command;
use page::Page;
// the colours, the rows and the icons, which Welcome draws with too
use rift_ui::{icons, theme, widgets};

const USAGE: &str = "Usage: rift-settings [--page <name>] [--screenshot <png>]\n       rift-settings [--set <name> <value> | --state]\n       rift-settings --set scale <screen> <1 or 2>";

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
            println!("rift-settings {}", librift::VERSION);
            ExitCode::SUCCESS
        }
        ["--state"] => match control::ask(&Command::State) {
            Ok(lines) => {
                print!("{lines}");
                ExitCode::SUCCESS
            }
            Err(why) => fail(&why),
        },
        // a setting takes one word, except the size of a screen, which takes the screen and then
        // the size: `--set scale eDP-1 2`
        ["--set", name, rest @ ..] if !rest.is_empty() => {
            tell(&Command::Set((*name).to_string(), rest.join(" ")))
        }
        ["--page", word] => match Page::from_word(word) {
            Some(page) => open(Some(page), None),
            None => fail(&format!(
                "rift-settings: there is no page called {word}. The pages are {}.",
                words()
            )),
        },
        ["--screenshot", png] => open(None, Some(PathBuf::from(*png))),
        ["--page", word, "--screenshot", png] => match Page::from_word(word) {
            Some(page) => open(Some(page), Some(PathBuf::from(*png))),
            None => fail(&format!(
                "rift-settings: there is no page called {word}. The pages are {}.",
                words()
            )),
        },
        [] => open(None, None),
        [other, ..] => fail(&format!("rift-settings: unknown option {other}\n{USAGE}")),
    }
}

/// Open the window, or show the page on the one that is already open. A session has one Settings
/// window, the way it has one of every other app that keeps state on screen. A window that is only
/// there to have its picture taken is not one of those, so it opens whatever else is up.
fn open(page: Option<Page>, screenshot: Option<PathBuf>) -> ExitCode {
    if screenshot.is_none() && control::already_open() {
        if let Some(page) = page {
            return tell(&Command::Page(page.word().to_string()));
        }
        println!("Settings is already open.");
        return ExitCode::SUCCESS;
    }
    match ui::run(ui::Start { page, screenshot }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => fail(&format!("rift-settings: {why}")),
    }
}

/// Send one line to the window that is open.
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

/// Every page's word, for the help and for a word that is not one.
fn words() -> String {
    Page::ALL
        .iter()
        .map(|page| page.word())
        .collect::<Vec<_>>()
        .join(", ")
}
