//! `rift update`: the next version into the slot that is not running.
//!
//! Vault does the work as root, since it writes a partition of the drive, and this prints what it
//! would cost before it starts and what it wrote when it is done. `--check` stops after the plan.

use std::process::ExitCode;

use librift::update::Plan;
use librift::vault;

const USAGE: &str = "Usage: rift update [--check]";

const HELP: &str = "Installs the version waiting where updates come from into the slot that is \
not running, so the version you are on now stays where it is. Only the parts of it this drive \
does not have already are fetched. The new version gets three boots to prove itself: if it does \
not reach the desktop in any of them, the next boot starts the old one again. --check says what \
an update would cost and installs nothing.";

pub fn run(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            println!("{USAGE}\n\n{HELP}");
            ExitCode::SUCCESS
        }
        None => install(),
        Some("--check") if args.len() == 1 => check(),
        Some(other) => crate::text::unknown("update", other, USAGE),
    }
}

/// What an update would do, and nothing more.
fn check() -> ExitCode {
    match vault::next_version() {
        Ok(plan) => {
            said(&plan);
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

/// The plan, then the install.
fn install() -> ExitCode {
    let plan = match vault::next_version() {
        Ok(plan) => plan,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };
    said(&plan);
    if !plan.waiting() {
        return ExitCode::SUCCESS;
    }
    match vault::update() {
        Ok(written) => {
            for line in written.said() {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

/// The plan, line by line.
fn said(plan: &Plan) {
    for line in plan.said() {
        println!("{line}");
    }
}
