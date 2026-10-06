//! `rift doctor --report`: the hardware report for the machine it runs on, as the markdown of one
//! file under `hw/` in Rift's repository.
//!
//! It prints and writes nothing. There is no checkout on a drive to write a file into, the image
//! itself is read only, and a person who wants the file redirects it into one. The name the file
//! wants goes on stderr, so what is on stdout is the file and nothing else.
//!
//! Every fact comes from the kernel through [`librift::hw`], or from Orbit, which has already read
//! the EDID of every screen and decided what this machine can carry. The checks are `rift doctor`'s
//! own, printed inside the report, because what a machine's own checks say is the honest answer to
//! what worked.

use std::fmt::Write as _;
use std::fs;
use std::process::ExitCode;

use librift::hw::{self, Machine, Pci, Startup};
use librift::orbit::{self, Host, Output};
use librift::{drives, ghost, release};

use crate::doctor::{self, Check, Verdict};

pub fn run() -> ExitCode {
    let ghost = ghost::on();
    let checks = doctor::checks(ghost);
    let machine = Machine::read(&hw::Roots::default(), own_disk().as_deref());
    let host = orbit::host().ok();
    let os_release = fs::read_to_string(release::PATH).unwrap_or_default();
    print!(
        "{}",
        Report {
            machine: &machine,
            host: host.as_ref(),
            startup: hw::startup().ok().flatten(),
            checks: &checks,
            ghost,
            date: librift::time::today(),
            version: release::value(&os_release, "IMAGE_VERSION")
                .filter(|version| !version.is_empty())
                .unwrap_or_else(|| release::name(&os_release)),
        }
        .file()
    );
    eprintln!("This machine's report is hw/{}.md.", machine.slug());
    doctor::exit(&checks)
}

/// The name of the disk the system runs from, which is the one the esp is a partition of.
fn own_disk() -> Option<String> {
    let disk = drives::own_disk()?;
    Some(disk.file_name()?.to_str()?.to_string())
}

/// What a report is written from.
struct Report<'a> {
    /// What the kernel says this machine is.
    machine: &'a Machine,
    /// What Orbit has decided about it, when Orbit answered.
    host: Option<&'a Host>,
    /// What systemd says the boot took, when it has finished starting.
    startup: Option<Startup>,
    /// What `rift doctor` found.
    checks: &'a [Check],
    /// Whether this is a Ghost boot, which is the reason for several of the rows above.
    ghost: bool,
    /// The day the report was written.
    date: String,
    /// The version of the system it was written on.
    version: String,
}

impl Report<'_> {
    /// The whole file, in the order `hw/TEMPLATE.md` has it.
    fn file(&self) -> String {
        let machine = self.machine;
        let mut out = format!("# {}\n\n", machine.title());
        out.push_str("| | |\n|---|---|\n");
        for (label, value) in self.facts() {
            let _ = writeln!(out, "| {label} | {value} |");
        }
        let _ = write!(
            out,
            "\n## Verdict\n\n{}\n\n## What worked\n\nEvery check of `rift doctor` on this \
             machine:\n\n```\n{}```\n\n## What didn't\n\n{}\n\n## Notes for Orbit\n\n{}\n\n\
             ## PCI\n\n```\n{}```\n\n## USB\n\n```\n{}```\n",
            verdict(machine, self.startup),
            doctor::report(self.checks, self.ghost),
            didnt(machine, self.checks),
            notes(machine, self.host),
            listing(&machine.pci.iter().map(Pci::line).collect::<Vec<_>>()),
            listing(&machine.usb.iter().map(hw::Usb::line).collect::<Vec<_>>()),
        );
        out
    }

    /// The table at the top: one row per fact, in the order the template has them.
    fn facts(&self) -> Vec<(&'static str, String)> {
        let machine = self.machine;
        vec![
            ("Date", self.date.clone()),
            ("Rift version", self.version.clone()),
            ("Kernel", or_unknown(&machine.kernel)),
            ("Machine", machine.row()),
            (
                "Drive",
                machine
                    .drive
                    .as_ref()
                    .map(hw::Drive::row)
                    .filter(|row| !row.is_empty())
                    .unwrap_or_else(|| "unknown".to_string()),
            ),
            ("Firmware", machine.firmware_row()),
            ("CPU", machine.cpu_row()),
            ("RAM", librift::size(machine.memory)),
            ("GPU", machine.devices(Pci::display)),
            ("Wi-Fi", machine.devices(Pci::wireless)),
            ("Ethernet", machine.devices(Pci::ethernet)),
            ("Display(s)", screens(machine, self.host)),
        ]
    }
}

