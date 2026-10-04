//! rift-welcome: what a new drive shows its owner once, after the first unlock, and again from the
//! Applications menu. It sets how the desktop looks, installs the apps Rift suggests from
//! Flathub, and shows how to run more programming languages.
//!
//! `rift-welcome` opens the window, or brings up the one that is open. `rift-welcome --login` is
//! what the session starts at every login: it opens only on a drive that has not been welcomed, and
//! never in Ghost mode, where nothing it sets would be kept.
//! `rift-welcome --page <name>` opens or shows one page, `rift-welcome --set <name> <value>` does
//! what pressing it on its page would, and `rift-welcome --state` prints the page that is up and
//! what the window knows.

mod appearance;
mod apps;
mod control;
mod developer;
mod done;
mod install;
mod note;
mod page;
mod start;
mod ui;

use std::path::PathBuf;
use std::process::ExitCode;

use control::Command;
use page::Page;
// the colours, the rows and the icons, which Settings draws with too
use rift_ui::{icons, theme, widgets};

const USAGE: &str = "Usage: rift-welcome [--login | --page <name>] [--screenshot <png>]\n       rift-welcome [--set <name> <value> | --state]";

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
            println!("rift-welcome {}", librift::VERSION);
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
        ["--login"] => login(),
        ["--page", word] => match Page::from_word(word) {
            Some(page) => open(Some(page), None),
            None => fail(&format!(
                "rift-welcome: there is no page called {word}. The pages are {}.",
                words()
            )),
        },
        ["--screenshot", png] => open(None, Some(PathBuf::from(*png))),
        ["--page", word, "--screenshot", png] => match Page::from_word(word) {
            Some(page) => open(Some(page), Some(PathBuf::from(*png))),
            None => fail(&format!(
                "rift-welcome: there is no page called {word}. The pages are {}.",
                words()
            )),
        },
        [] => open(None, None),
        [other, ..] => fail(&format!("rift-welcome: unknown option {other}\n{USAGE}")),
    }
}

/// At a login: open on a drive that has not been welcomed, and on no other.
///
/// Never in Ghost mode. The note that says a drive has been welcomed is in home, and in Ghost mode
/// home is memory, so there is never a note and Welcome would open at every Ghost login. It would
/// also be offering to keep things a session that keeps nothing cannot: the wallpaper, the accent
/// and the Flatpaks all go when the machine does. It still opens from the Applications menu, and the
/// start page says there what is not kept.
fn login() -> ExitCode {
    if librift::ghost::on() || note::welcomed() || control::already_open() {
        return ExitCode::SUCCESS;
    }
    run(ui::Start::default())
}

/// Open the window, or ask the Welcome that is running to open or show a page. A session has one
/// Welcome. A window that is only there to have its picture taken opens whatever else is running.
fn open(page: Option<Page>, screenshot: Option<PathBuf>) -> ExitCode {
    if screenshot.is_none() && control::already_open() {
        return tell(&page.map_or(Command::Open, |page| Command::Page(page.word().to_string())));
    }
    run(ui::Start { page, screenshot })
}

fn run(start: ui::Start) -> ExitCode {
    match ui::run(start) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => fail(&format!("rift-welcome: {why}")),
    }
}

/// Send one line to the Welcome that is running.
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
