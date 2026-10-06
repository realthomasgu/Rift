//! rift: the CLI. `host`, `ai`, `doctor`, `snapshot`, `backup`, `net` and `update` ask the same
//! D-Bus services Lens uses, `clone` runs Vault as root, `run --sandbox` runs Airlock, `wallpaper`
//! writes the owner's setting and Horizon's part of the config, and `guide` opens the guide that
//! comes with the system. The other commands are only a line in the help so far.

mod ai;
mod backup;
mod clone;
mod doctor;
mod guide;
mod hardware;
mod host;
mod net;
mod restore;
mod run;
mod search;
mod session;
mod snapshot;
mod text;
mod update;
mod version;
mod wallpaper;

use std::process::ExitCode;

/// Subcommands and the phase in which each becomes real.
const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "update",
        "Install the next version into the slot that is not running",
        "Phase 3",
    ),
    ("rollback", "Boot the previous system slot", "Phase 2"),
    (
        "snapshot",
        "List, take and restore from Timeline snapshots",
        "Phase 2",
    ),
    (
        "backup",
        "List, make and restore from encrypted backups",
        "Phase 2",
    ),
    (
        "clone",
        "Write a complete second drive with a fresh key",
        "Phase 2",
    ),
    (
        "host",
        "Show or set what Orbit remembers about this machine",
        "Phase 1",
    ),
    (
        "ai",
        "Ask Quasar a question, search home by meaning, say words or write down speech",
        "Phase 1",
    ),
    ("run", "Run a command inside a Airlock sandbox", "Phase 2"),
    (
        "net",
        "Turn the network off or on for an app in a sandbox",
        "Phase 2",
    ),
    (
        "session",
        "Show the windows that were open, and where each one stood",
        "Phase 2",
    ),
    (
        "doctor",
        "Check the drive, the host, and the services",
        "Phase 1",
    ),
    (
        "wallpaper",
        "List the wallpapers, or set a photograph, a picture or a colour",
        "Phase 1",
    ),
    (
        "guide",
        "Open the guide that comes with the system",
        "Phase 1",
    ),
    ("flash", "Write Rift onto a drive", "Phase 2"),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        None | Some("--help" | "-h" | "help") => {
            usage();
            ExitCode::SUCCESS
        }
        Some("--version" | "-V") => version::run(rest),
        Some("update") => update::run(rest),
        Some("host") => host::run(rest),
        Some("ai") => ai::run(rest),
        Some("doctor") => doctor::run(rest),
        Some("snapshot") => snapshot::run(rest),
        Some("backup") => backup::run(rest),
        Some("clone") => clone::run(rest),
        Some("run") => run::run(rest),
        Some("net") => net::run(rest),
        Some("session") => session::run(rest),
        Some("wallpaper") => wallpaper::run(rest),
        Some("guide") => guide::run(rest),
        Some(cmd) => {
            if let Some((name, what, phase)) = COMMANDS.iter().find(|(name, _, _)| *name == cmd) {
                eprintln!("rift {name}: {what}. Not implemented yet ({phase}).");
            } else {
                eprintln!("rift: unknown command `{cmd}`\n");
                usage();
            }
            ExitCode::from(2)
        }
    }
}

fn usage() {
    println!(
        "rift {}\nYour computer, in your pocket.\n",
        librift::VERSION
    );
    println!("Usage: rift <command> [args]\n\nCommands:");
    for (name, what, _) in COMMANDS {
        println!("  {name:<10} {what}");
    }
}