/// Boots, and how long it took. A report is written on a machine that is running, so it booted;
/// the file of a machine that does not boot is written by hand somewhere else.
fn verdict(machine: &Machine, startup: Option<Startup>) -> String {
    match startup {
        Some(startup) => format!(
            "Boots: yes. Power on to the login took {}: {}.",
            startup.took(),
            startup.sentence()
        ),
        None => format!(
            "Boots: yes. Up {} s when this report was written.",
            machine.uptime
        ),
    }
}

/// What a machine's own checks found: the PCI devices nothing drives, and the checks that did not
/// pass. Everything else a person adds to the file by hand.
fn didnt(machine: &Machine, checks: &[Check]) -> String {
    let mut lines = Vec::new();
    let unbound = machine.unbound();
    if !unbound.is_empty() {
        let said: Vec<String> = unbound.iter().map(|device| device.said()).collect();
        let (count, verb) = if unbound.len() == 1 {
            ("One PCI device".to_string(), "has")
        } else {
            (format!("{} PCI devices", unbound.len()), "have")
        };
        lines.push(format!("{count} {verb} no driver: {}.", said.join(", ")));
    }
    for check in checks {
        match check.verdict {
            Verdict::Passed => {}
            Verdict::Warning => lines.push(format!("{} warned: {}.", check.name, check.detail)),
            Verdict::Failed => lines.push(format!("{} failed: {}.", check.name, check.detail)),
        }
    }
    if lines.is_empty() {
        return "Every PCI device has a driver, and no check warned or failed.".to_string();
    }
    if unbound.is_empty() {
        lines.insert(0, "Every PCI device has a driver.".to_string());
    }
    lines.join("\n")
}

/// What Orbit decided for this machine, which is the part of the host profile a person reading a
/// report wants, and the quirks it had to work around.
fn notes(machine: &Machine, host: Option<&Host>) -> String {
    let Some(host) = host else {
        return "Orbit did not answer, so nothing here comes from the host profile.".to_string();
    };
    let mut said = format!(
        "Orbit calls this machine class {}, graphics {} and AI tier {}",
        host.class, host.gpu_path, host.ai_tier
    );
    let scales: Vec<String> = host
        .outputs
        .iter()
        .map(|output| format!("{} at scale {}", output.connector, output.scale))
        .collect();
    if scales.is_empty() {
        said.push('.');
    } else {
        let _ = write!(said, ", and draws {}.", scales.join(", "));
    }
    let blind: Vec<&str> = host
        .outputs
        .iter()
        .filter(|output| output.width == 0)
        .map(|output| output.connector.as_str())
        .collect();
    if !blind.is_empty() {
        let _ = write!(
            said,
            " {} reports no EDID, so its size is the default one.",
            blind.join(" and ")
        );
    }
    if machine.screens.len() > host.outputs.len() {
        let _ = write!(
            said,
            " The kernel sees {} connected outputs and Orbit {}.",
            machine.screens.len(),
            host.outputs.len()
        );
    }
    said
}

/// The screens, as Orbit has them where it answered: the mode, the size of the panel and the size
/// it is drawn at. Without Orbit it is the connectors the kernel knows and the mode each asks for.
fn screens(machine: &Machine, host: Option<&Host>) -> String {
    match host.filter(|host| !host.outputs.is_empty()) {
        Some(host) => host
            .outputs
            .iter()
            .map(describe)
            .collect::<Vec<_>>()
            .join("; "),
        None => machine.screen_row(),
    }
}

