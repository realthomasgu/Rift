//! `rift host`: what Orbit remembers about this machine, one row per setting, or one value by
//! itself. `rift host set` writes the settings a person decides into the profile, through Orbit,
//! which is the only thing that writes that file. `rift host auto-unlock` reads and sets whether
//! this machine's tpm opens the drive without its passphrase, which is Vault's to do, and `rift
//! host keys`, `enroll-key` and `remove-key` are the security keys that open it.

use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

use librift::orbit::{self, Host, Output};
use librift::vault::{self, AutoUnlock};

use crate::text;

const USAGE: &str = "Usage: rift host [class | tier]\n       rift host set <class | tier | gpu> \
<value>\n       rift host set scale <screen> <1 or 2>\n       rift host auto-unlock [on | off]\n\
       rift host keys\n       sudo rift host enroll-key\n       rift host remove-key <keyslot>";

const HELP: &str = "Shows what Orbit remembers about this machine. class prints the host class \
(owned, trusted or borrowed) by itself, and tier the AI tier. set writes one of them into this \
machine's profile: the class, the AI tier, the graphics path, or the size a screen is drawn at. \
auto-unlock says whether this machine's tpm opens the drive without its passphrase, and on or off \
seals a key to it or wipes the one there is. Only a machine whose class is owned may hold one, and \
the passphrase always opens the drive, here and anywhere else. keys says which security keys open \
the drive, enroll-key adds the one that is plugged in, and remove-key takes one off by the keyslot \
keys prints. A security key belongs to the drive and not to a machine, so any number of them may \
be enrolled and they work on every machine.";

