//! The Store, driven from a terminal. The app listens on a socket in the session's runtime directory
//! while it runs, with its window open or with an install still going after the window closed.
//! `rift-store` asks it to open the window again, `rift-store --page <name>` shows a page,
//! `rift-store --set <name> <value>` does what pressing it on the page does, and
//! `rift-store --state` prints the page that is up and what the window knows. The runtime
//! directory belongs to one person, so only that person can drive their own window.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

/// The name of the socket inside the runtime directory.
const SOCKET: &str = "rift-store.sock";

/// One line of the protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Open the window, when it is not open.
    Open,
    /// Show this page, by the word in [`crate::page::Page::word`], opening the window for it.
    Page(String),
    /// Do one thing a page does: its name, then its value.
    Set(String, String),
    /// Print the page that is up and what the window knows.
    State,
}

impl Command {
    /// The line that carries this command.
    #[must_use]
    pub fn line(&self) -> String {
        match self {
            Self::Open => "open".to_string(),
            Self::Page(word) => format!("page {word}"),
            Self::Set(name, value) => format!("set {name} {value}"),
            Self::State => "state".to_string(),
        }
    }
}

/// Read one line of the protocol. `None` when it is not one of the four.
#[must_use]
pub fn parse(line: &str) -> Option<Command> {
    let line = line.trim_end_matches(['\r', '\n']);
    let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
    match verb {
        "open" => Some(Command::Open),
        "page" => Some(Command::Page(rest.trim().to_string())),
        "set" => {
            let (name, value) = rest.trim().split_once(' ')?;
            Some(Command::Set(
                name.trim().to_string(),
                value.trim().to_string(),
            ))
        }
        "state" => Some(Command::State),
        _ => None,
    }
}

/// Where the socket is. `None` when the session has no runtime directory.
#[must_use]
pub fn path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join(SOCKET))
}

/// Send one command to the Store that is running, and read back whatever it answers.
///
/// # Errors
///
/// When there is no runtime directory, or no Welcome is listening on the socket.
pub fn ask(command: &Command) -> Result<String, String> {
    let path = path().ok_or("The Store could not find the session runtime directory")?;
    let mut stream = UnixStream::connect(&path)
        .map_err(|e| format!("Could not reach the Store on {}: {e}", path.display()))?;
    writeln!(stream, "{}", command.line()).map_err(|e| format!("Could not write to it: {e}"))?;
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .map_err(|e| format!("Could not read its answer: {e}"))?;
    Ok(answer)
}

/// Whether a Store is already running in this session.
#[must_use]
pub fn already_open() -> bool {
    path().is_some_and(|path| UnixStream::connect(path).is_ok())
}

/// Listen on the socket and hand every command to `each`, writing back what it returns. Blocks;
/// the app calls it on a thread of its own.
///
/// # Errors
///
/// When there is no runtime directory or the socket cannot be opened.
pub fn serve<F: Fn(Command) -> Option<String>>(each: F) -> Result<(), String> {
    let path = path().ok_or("The Store could not find the session runtime directory")?;
    // a socket file an earlier run left behind refuses the bind, and no one else owns this name
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .map_err(|e| format!("Could not listen on {}: {e}", path.display()))?;
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        let answer = parse(&line).and_then(&each);
        if let Some(answer) = answer {
            let _ = stream.write_all(answer.as_bytes());
        }
        let _ = stream.flush();
    }
    Ok(())
}

/// Take the socket file away as the app ends, so the next `rift-store` starts a window of its
/// own instead of knocking on a door no one answers.
pub fn close() {
    if let Some(path) = path() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_reads_back_from_its_line() {
        for command in [
            Command::Open,
            Command::Page("apps".to_string()),
            Command::Set("search".to_string(), "editor".to_string()),
            Command::Set("install".to_string(), "now".to_string()),
            Command::State,
        ] {
            assert_eq!(parse(&command.line()), Some(command.clone()), "{command:?}");
            assert_eq!(parse(&(command.line() + "\n")), Some(command));
        }
    }

    #[test]
    fn a_line_that_is_not_one_of_them_is_nothing() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("quit"), None);
        assert_eq!(parse("set open"), None);
        assert_eq!(parse("page"), Some(Command::Page(String::new())));
    }
}