fn describe(output: &Output) -> String {
    let Output {
        connector,
        width,
        height,
        width_cm,
        height_cm,
        scale,
    } = output;
    if *width == 0 {
        return format!("{connector}, no EDID, scale {scale}");
    }
    let mut said = format!("{connector}, {width}x{height}");
    if *width_cm > 0 && *height_cm > 0 {
        let _ = write!(said, ", {width_cm}x{height_cm} cm");
        if let Some(dpi) = output.dpi() {
            let _ = write!(said, ", {dpi:.0} dpi");
        }
    }
    let _ = write!(said, ", scale {scale}");
    said
}

/// A fenced listing, or one line saying there is nothing in it.
fn listing(lines: &[String]) -> String {
    if lines.is_empty() {
        return "none\n".to_string();
    }
    let mut out = String::new();
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
    out
}

fn or_unknown(field: &str) -> String {
    if field.is_empty() {
        "unknown".to_string()
    } else {
        field.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use librift::hw::{Drive, SecureBoot, Usb};

    /// The virtual machine the boot test reports on, as the kernel describes it there.
    fn vm() -> Machine {
        let pci = |address: &str, class, id: &str, driver: &str| Pci {
            address: address.to_string(),
            class,
            id: id.to_string(),
            driver: driver.to_string(),
        };
        Machine {
            vendor: "QEMU".into(),
            model: "Standard PC (Q35 + ICH9, 2009)".into(),
            version: "pc-q35-10.0".into(),
            firmware: "EFI Development Kit II / OVMF 0.0.0 02/06/2015".into(),
            secure_boot: SecureBoot::Off,
            kernel: "6.12.48".into(),
            cpu: "QEMU Virtual CPU version 2.5+".into(),
            threads: 2,
            memory: 4_012_348 * 1024,
            drive: Some(Drive {
                bus: "nvme".into(),
                model: "QEMU NVMe Ctrl".into(),
                size: 10 << 30,
                removable: false,
            }),
            screens: vec![hw::Screen {
                connector: "Virtual-1".into(),
                mode: "1280x800".into(),
            }],
            pci: vec![
                pci("00:00.0", 0x06_00_00, "8086:29c0", ""),
                pci("00:01.0", 0x03_00_00, "1af4:1050", "virtio-pci"),
                pci("00:02.0", 0x02_00_00, "1af4:1041", "virtio-pci"),
                pci("00:03.0", 0x01_08_02, "1b36:0010", "nvme"),
            ],
            usb: vec![Usb {
                port: "usb1".into(),
                id: "1d6b:0002".into(),
                name: "Linux Foundation 2.0 root hub".into(),
            }],
            uptime: 41,
            ..Machine::default()
        }
    }

    fn host() -> Host {
        Host {
            fingerprint: "5297c0f65d6a0f1c".into(),
            class: "borrowed".into(),
            outputs: vec![Output {
                connector: "Virtual-1".into(),
                width: 1280,
                height: 800,
                width_cm: 32,
                height_cm: 20,
                scale: 1,
            }],
            gpu_path: "none".into(),
            ai_tier: "small".into(),
        }
    }

    fn checks() -> Vec<Check> {
        vec![
            Check {
                name: "Orbit",
                verdict: Verdict::Passed,
                detail: "On the bus, host 5297c0f65d6a, class borrowed, AI tier small".into(),
            },
            Check {
                name: "Quasar",
                verdict: Verdict::Passed,
                detail: "Ready, qwen3-0.6b-q8_0 for tier small".into(),
            },
        ]
    }

    fn report(machine: &Machine, host: Option<&Host>, checks: &[Check]) -> String {
        Report {
            machine,
            host,
            startup: Startup::from_timestamps(0, 0, 3_118_002, 14_512_337),
            checks,
            ghost: false,
            date: "2026-10-06".into(),
            version: "0.1.0".into(),
        }
        .file()
    }

    /// The whole report for that machine, which is what the boot test reads row by row.
    const VM: &str = r"# QEMU Standard PC (Q35 + ICH9, 2009)

| | |
|---|---|
| Date | 2026-10-06 |
| Rift version | 0.1.0 |
| Kernel | 6.12.48 |
| Machine | QEMU Standard PC (Q35 + ICH9, 2009), pc-q35-10.0, desktop |
| Drive | nvme, QEMU NVMe Ctrl, 10.0 GiB |
| Firmware | EFI Development Kit II / OVMF 0.0.0 02/06/2015, secure boot off |
| CPU | QEMU Virtual CPU version 2.5+, 2 threads |
| RAM | 3.8 GiB |
| GPU | 1af4:1050, virtio-pci |
| Wi-Fi | none |
| Ethernet | 1af4:1041, virtio-pci |
| Display(s) | Virtual-1, 1280x800, 32x20 cm, 102 dpi, scale 1 |

## Verdict

Boots: yes. Power on to the login took 14.5 s: 3.1 s of kernel and 11.4 s of userspace.

## What worked

Every check of `rift doctor` on this machine:

```
Orbit   Passed   On the bus, host 5297c0f65d6a, class borrowed, AI tier small
Quasar  Passed   Ready, qwen3-0.6b-q8_0 for tier small

2 checks, 2 passed, 0 warnings, 0 failed.
```

## What didn't

One PCI device has no driver: bridge 8086:29c0.

## Notes for Orbit

Orbit calls this machine class borrowed, graphics none and AI tier small, and draws Virtual-1 at scale 1.

## PCI

```
00:00.0 060000 8086:29c0 no driver
00:01.0 030000 1af4:1050 virtio-pci
00:02.0 020000 1af4:1041 virtio-pci
00:03.0 010802 1b36:0010 nvme
```

## USB

```
usb1 1d6b:0002 Linux Foundation 2.0 root hub
```
";

    #[test]
    fn a_report_is_the_file_the_template_shapes() {
        assert_eq!(report(&vm(), Some(&host()), &checks()), VM);
    }

    #[test]
    fn without_orbit_the_screens_are_the_connectors_the_kernel_knows() {
        let said = report(&vm(), None, &checks());
        assert!(
            said.contains("| Display(s) | Virtual-1, 1280x800 |"),
            "{said}"
        );
        assert!(
            said.contains("Orbit did not answer, so nothing here comes from the host profile."),
            "{said}"
        );
    }

    #[test]
    fn a_machine_that_answered_nothing_still_has_a_file() {
        let said = Report {
            machine: &Machine::default(),
            host: None,
            startup: None,
            checks: &[],
            ghost: false,
            date: "2026-10-06".into(),
            version: "0.1.0".into(),
        }
        .file();
        assert!(said.starts_with("# unknown machine\n"), "{said}");
        for row in [
            "| Kernel | unknown |",
            "| Drive | unknown |",
            "| CPU | unknown |",
            "| RAM | 0 MiB |",
            "| GPU | none |",
            "| Display(s) | none |",
        ] {
            assert!(said.contains(row), "{row} is not in\n{said}");
        }
        assert!(
            said.contains("Boots: yes. Up 0 s when this report was written."),
            "{said}"
        );
        assert!(said.contains("## PCI\n\n```\nnone\n```"), "{said}");
        assert!(said.contains("## USB\n\n```\nnone\n```"), "{said}");
    }

    #[test]
    fn what_didnt_is_the_devices_with_no_driver_and_the_checks_that_did_not_pass() {
        let mut machine = vm();
        // the graphics card with nothing driving it, which is the report worth having
        machine.pci[1].driver.clear();
        let mut checks = checks();
        checks[1].verdict = Verdict::Warning;
        checks[1].detail = "No chat model is on the drive".into();
        let said = report(&machine, Some(&host()), &checks);
        assert!(
            said.contains(
                "2 PCI devices have no driver: bridge 8086:29c0, display controller \
                 1af4:1050.\nQuasar warned: No chat model is on the drive."
            ),
            "{said}"
        );
        assert!(said.contains("| GPU | 1af4:1050, no driver |"), "{said}");
    }

    #[test]
    fn a_ghost_boot_says_the_mode_over_the_checks() {
        let said = Report {
            machine: &vm(),
            host: Some(&host()),
            startup: None,
            checks: &[Check {
                name: "Persist",
                verdict: Verdict::Passed,
                detail: "Ghost mode, persist is locked and nothing of it is mounted".into(),
            }],
            ghost: true,
            date: "2026-10-06".into(),
            version: "0.1.0".into(),
        }
        .file();
        assert!(
            said.contains("Ghost mode. The drive stays locked"),
            "{said}"
        );
    }
}
