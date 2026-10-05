//! `rift net`: the network switch Airlock keeps for each app that runs in a sandbox, Flatpak apps
//! among them. Lists the apps it knows, and turns an app's network off or on.

use std::fmt::Write as _;
use std::process::ExitCode;

use librift::airlock::{self, App};

use crate::text;

const USAGE: &str = "Usage: rift net [list]\n       rift net off <app>\n       rift net on <app>";

const HELP: &str = "Airlock keeps a network switch for each app that runs in a sandbox. off \
takes the network away from an app, at once in the sandboxes it runs in now and in every one it \
starts later, until on gives it back. A Flatpak app is named by its id and is on the switch as \
soon as it is installed; the others are named after the command, or by --name when rift run \
--sandbox starts them. list shows every Flatpak app that is installed, every app whose network \
is off, and every app that runs in a sandbox now.";

pub fn run(args: &[String]) -> ExitCode {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] | ["list"] => list(),
        ["--help" | "-h", ..] => {
            println!("{USAGE}\n\n{HELP}");
            ExitCode::SUCCESS
        }
        [switch @ ("off" | "on"), app] => set(app, *switch == "on"),
        [switch @ ("off" | "on")] => {
            eprintln!("rift net {switch}: the name of an app is needed\n{USAGE}");
            ExitCode::from(2)
        }
        ["list", extra, ..] | ["off" | "on", _, extra, ..] | [extra, ..] => {
            text::unknown("net", extra, USAGE)
        }
    }
}

fn list() -> ExitCode {
    match airlock::apps() {
        Ok(apps) if apps.is_empty() => {
            println!(
                "No app is on the switch: no Flatpak app is installed, none runs in a sandbox, \
                 and every app has the network."
            );
            ExitCode::SUCCESS
        }
        Ok(apps) => {
            print!("{}", rows(&apps));
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

fn set(app: &str, on: bool) -> ExitCode {
    if let Some(why) = airlock::name_problem(app) {
        eprintln!("{why}");
        return ExitCode::from(2);
    }
    match airlock::set_network(app, on) {
        Ok(running) => {
            println!("{}", switched(app, on, running));
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

/// What turning the switch says.
fn switched(app: &str, on: bool, running: u32) -> String {
    let state = if on { "on" } else { "off" };
    match running {
        0 => format!("The network is {state} for {app}."),
        1 => format!("The network is {state} for {app}, also in the sandbox it runs in now."),
        more => {
            format!(
                "The network is {state} for {app}, also in the {more} sandboxes it runs in now."
            )
        }
    }
}

/// What kind of app a row is: a Flatpak app by its id, or a command `rift run --sandbox` started.
fn kind(app: &App) -> &'static str {
    if app.flatpak { "Flatpak" } else { "Command" }
}

/// The apps under a header, their columns lined up.
fn rows(apps: &[App]) -> String {
    let width = apps
        .iter()
        .map(|app| app.name.len())
        .chain(["App".len()])
        .max()
        .unwrap_or(0);
    let mut out = format!("{:<width$}  Kind     Network  Running\n", "App");
    for app in apps {
        let network = if app.network { "On" } else { "Off" };
        let _ = writeln!(
            out,
            "{:<width$}  {:<7}  {network:<7}  {}",
            app.name,
            kind(app),
            app.running
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_says_where_it_took_effect() {
        assert_eq!(switched("curl", false, 0), "The network is off for curl.");
        assert_eq!(
            switched("fetcher", false, 1),
            "The network is off for fetcher, also in the sandbox it runs in now."
        );
        assert_eq!(
            switched("fetcher", true, 3),
            "The network is on for fetcher, also in the 3 sandboxes it runs in now."
        );
    }

    #[test]
    fn apps_line_up_under_a_header() {
        let app = |name: &str, network, running, flatpak| App {
            name: name.to_string(),
            network,
            running,
            flatpak,
        };
        assert_eq!(
            rows(&[
                app("dev.rift.TestApp", false, 1, true),
                app("fetcher", true, 0, false),
            ]),
            "App               Kind     Network  Running\n\
             dev.rift.TestApp  Flatpak  Off      1\n\
             fetcher           Command  On       0\n"
        );
        assert_eq!(
            rows(&[app("hx", false, 1, false)]),
            "App  Kind     Network  Running\nhx   Command  Off      1\n"
        );
    }
}