pub fn run(args: &[String]) -> ExitCode {
    let one = match args {
        [] => None,
        [arg] if arg == "--help" || arg == "-h" => {
            println!("{USAGE}\n\n{HELP}");
            return ExitCode::SUCCESS;
        }
        [arg] if field(arg).is_some() => field(arg),
        [arg, rest @ ..] if arg == "set" => return set(rest),
        [arg, rest @ ..] if arg == "auto-unlock" => return auto_unlock(rest),
        [arg, rest @ ..] if arg == "keys" => return keys(rest),
        [arg, rest @ ..] if arg == "enroll-key" => return enroll_key(rest),
        [arg, rest @ ..] if arg == "remove-key" => return remove_key(rest),
        [arg, rest @ ..] => return text::unknown("host", rest.first().unwrap_or(arg), USAGE),
    };
    match orbit::host() {
        Ok(host) => {
            match one {
                Some(value) => println!("{}", value(host)),
                None => print!("{}", text::table(&rows(&host))),
            }
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

/// `rift host set <name> <value>`, and `rift host set scale <screen> <1 or 2>`. Orbit writes it
/// into the `[set]` layer of this machine's profile and says so on the bus.
fn set(args: &[String]) -> ExitCode {
    let written = match args {
        [name, value] if name != "scale" => orbit::set(name, value).map(|()| {
            println!("{name} is {value} for this machine.");
        }),
        [name, screen, size] if name == "scale" => match size.trim().parse::<u32>() {
            Ok(scale) => orbit::set_display_scale(screen, scale).map(|()| {
                // the compositor reads the scale from a part of its config under home, which the
                // session writes when it starts. a shell with no home of its own is no reason to
                // fail: the profile is where the setting lives
                let _ = orbit::follow();
                println!("{screen} is drawn at scale {scale}.");
            }),
            Err(_) => Err(format!(
                "\"{}\" is not a size a screen is drawn at. It is 1 or 2.",
                size.trim()
            )),
        },
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

/// The value `rift host <name>` prints by itself.
fn field(name: &str) -> Option<fn(Host) -> String> {
    match name {
        "class" => Some(|host| host.class),
        "tier" => Some(|host| host.ai_tier),
        _ => None,
    }
}

fn rows(host: &Host) -> Vec<(&'static str, String)> {
    let mut rows = vec![
        ("Fingerprint", host.fingerprint.clone()),
        ("Class", host.class.clone()),
    ];
    if host.outputs.is_empty() {
        rows.push(("Display", "none".into()));
    }
    for output in &host.outputs {
        rows.push(("Display", describe(output)));
    }
    rows.push(("GPU path", host.gpu_path.clone()));
    rows.push(("AI tier", host.ai_tier.clone()));
    rows
}

fn describe(output: &Output) -> String {
    let Output {
        connector,
        width,
        height,
        scale,
        ..
    } = output;
    if *width == 0 {
        format!("{connector}, no EDID, scale {scale}")
    } else {
        format!("{connector}, {width}x{height}, scale {scale}")
    }
}

/// `rift host auto-unlock [on | off]`. With no word it says what the drive does at boot. `on`
/// asks for the drive's passphrase and has Vault seal a key for it to this machine's tpm; `off`
/// wipes that key. The passphrase slot is never touched, so the drive always opens with it.
fn auto_unlock(args: &[String]) -> ExitCode {
    let wanted = match args {
        [] => None,
        [word] if word == "on" => Some(true),
        [word] if word == "off" => Some(false),
        [word, ..] => {
            eprintln!("rift host: `{word}` is not on or off\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let read = match vault::auto_unlock() {
        Ok(read) => read,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::FAILURE;
        }
    };
    let (state, has_tpm) = read;
    let Some(on) = wanted else {
        for line in says(&state, has_tpm) {
            println!("{line}");
        }
        return ExitCode::SUCCESS;
    };
    if on && state.here() {
        println!("This drive already opens by itself on this machine.");
        return ExitCode::SUCCESS;
    }
    if !on && matches!(state, AutoUnlock::Off) {
        println!("This drive already asks for its passphrase at every boot.");
        return ExitCode::SUCCESS;
    }
    // vault refuses both of these itself, since it is the one that has to. asking here as well
    // means a passphrase is not typed for an answer that is already known
    if on && !has_tpm {
        eprintln!("{}", vault::NO_TPM);
        return ExitCode::FAILURE;
    }
    if on {
        match orbit::host() {
            Ok(host) if host.class != "owned" => {
                eprintln!("{}", vault::not_owned(&host.class));
                return ExitCode::FAILURE;
            }
            Ok(_) => {}
            Err(why) => {
                eprintln!("{why}");
                return ExitCode::FAILURE;
            }
        }
    }
    let passphrase = if on {
        match text::hidden("Type this drive's passphrase: ") {
            Some(typed) if !typed.is_empty() => typed,
            Some(_) => {
                eprintln!(
                    "A key is sealed with the passphrase that already opens the drive, so it \
                           cannot be empty."
                );
                return ExitCode::FAILURE;
            }
            None => {
                eprintln!(
                    "Sealing a key needs the drive's passphrase, and there is no terminal to \
                           type it on."
                );
                return ExitCode::FAILURE;
            }
        }
    } else {
        String::new()
    };
    match vault::set_auto_unlock(on, &passphrase) {
        Ok(()) => {
            for line in did(on, &state) {
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

/// `rift host keys`: the security keys that open the drive, one line each.
fn keys(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        eprintln!(
            "rift host keys: `{}` is not one of its words\n{USAGE}",
            args[0]
        );
        return ExitCode::from(2);
    }
    match vault::security_keys() {
        Ok(keys) => {
            for line in vault::keys_read_as(&keys) {
                println!("{line}");
            }
            if keys.is_empty() {
                println!("sudo rift host enroll-key adds the one that is plugged in.");
            } else {
                println!("Its passphrase opens it as well, with a key or without one.");
            }
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

/// `sudo rift host enroll-key`: the security key that is plugged in, added to the drive. Vault does
/// it as root in this terminal, because the key is touched and its pin typed while it waits, so
/// this runs `vault enroll-key` in its place.
fn enroll_key(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        eprintln!(
            "rift host enroll-key: `{}` is not one of its words\n{USAGE}",
            args[0]
        );
        return ExitCode::from(2);
    }
    let error = Command::new("vault").arg("enroll-key").exec();
    eprintln!("Could not run vault: {error}");
    ExitCode::FAILURE
}

/// `rift host remove-key <keyslot>`: one security key taken off the drive.
fn remove_key(args: &[String]) -> ExitCode {
    let [slot] = args else {
        eprintln!("rift host remove-key: one keyslot, as rift host keys prints it\n{USAGE}");
        return ExitCode::from(2);
    };
    let Ok(slot) = slot.trim().parse::<u32>() else {
        eprintln!(
            "rift host remove-key: `{}` is not a keyslot. rift host keys prints them.",
            slot.trim()
        );
        return ExitCode::from(2);
    };
    match vault::remove_security_key(slot) {
        Ok(()) => {
            println!("The security key in keyslot {slot} no longer opens this drive.");
            println!("Its passphrase still does.");
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

/// What `rift host auto-unlock` prints about the drive as it is.
fn says(state: &AutoUnlock, has_tpm: bool) -> Vec<String> {
    match state {
        AutoUnlock::Here => vec!["This drive opens by itself on this machine.".to_string()],
        AutoUnlock::Elsewhere(machine) => {
            let which = text::short(machine);
            let named = if which.is_empty() {
                "another machine".to_string()
            } else {
                format!("machine {which}")
            };
            vec![format!(
                "This drive opens by itself on {named}, and asks for its passphrase here."
            )]
        }
        AutoUnlock::Off if has_tpm => vec![
            "This drive asks for its passphrase at every boot.".to_string(),
            "rift host auto-unlock on seals a key to this machine's tpm.".to_string(),
        ],
        AutoUnlock::Off => vec![
            "This drive asks for its passphrase at every boot.".to_string(),
            "This machine has no tpm, so there is nothing to seal a key to.".to_string(),
        ],
    }
}

/// What it prints once Vault has sealed a key or wiped one.
fn did(on: bool, before: &AutoUnlock) -> Vec<String> {
    if !on {
        return vec!["This drive asks for its passphrase at every boot again.".to_string()];
    }
    let mut said = vec!["This drive now opens by itself on this machine.".to_string()];
    if let AutoUnlock::Elsewhere(machine) = before {
        let which = text::short(machine);
        said.push(if which.is_empty() {
            "The machine that held the key before no longer does.".to_string()
        } else {
            format!("Machine {which} held the key before and no longer does.")
        });
    }
    said.push("Its passphrase still opens it, here and on any other machine.".to_string());
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(outputs: Vec<Output>) -> Host {
        Host {
            fingerprint: "5297c0f65d6a".repeat(5) + "abcd",
            class: "borrowed".into(),
            outputs,
            gpu_path: "none".into(),
            ai_tier: "small".into(),
        }
    }

    #[test]
    fn a_virtual_machine_reads_as_rows() {
        let qemu = host(vec![Output {
            connector: "Virtual-1".into(),
            width: 1280,
            height: 800,
            width_cm: 32,
            height_cm: 20,
            scale: 1,
        }]);
        let expected = format!(
            "Fingerprint: {}\nClass:       borrowed\nDisplay:     Virtual-1, 1280x800, scale 1\n\
             GPU path:    none\nAI tier:     small\n",
            qemu.fingerprint
        );
        assert_eq!(text::table(&rows(&qemu)), expected);
    }

    #[test]
    fn every_output_gets_a_row() {
        let laptop = host(vec![
            Output {
                connector: "eDP-1".into(),
                width: 2880,
                height: 1800,
                width_cm: 30,
                height_cm: 19,
                scale: 2,
            },
            Output {
                connector: "HDMI-A-1".into(),
                width: 0,
                height: 0,
                width_cm: 0,
                height_cm: 0,
                scale: 1,
            },
        ]);
        let displays: Vec<String> = rows(&laptop)
            .into_iter()
            .filter(|(label, _)| *label == "Display")
            .map(|(_, value)| value)
            .collect();
        assert_eq!(
            displays,
            ["eDP-1, 2880x1800, scale 2", "HDMI-A-1, no EDID, scale 1"]
        );
        let headless = rows(&host(Vec::new()));
        assert!(headless.contains(&("Display", "none".to_string())));
    }

    #[test]
    fn what_the_drive_does_at_boot_reads_as_a_sentence() {
        assert_eq!(
            says(&AutoUnlock::Here, true),
            ["This drive opens by itself on this machine."]
        );
        let other = says(&AutoUnlock::Elsewhere("5297c0f65d6a34ff".into()), true);
        assert_eq!(
            other,
            [
                "This drive opens by itself on machine 5297c0f65d6a, and asks for its passphrase here."
            ]
        );
        let nameless = says(&AutoUnlock::Elsewhere(String::new()), true);
        assert!(nameless[0].contains("another machine"));
        let off = says(&AutoUnlock::Off, true);
        assert_eq!(off[0], "This drive asks for its passphrase at every boot.");
        assert!(off[1].contains("rift host auto-unlock on"));
        assert!(says(&AutoUnlock::Off, false)[1].contains("no tpm"));
    }

    #[test]
    fn sealing_a_key_says_which_machine_held_it_before() {
        let fresh = did(true, &AutoUnlock::Off);
        assert_eq!(fresh[0], "This drive now opens by itself on this machine.");
        assert_eq!(fresh.len(), 2);
        assert!(fresh[1].contains("passphrase still opens it"));
        let moved = did(true, &AutoUnlock::Elsewhere("5297c0f65d6a34ff".into()));
        assert_eq!(moved.len(), 3);
        assert_eq!(
            moved[1],
            "Machine 5297c0f65d6a held the key before and no longer does."
        );
        assert_eq!(
            did(false, &AutoUnlock::Here),
            ["This drive asks for its passphrase at every boot again."]
        );
    }

    #[test]
    fn class_and_tier_print_one_value_each() {
        let value = |name| field(name).map(|get| get(host(Vec::new())));
        assert_eq!(value("class").as_deref(), Some("borrowed"));
        assert_eq!(value("tier").as_deref(), Some("small"));
        assert_eq!(value("fingerprint"), None);
    }
}
