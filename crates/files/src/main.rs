//! rift-files: Files, the file manager. A window for each folder, with the places down the left,
//! what is in the folder in a list, and the trash; files open with the apps that open their kind.
//!
//! The header bar searches the folder and what is under it, by name as it is typed and by meaning
//! on Enter, and the Timeline shows the folder as it was at one of Vault's snapshots of home. A
//! folder is shown as a list of rows or as a grid of the pictures of its files.
//!
//! `rift-files` opens a window on home, and `rift-files <folder>` one on that folder, or on the
//! folder of a file with the file selected; with Files running, it asks that one for the window.
//! `rift-files --set <name> <value>` does what pressing it would in the window in front, and
//! `rift-files --state` prints what that window shows. `rift-files --bus` is what the session bus
//! starts when an app asks for the file manager: the app with no window until the call says what
//! to show.

mod actions;
mod browser;
// the FileManager1 interface on the session bus, which is a bus and so a linux one
#[cfg(target_os = "linux")]
mod bus;
mod control;
mod dialogs;
mod find;
mod grid;
mod jobs;
mod keys;
mod list;
mod menus;
mod props;
mod thumbs;
mod timeline;
mod ui;
mod view;

use std::path::PathBuf;
use std::process::ExitCode;

use control::Command;
// the colours, the rows, the menus and the icons, which Settings and Welcome draw with too
use rift_ui::{icons, theme, widgets};

const USAGE: &str = "Usage: rift-files [<folder or file>...] [--screenshot <png>]\n       rift-files [--bus | --set <name> <value> | --state]";

fn main() -> ExitCode {
    let args = pretending(std::env::args().skip(1).collect());
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--help" | "-h"] => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        ["--version"] => {
            println!("rift-files {}", librift::VERSION);
            ExitCode::SUCCESS
        }
        ["--bus"] => serve(),
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
        [paths @ .., "--screenshot", png] => open(paths, Some(PathBuf::from(*png))),
        [other, ..] if other.starts_with("--") => {
            fail(&format!("rift-files: unknown option {other}\n{USAGE}"))
        }
        paths => open(paths, None),
    }
}

/// Takes `--as-ghost` off the front and makes this process draw as a Ghost session does: the
/// drive's own exchange partition listed as a place nothing has mounted. That is how the picture of
/// the sidebar is taken on a machine that has no Rift drive to boot at all. Only in a debug build:
/// the image is built in release, where the flag is unknown and the kernel command line stays the
/// only way a boot is a Ghost one (ADR-0085).
#[cfg(debug_assertions)]
fn pretending(args: Vec<String>) -> Vec<String> {
    match args.split_first() {
        Some((first, rest)) if first == "--as-ghost" => {
            librift::ghost::pretend();
            ui::as_ghost();
            rest.to_vec()
        }
        _ => args,
    }
}

/// Every argument, as they were typed.
#[cfg(not(debug_assertions))]
fn pretending(args: Vec<String>) -> Vec<String> {
    args
}

/// Answer on the session bus with no window open, which is how the bus starts Files for a call to
/// `org.freedesktop.FileManager1`. A Files that is already running has the name, so there is
/// nothing to do.
fn serve() -> ExitCode {
    if control::already_open() {
        return ExitCode::SUCCESS;
    }
    match ui::run(ui::Start {
        bus: true,
        ..ui::Start::default()
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => fail(&format!("rift-files: {why}")),
    }
}

/// Open a window for each path, or on home with none, or ask the Files that is running for them. A
/// window that is only there to have its picture taken opens whatever else is running.
fn open(given: &[&str], screenshot: Option<PathBuf>) -> ExitCode {
    let paths: Vec<PathBuf> = given.iter().map(|given| path_of(given)).collect();
    if screenshot.is_none() && control::already_open() {
        if paths.is_empty() {
            return tell(&Command::Open(String::new()));
        }
        for path in &paths {
            if let code @ ExitCode::FAILURE = tell(&Command::Open(path.display().to_string())) {
                return code;
            }
        }
        return ExitCode::SUCCESS;
    }
    match ui::run(ui::Start {
        open: paths,
        screenshot,
        bus: false,
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => fail(&format!("rift-files: {why}")),
    }
}

/// What a path on the command line names. The trash's own address is passed on as it is, since it
/// is no place on a disk; anything else is made absolute, so a folder named from a terminal is the
/// one that was meant wherever the app is started from.
fn path_of(given: &str) -> PathBuf {
    let path = librift::files::path_of(given);
    if librift::files::is_trash(&path) {
        return path;
    }
    std::path::absolute(&path).unwrap_or(path)
}

/// Send one line to the Files that is running.
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
