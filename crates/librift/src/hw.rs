//! What this machine is, out of sysfs and procfs: the facts a hardware report needs.
//!
//! `rift doctor --report` prints these as the markdown of one file under `hw/`, and the hardware
//! page of the website is generated from those files. The kernel is the only source here: nothing
//! runs dmidecode, lspci or lsusb, so a report can be written from a Ghost boot or a rescue shell
//! and the image is free to carry those programs for a person rather than for this.
//!
//! Where sysfs has no name there is none. A PCI device is its address, its class, its two ids and
//! the driver that bound to it, which is what a hardware database has to know anyway: a device
//! with no driver is the thing a report exists to say. USB devices do have names, because the
//! device itself tells the kernel what it is called.

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(feature = "bus")]
use crate::bus;

/// Where the facts are read from: `/sys` and `/proc` on a running machine, and a directory of
/// files in the tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    /// `/sys`.
    pub sys: PathBuf,
    /// `/proc`.
    pub proc: PathBuf,
}

impl Default for Roots {
    fn default() -> Self {
        Self {
            sys: PathBuf::from("/sys"),
            proc: PathBuf::from("/proc"),
        }
    }
}

/// Whether the firmware checked a signature on what it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SecureBoot {
    /// The firmware checked, and this system passed.
    On,
    /// The firmware did not check.
    Off,
    /// There is no EFI variable to read, which is a machine that booted some other way, or a
    /// kernel with efivarfs not mounted.
    #[default]
    Unknown,
}

impl SecureBoot {
    /// The word a report prints.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Unknown => "unknown",
        }
    }

    /// From the bytes of the `SecureBoot` EFI variable: four bytes of attributes and then one
    /// byte that is 1 when the firmware checked.
    #[must_use]
    pub fn from_variable(bytes: &[u8]) -> Self {
        match bytes.get(4) {
            Some(1) => Self::On,
            Some(_) => Self::Off,
            None => Self::Unknown,
        }
    }
}

/// One PCI device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pci {
    /// Where it sits, as lspci writes it: `00:02.0`.
    pub address: String,
    /// Its class, subclass and programming interface, the three bytes sysfs puts in `class`.
    pub class: u32,
    /// Vendor and device id, lower-case hex: `8086:a7a1`.
    pub id: String,
    /// The driver that bound to it, empty when none did.
    pub driver: String,
}

impl Pci {
    /// The line a listing prints: where it is, its class, its ids and its driver.
    #[must_use]
    pub fn line(&self) -> String {
        let driver = if self.driver.is_empty() {
            "no driver".to_string()
        } else {
            self.driver.clone()
        };
        format!("{} {:06x} {} {driver}", self.address, self.class, self.id)
    }

    /// Its class in words, and its ids: `display controller 1234:1111`.
    #[must_use]
    pub fn said(&self) -> String {
        format!("{} {}", pci_class(self.class), self.id)
    }

    /// The ids and the driver, for a row of the table of facts.
    #[must_use]
    pub fn row(&self) -> String {
        if self.driver.is_empty() {
            format!("{}, no driver", self.id)
        } else {
            format!("{}, {}", self.id, self.driver)
        }
    }

    /// A graphics card, class 03.
    #[must_use]
    pub const fn display(&self) -> bool {
        self.class >> 16 == 0x03
    }

    /// A wireless card: either the other kind of network controller, which is what every Wi-Fi
    /// card of the last fifteen years reports, or the wireless class itself.
    #[must_use]
    pub const fn wireless(&self) -> bool {
        self.class >> 8 == 0x0280 || self.class >> 16 == 0x0d
    }

    /// A wired network card, class 02 subclass 00.
    #[must_use]
    pub const fn ethernet(&self) -> bool {
        self.class >> 8 == 0x0200
    }
}

/// What a PCI base class is, in the words of the specification. The class is the first of the
/// three bytes sysfs puts in `class`.
#[must_use]
pub const fn pci_class(class: u32) -> &'static str {
    match class >> 16 {
        0x00 => "unclassified device",
        0x01 => "storage controller",
        0x02 => "network controller",
        0x03 => "display controller",
        0x04 => "multimedia device",
        0x05 => "memory controller",
        0x06 => "bridge",
        0x07 => "communication controller",
        0x08 => "system peripheral",
        0x09 => "input device",
        0x0a => "docking station",
        0x0b => "processor",
        0x0c => "serial bus controller",
        0x0d => "wireless controller",
        0x0e => "intelligent controller",
        0x0f => "satellite controller",
        0x10 => "encryption controller",
        0x11 => "signal processing controller",
        0x12 => "processing accelerator",
        0x13 => "non-essential instrumentation",
        _ => "device",
    }
}

/// One USB device, as the device itself says it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usb {
    /// Where it is plugged in, the kernel's name for it: `1-1`.
    pub port: String,
    /// Vendor and product id, lower-case hex: `1d6b:0002`.
    pub id: String,
    /// What it calls itself, maker and product, empty when it says neither.
    pub name: String,
}

impl Usb {
    /// The line a listing prints.
    #[must_use]
    pub fn line(&self) -> String {
        if self.name.is_empty() {
            format!("{} {}", self.port, self.id)
        } else {
            format!("{} {} {}", self.port, self.id, self.name)
        }
    }
}

/// The disk the system runs from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Drive {
    /// How it is attached: `usb`, `nvme`, `sata`, `mmc` or `virtio`, empty when the device tree
    /// does not say.
    pub bus: String,
    /// What the device calls itself, its maker and its model.
    pub model: String,
    /// How big it is, in bytes.
    pub size: u64,
    /// Whether the kernel calls it removable, which a stick is and an internal disk is not.
    pub removable: bool,
}

impl Drive {
    /// The row of the table of facts: how it is attached, what it is, how big, and removable when
    /// the kernel says so.
    #[must_use]
    pub fn row(&self) -> String {
        let mut said = Vec::new();
        if !self.bus.is_empty() {
            said.push(self.bus.clone());
        }
        if !self.model.is_empty() {
            said.push(self.model.clone());
        }
        if self.size > 0 {
            said.push(crate::size(self.size));
        }
        if self.removable {
            said.push("removable".to_string());
        }
        said.join(", ")
    }

    /// Reads `<block>/<disk>`: how big it is, what it calls itself, whether it is removable, and
    /// how it is attached, which is where in the device tree the disk sits.
    #[must_use]
    pub fn read(block: &Path, disk: &str) -> Self {
        let dir = block.join(disk);
        let sectors: u64 = trimmed(&dir.join("size")).parse().unwrap_or(0);
        let device = dir.join("device");
        let model = words(
            &[
                trimmed(&device.join("vendor")),
                trimmed(&device.join("model")),
            ]
            .join(" "),
        );
        let under = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        Self {
            bus: bus_of(&under),
            model,
            // the kernel counts a block device in 512 byte sectors whatever its blocks are
            size: sectors.saturating_mul(512),
            removable: trimmed(&dir.join("removable")) == "1",
        }
    }
}

/// How a disk is attached, from the path its sysfs directory points at. The first name in the
/// path that says a bus wins, so a stick in a card reader is usb and not mmc.
#[must_use]
pub fn bus_of(path: &Path) -> String {
    for part in path.iter().filter_map(|part| part.to_str()) {
        let bus = match part {
            _ if part.starts_with("usb") => "usb",
            _ if part.starts_with("nvme") => "nvme",
            _ if part.starts_with("ata") => "sata",
            _ if part.starts_with("mmc") => "mmc",
            _ if part.starts_with("virtio") => "virtio",
            _ => continue,
        };
        return bus.to_string();
    }
    String::new()
}

/// A connected output, as the kernel has it. The size it is drawn at and its size in centimetres
/// are Orbit's, which reads the EDID; this is what is there with no session running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// The connector: `eDP-1`.
    pub connector: String,
    /// Its preferred mode, `1920x1200`, empty when it reports none.
    pub mode: String,
}

/// Everything a report says about the machine it is written on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Machine {
    /// DMI's `sys_vendor`.
    pub vendor: String,
    /// DMI's `product_name`.
    pub model: String,
    /// DMI's `product_version`, which is where some makers put the name a person knows.
    pub version: String,
    /// The board, maker and name.
    pub board: String,
    /// The firmware, maker, version and date.
    pub firmware: String,
    /// Whether the firmware checked a signature on what it started.
    pub secure_boot: SecureBoot,
    /// Whether this machine has a battery, which is how Orbit tells a laptop from a desktop.
    pub battery: bool,
    /// The kernel's version.
    pub kernel: String,
    /// The processor, as it names itself.
    pub cpu: String,
    /// How many processors the kernel sees.
    pub threads: usize,
    /// Memory in bytes, as the kernel counts it once the firmware has had its share.
    pub memory: u64,
    /// The disk the system runs from, when it is known.
    pub drive: Option<Drive>,
    /// Every connected output.
    pub screens: Vec<Screen>,
    /// Every PCI device, in the order of their addresses.
    pub pci: Vec<Pci>,
    /// Every USB device, root hubs included, in the order of their ports.
    pub usb: Vec<Usb>,
    /// How long the machine had been up when this was read, in seconds.
    pub uptime: u64,
}

impl Machine {
    /// Reads the running machine. `disk` is the disk the system runs from, the file name of what
    /// [`crate::drives::own_disk`] answers, and `None` leaves the drive row out.
    #[must_use]
    pub fn read(roots: &Roots, disk: Option<&str>) -> Self {
        let dmi = roots.sys.join("class/dmi/id");
        let field = |name: &str| trimmed(&dmi.join(name));
        let cpuinfo = fs::read_to_string(roots.proc.join("cpuinfo")).unwrap_or_default();
        Self {
            vendor: field("sys_vendor"),
            model: field("product_name"),
            version: field("product_version"),
            board: words(&[field("board_vendor"), field("board_name")].join(" ")),
            firmware: words(
                &[
                    field("bios_vendor"),
                    field("bios_version"),
                    field("bios_date"),
                ]
                .join(" "),
            ),
            secure_boot: secure_boot(&roots.sys),
            battery: battery(&roots.sys.join("class/power_supply")),
            kernel: trimmed(&roots.proc.join("sys/kernel/osrelease")),
            cpu: cpu_name(&cpuinfo),
            threads: cpuinfo
                .lines()
                .filter(|line| line.starts_with("processor"))
                .count(),
            memory: memory_bytes(
                &fs::read_to_string(roots.proc.join("meminfo")).unwrap_or_default(),
            ),
            drive: disk.map(|disk| Drive::read(&roots.sys.join("class/block"), disk)),
            screens: screens(&roots.sys.join("class/drm")),
            pci: pci(&roots.sys.join("bus/pci/devices")),
            usb: usb(&roots.sys.join("bus/usb/devices")),
            uptime: uptime(&fs::read_to_string(roots.proc.join("uptime")).unwrap_or_default()),
        }
    }

    /// What the machine is called: its maker and its model, or `unknown machine` when DMI says
    /// neither, which is a board that provides no DMI at all.
    #[must_use]
    pub fn title(&self) -> String {
        let said = words(&format!("{} {}", self.vendor, self.model));
        if said.is_empty() {
            "unknown machine".to_string()
        } else {
            said
        }
    }

    /// The file name of this machine's report, `vendor-model.md` without the suffix. Anything
    /// that is not a letter or a digit becomes one dash.
    #[must_use]
    pub fn slug(&self) -> String {
        let said = format!("{}-{}", or_unknown(&self.vendor), or_unknown(&self.model));
        let mut slug = String::with_capacity(said.len());
        for c in said.chars() {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
            } else if !slug.ends_with('-') {
                slug.push('-');
            }
        }
        slug.trim_matches('-').to_string()
    }

    /// The row of the table of facts that says which machine this is: what DMI calls it, the name
    /// a person knows it by where the maker put one there, the board where it adds anything, and
    /// whether it is a laptop.
    #[must_use]
    pub fn row(&self) -> String {
        let mut said = self.title();
        for more in [self.version.as_str(), self.board.as_str()] {
            if !more.is_empty()
                && !said
                    .to_ascii_lowercase()
                    .contains(&more.to_ascii_lowercase())
            {
                said.push_str(", ");
                said.push_str(more);
            }
        }
        said.push_str(if self.battery {
            ", laptop"
        } else {
            ", desktop"
        });
        said
    }

    /// The processor and how many of it the kernel sees.
    #[must_use]
    pub fn cpu_row(&self) -> String {
        let name = if self.cpu.is_empty() {
            "unknown".to_string()
        } else {
            self.cpu.clone()
        };
        match self.threads {
            0 => name,
            1 => format!("{name}, 1 thread"),
            threads => format!("{name}, {threads} threads"),
        }
    }

    /// The firmware and whether it checked a signature on what it started.
    #[must_use]
    pub fn firmware_row(&self) -> String {
        let secure = format!("secure boot {}", self.secure_boot.word());
        if self.firmware.is_empty() {
            secure
        } else {
            format!("{}, {secure}", self.firmware)
        }
    }

    /// The PCI devices of one kind, as a row of the table of facts, or `none`.
    #[must_use]
    pub fn devices(&self, kind: fn(&Pci) -> bool) -> String {
        let said: Vec<String> = self.pci.iter().filter(|d| kind(d)).map(Pci::row).collect();
        if said.is_empty() {
            "none".to_string()
        } else {
            said.join("; ")
        }
    }

    /// The connectors with something plugged into them, with the mode each one asks for. Orbit
    /// says this better, from the EDID; this is the answer when Orbit is not there.
    #[must_use]
    pub fn screen_row(&self) -> String {
        if self.screens.is_empty() {
            return "none".to_string();
        }
        self.screens
            .iter()
            .map(|screen| {
                if screen.mode.is_empty() {
                    screen.connector.clone()
                } else {
                    format!("{}, {}", screen.connector, screen.mode)
                }
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// The PCI devices no driver bound to, which is the list a report exists to carry.
    #[must_use]
    pub fn unbound(&self) -> Vec<&Pci> {
        self.pci.iter().filter(|d| d.driver.is_empty()).collect()
    }
}

/// `unknown` for a DMI field the machine does not provide.
fn or_unknown(field: &str) -> &str {
    if field.is_empty() { "unknown" } else { field }
}

/// A file under /sys or /proc without its trailing newline, empty when it cannot be read.
fn trimmed(path: &Path) -> String {
    fs::read_to_string(path)
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

/// Words with one space between them. DMI and SCSI both pad their strings.
fn words(said: &str) -> String {
    said.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `MemTotal` from a meminfo file, in bytes.
fn memory_bytes(meminfo: &str) -> u64 {
    meminfo
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix("MemTotal:")?;
            let kib: u64 = rest.split_whitespace().next()?.parse().ok()?;
            Some(kib.saturating_mul(1024))
        })
        .unwrap_or(0)
}

/// The first `model name` of a cpuinfo file.
fn cpu_name(cpuinfo: &str) -> String {
    cpuinfo
        .lines()
        .find_map(|line| {
            let value = line.strip_prefix("model name")?.trim_start();
            Some(words(value.strip_prefix(':')?))
        })
        .unwrap_or_default()
}

/// The seconds in the first field of an uptime file.
fn uptime(text: &str) -> u64 {
    text.split_whitespace()
        .next()
        .and_then(|field| field.split('.').next())
        .and_then(|seconds| seconds.parse().ok())
        .unwrap_or(0)
}

/// Whether one of the power supplies under `dir` is a battery, the way Orbit tells a laptop from
/// a desktop.
fn battery(dir: &Path) -> bool {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| trimmed(&entry.path().join("type")) == "Battery")
}

/// The `SecureBoot` EFI variable, which is one byte in a file named after the global EFI variable
/// namespace. The name is looked up rather than spelled out, so a firmware that writes the uuid
/// in another case still answers.
fn secure_boot(sys: &Path) -> SecureBoot {
    let efivars = sys.join("firmware/efi/efivars");
    let found = fs::read_dir(&efivars)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name())
        .find(|name| {
            name.to_str()
                .is_some_and(|name| name.starts_with("SecureBoot-"))
        });
    match found {
        Some(name) => fs::read(efivars.join(name))
            .map(|bytes| SecureBoot::from_variable(&bytes))
            .unwrap_or_default(),
        None => SecureBoot::Unknown,
    }
}

/// The connectors under `/sys/class/drm` with something plugged into them. The directory of one
/// is `card0-eDP-1`, and the connector is what follows the card.
fn screens(drm: &Path) -> Vec<Screen> {
    let mut screens: Vec<Screen> = fs::read_dir(drm)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            if trimmed(&dir.join("status")) != "connected" {
                return None;
            }
            let name = entry.file_name().to_str()?.to_string();
            let connector = name.split_once('-').map(|(_, rest)| rest)?.to_string();
            Some(Screen {
                connector,
                mode: trimmed(&dir.join("modes"))
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect();
    screens.sort_by(|a, b| a.connector.cmp(&b.connector));
    screens
}

/// Every PCI device under `/sys/bus/pci/devices`, in the order of their addresses. The domain is
/// left off an address in the one domain every ordinary machine has, as lspci leaves it off.
fn pci(devices: &Path) -> Vec<Pci> {
    let mut found: Vec<Pci> = fs::read_dir(devices)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            let name = entry.file_name().to_str()?.to_string();
            let vendor = hex_id(&trimmed(&dir.join("vendor")))?;
            let device = hex_id(&trimmed(&dir.join("device")))?;
            Some(Pci {
                address: name.strip_prefix("0000:").unwrap_or(&name).to_string(),
                class: u32::from_str_radix(&hex_id(&trimmed(&dir.join("class")))?, 16).ok()?,
                id: format!("{vendor}:{device}"),
                driver: fs::read_link(dir.join("driver"))
                    .ok()
                    .and_then(|target| Some(target.file_name()?.to_str()?.to_string()))
                    .unwrap_or_default(),
            })
        })
        .collect();
    found.sort_by(|a, b| a.address.cmp(&b.address));
    found
}

/// Every USB device under `/sys/bus/usb/devices`. The directories there are devices and the
/// interfaces of a device both; a device is the one with ids on it.
fn usb(devices: &Path) -> Vec<Usb> {
    let mut found: Vec<Usb> = fs::read_dir(devices)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            let vendor = hex_id(&trimmed(&dir.join("idVendor")))?;
            let product = hex_id(&trimmed(&dir.join("idProduct")))?;
            Some(Usb {
                port: entry.file_name().to_str()?.to_string(),
                id: format!("{vendor}:{product}"),
                name: words(
                    &[
                        trimmed(&dir.join("manufacturer")),
                        trimmed(&dir.join("product")),
                    ]
                    .join(" "),
                ),
            })
        })
        .collect();
    found.sort_by(|a, b| a.port.cmp(&b.port));
    found
}

/// Turns sysfs's `0x8086` into `8086`, the way Orbit does. Anything that is not hex is dropped.
fn hex_id(raw: &str) -> Option<String> {
    let digits = raw.strip_prefix("0x").unwrap_or(raw);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(digits.to_ascii_lowercase())
}

/// How long the machine took to start, as systemd counted it: the same four spans
/// `systemd-analyze` prints, in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Startup {
    /// The firmware, from power on to the boot loader.
    pub firmware: u64,
    /// The boot loader, to the kernel.
    pub loader: u64,
    /// The kernel and the initrd, to the first userspace process.
    pub kernel: u64,
    /// Userspace, to the moment systemd called the system started.
    pub userspace: u64,
}

impl Startup {
    /// From systemd's four monotonic timestamps. The firmware and the loader count backwards from
    /// the kernel, which is where the monotonic clock starts, so each one is the moment it began.
    /// `None` until the system has finished starting, which is when the last timestamp is set.
    #[must_use]
    pub fn from_timestamps(
        firmware: u64,
        loader: u64,
        userspace: u64,
        finish: u64,
    ) -> Option<Self> {
        if finish == 0 || userspace == 0 || finish < userspace {
            return None;
        }
        Some(Self {
            firmware: firmware.saturating_sub(loader),
            loader,
            kernel: userspace,
            userspace: finish - userspace,
        })
    }

    /// Power on to the moment the system was up.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.firmware + self.loader + self.kernel + self.userspace
    }

    /// Power on to the moment the system was up, in seconds with one decimal.
    #[must_use]
    pub fn took(&self) -> String {
        seconds(self.total())
    }

    /// The spans in words, the ones that took no time left out. A virtual machine's firmware says
    /// nothing of itself, so there those two are missing.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut said: Vec<String> = [
            (self.firmware, "firmware"),
            (self.loader, "loader"),
            (self.kernel, "kernel"),
            (self.userspace, "userspace"),
        ]
        .into_iter()
        .filter(|(span, _)| *span > 0)
        .map(|(span, what)| format!("{} of {what}", seconds(span)))
        .collect();
        let Some(last) = said.pop() else {
            return "no time at all".to_string();
        };
        if said.is_empty() {
            last
        } else {
            format!("{} and {last}", said.join(", "))
        }
    }
}

/// Microseconds in seconds with one decimal, the way systemd writes a boot.
fn seconds(micros: u64) -> String {
    let tenths = (micros + 50_000) / 100_000;
    format!("{}.{} s", tenths / 10, tenths % 10)
}

/// What systemd says the boot took, read off the system bus.
///
/// # Errors
///
/// A sentence when the system bus or systemd cannot be reached.
#[cfg(feature = "bus")]
pub fn startup() -> Result<Option<Startup>, String> {
    use std::time::Duration;

    const SYSTEMD: &str = "org.freedesktop.systemd1";
    let connection = bus::connect(Duration::from_secs(10))?;
    let manager = bus::object(
        &connection,
        SYSTEMD,
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )
    .map_err(|e| bus::sentence_for("systemd", e))?;
    let stamp = |name: &str| {
        manager
            .get_property::<u64>(name)
            .map_err(|e| bus::sentence_for("systemd", e))
    };
    Ok(Startup::from_timestamps(
        stamp("FirmwareTimestampMonotonic")?,
        stamp("LoaderTimestampMonotonic")?,
        stamp("UserspaceTimestampMonotonic")?,
        stamp("FinishTimestampMonotonic")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CPUINFO: &str = "\
processor\t: 0
vendor_id\t: GenuineIntel
model name\t: QEMU Virtual CPU version 2.5+
cpu MHz\t\t: 2800.000

processor\t: 1
vendor_id\t: GenuineIntel
model name\t: QEMU Virtual CPU version 2.5+
";

    /// A directory of its own for one test, gone again at the end of it.
    fn work(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("librift-hw-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// Enough of a sysfs and a procfs for one machine: the virtual one the boot test reports on.
    fn fake(root: &Path) -> Roots {
        let sys = root.join("sys");
        let proc = root.join("proc");
        let dmi = sys.join("class/dmi/id");
        for (name, value) in [
            ("sys_vendor", "QEMU\n"),
            ("product_name", "Standard PC (Q35 + ICH9, 2009)\n"),
            ("product_version", "pc-q35-10.0\n"),
            ("bios_vendor", "EFI Development Kit II / OVMF\n"),
            ("bios_version", "0.0.0\n"),
            ("bios_date", "02/06/2015\n"),
        ] {
            write(&dmi.join(name), value);
        }
        write(&sys.join("class/power_supply/.keep"), "");
        // the secure boot variable: four bytes of attributes, then the byte that counts
        fs::create_dir_all(sys.join("firmware/efi/efivars")).unwrap();
        fs::write(
            sys.join("firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c"),
            [6, 0, 0, 0, 0],
        )
        .unwrap();
        write(&sys.join("class/drm/card0-Virtual-1/status"), "connected\n");
        write(
            &sys.join("class/drm/card0-Virtual-1/modes"),
            "1280x800\n1024x768\n",
        );
        write(
            &sys.join("class/drm/card0-Virtual-2/status"),
            "disconnected\n",
        );
        write(&sys.join("class/drm/version"), "drm 1.1.0 20060810\n");
        let pci = sys.join("bus/pci/devices");
        for (address, vendor, device, class) in [
            ("0000:00:00.0", "0x8086", "0x29C0", "0x060000"),
            ("0000:00:01.0", "0x1af4", "0x1050", "0x030000"),
            ("0000:00:02.0", "0x1af4", "0x1041", "0x020000"),
            ("0000:00:03.0", "0x1b36", "0x0010", "0x010802"),
        ] {
            write(&pci.join(address).join("vendor"), vendor);
            write(&pci.join(address).join("device"), device);
            write(&pci.join(address).join("class"), class);
        }
        let usb = sys.join("bus/usb/devices");
        write(&usb.join("usb1/idVendor"), "1d6b\n");
        write(&usb.join("usb1/idProduct"), "0002\n");
        write(&usb.join("usb1/manufacturer"), "Linux  Foundation\n");
        write(&usb.join("usb1/product"), "2.0 root hub\n");
        // an interface of that device, which is not a device and has no ids
        write(&usb.join("1-0:1.0/bInterfaceClass"), "09\n");
        let block = sys.join("class/block/nvme0n1");
        write(&block.join("size"), "20971520\n");
        write(&block.join("removable"), "0\n");
        write(
            &block.join("device/model"),
            "QEMU NVMe Ctrl                          \n",
        );
        write(&proc.join("cpuinfo"), CPUINFO);
        write(
            &proc.join("meminfo"),
            "MemTotal:        4012348 kB\nMemFree: 812044 kB\n",
        );
        write(&proc.join("sys/kernel/osrelease"), "6.12.48\n");
        write(&proc.join("uptime"), "41.90 158.32\n");
        Roots { sys, proc }
    }

    #[test]
    fn a_machine_reads_out_of_sysfs() {
        let root = work("machine");
        let roots = fake(&root);
        let machine = Machine::read(&roots, Some("nvme0n1"));
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(machine.title(), "QEMU Standard PC (Q35 + ICH9, 2009)");
        assert_eq!(machine.slug(), "qemu-standard-pc-q35-ich9-2009");
        assert_eq!(
            machine.row(),
            "QEMU Standard PC (Q35 + ICH9, 2009), pc-q35-10.0, desktop"
        );
        assert_eq!(machine.kernel, "6.12.48");
        assert_eq!(
            machine.cpu_row(),
            "QEMU Virtual CPU version 2.5+, 2 threads"
        );
        assert_eq!(machine.memory, 4_012_348 * 1024);
        assert_eq!(
            machine.firmware_row(),
            "EFI Development Kit II / OVMF 0.0.0 02/06/2015, secure boot off"
        );
        assert_eq!(machine.uptime, 41);
        // one connected output of the two connectors, and the mode it asks for
        assert_eq!(machine.screen_row(), "Virtual-1, 1280x800");
        // the drive it runs from, with the model's padding taken out. the name of the disk says
        // the bus here; on a running machine the whole device tree path does
        assert_eq!(
            machine.drive.as_ref().unwrap().row(),
            "nvme, QEMU NVMe Ctrl, 10.0 GiB"
        );
        // the four pci devices in the order of their addresses, and no driver bound to any of them
        assert_eq!(machine.pci.len(), 4);
        assert_eq!(machine.pci[0].line(), "00:00.0 060000 8086:29c0 no driver");
        assert_eq!(machine.pci[1].said(), "display controller 1af4:1050");
        assert_eq!(machine.devices(Pci::display), "1af4:1050, no driver");
        assert_eq!(machine.devices(Pci::wireless), "none");
        assert_eq!(machine.devices(Pci::ethernet), "1af4:1041, no driver");
        assert_eq!(machine.unbound().len(), 4);
        // the root hub, by the name it gave the kernel, and not the interface beside it
        assert_eq!(machine.usb.len(), 1);
        assert_eq!(
            machine.usb[0].line(),
            "usb1 1d6b:0002 Linux Foundation 2.0 root hub"
        );
    }

    #[test]
    fn a_machine_with_a_battery_is_a_laptop() {
        let root = work("laptop");
        let roots = fake(&root);
        write(&roots.sys.join("class/power_supply/BAT0/type"), "Battery\n");
        write(&roots.sys.join("class/power_supply/AC/type"), "Mains\n");
        let machine = Machine::read(&roots, None);
        fs::remove_dir_all(&root).unwrap();
        assert!(machine.battery);
        assert!(machine.row().ends_with(", laptop"), "{}", machine.row());
        assert!(machine.drive.is_none());
    }

    #[test]
    fn a_machine_that_says_nothing_about_itself_still_reads() {
        let roots = Roots {
            sys: PathBuf::from("/nonexistent/sys"),
            proc: PathBuf::from("/nonexistent/proc"),
        };
        let machine = Machine::read(&roots, Some("sda"));
        assert_eq!(
            machine,
            Machine {
                drive: Some(Drive::default()),
                ..Machine::default()
            }
        );
        assert_eq!(machine.title(), "unknown machine");
        assert_eq!(machine.slug(), "unknown-unknown");
        assert_eq!(machine.row(), "unknown machine, desktop");
        assert_eq!(machine.cpu_row(), "unknown");
        assert_eq!(machine.firmware_row(), "secure boot unknown");
        assert_eq!(machine.screen_row(), "none");
        assert_eq!(machine.devices(Pci::display), "none");
        assert_eq!(machine.drive.unwrap().row(), "");
    }

    #[test]
    fn the_name_a_person_knows_goes_in_the_row_once() {
        let laptop = Machine {
            vendor: "LENOVO".into(),
            model: "21F8CTO1WW".into(),
            version: "ThinkPad T14s Gen 4".into(),
            board: "LENOVO 21F8CTO1WW".into(),
            battery: true,
            ..Machine::default()
        };
        assert_eq!(
            laptop.row(),
            "LENOVO 21F8CTO1WW, ThinkPad T14s Gen 4, laptop"
        );
        assert_eq!(laptop.slug(), "lenovo-21f8cto1ww");
        let desktop = Machine {
            vendor: "System manufacturer".into(),
            model: "System Product Name".into(),
            board: "ASUSTeK COMPUTER INC. PRIME B650-PLUS".into(),
            ..Machine::default()
        };
        assert_eq!(
            desktop.row(),
            "System manufacturer System Product Name, ASUSTeK COMPUTER INC. PRIME B650-PLUS, \
             desktop"
        );
        assert_eq!(desktop.slug(), "system-manufacturer-system-product-name");
    }

    #[test]
    fn secure_boot_is_the_fifth_byte() {
        assert_eq!(SecureBoot::from_variable(&[6, 0, 0, 0, 1]), SecureBoot::On);
        assert_eq!(SecureBoot::from_variable(&[6, 0, 0, 0, 0]), SecureBoot::Off);
        assert_eq!(
            SecureBoot::from_variable(&[6, 0, 0, 0]),
            SecureBoot::Unknown
        );
        assert_eq!(SecureBoot::Unknown.word(), "unknown");
    }

    #[test]
    fn a_bus_is_the_first_one_in_the_path() {
        let usb = Path::new(
            "/sys/devices/pci0000:00/0000:00:14.0/usb2/2-1/2-1:1.0/host4/target4:0:0/4:0:0:0/block/sda",
        );
        assert_eq!(bus_of(usb), "usb");
        assert_eq!(
            bus_of(Path::new(
                "/sys/devices/pci0000:00/0000:00:1d.0/nvme/nvme0/nvme0n1"
            )),
            "nvme"
        );
        assert_eq!(
            bus_of(Path::new(
                "/sys/devices/pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0/block/sda"
            )),
            "sata"
        );
        assert_eq!(
            bus_of(Path::new(
                "/sys/devices/platform/soc/mmc_host/mmc0/mmc0:0001/block/mmcblk0"
            )),
            "mmc"
        );
        assert_eq!(
            bus_of(Path::new(
                "/sys/devices/pci0000:00/0000:00:05.0/virtio1/block/vda"
            )),
            "virtio"
        );
        assert_eq!(bus_of(Path::new("/sys/class/block/loop0")), "");
    }

    #[test]
    fn a_drive_reads_what_the_kernel_says_about_it() {
        let root = work("drive");
        let block = root.join("block");
        write(&block.join("sdb/size"), "120000000\n");
        write(&block.join("sdb/removable"), "1\n");
        write(&block.join("sdb/device/vendor"), "Samsung \n");
        write(&block.join("sdb/device/model"), "Flash Drive FIT \n");
        let drive = Drive::read(&block, "sdb");
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(drive.model, "Samsung Flash Drive FIT");
        assert_eq!(drive.size, 120_000_000 * 512);
        assert!(drive.removable);
        assert_eq!(drive.row(), "Samsung Flash Drive FIT, 57.2 GiB, removable");
    }

    #[test]
    fn a_class_is_the_first_of_its_three_bytes() {
        assert_eq!(pci_class(0x03_00_00), "display controller");
        assert_eq!(pci_class(0x02_80_00), "network controller");
        assert_eq!(pci_class(0x0d_20_00), "wireless controller");
        assert_eq!(pci_class(0xff_00_00), "device");
        let wifi = |class| Pci {
            address: "00:14.3".into(),
            class,
            id: "8086:51f0".into(),
            driver: String::new(),
        };
        // every wi-fi card of the last fifteen years is the other kind of network controller
        assert!(wifi(0x02_80_00).wireless());
        assert!(wifi(0x0d_21_00).wireless());
        assert!(!wifi(0x02_00_00).wireless());
        assert!(wifi(0x02_00_00).ethernet());
        assert!(!wifi(0x02_80_00).ethernet());
        assert!(wifi(0x03_00_00).display());
        assert_eq!(
            Pci {
                driver: "iwlwifi".into(),
                ..wifi(0x02_80_00)
            }
            .row(),
            "8086:51f0, iwlwifi"
        );
    }

    #[test]
    fn a_boot_is_the_four_spans_systemd_counted() {
        // the numbers of one real boot, in microseconds
        let boot = Startup::from_timestamps(2_236_489, 336_489, 3_118_002, 14_512_337).unwrap();
        assert_eq!(boot.firmware, 1_900_000);
        assert_eq!(boot.loader, 336_489);
        assert_eq!(boot.kernel, 3_118_002);
        assert_eq!(boot.userspace, 11_394_335);
        assert_eq!(boot.total(), 16_748_826);
        assert_eq!(boot.took(), "16.7 s");
        assert_eq!(
            boot.sentence(),
            "1.9 s of firmware, 0.3 s of loader, 3.1 s of kernel and 11.4 s of userspace"
        );
        // a machine still starting has no finish timestamp, and nothing to say
        assert_eq!(
            Startup::from_timestamps(2_236_489, 336_489, 3_118_002, 0),
            None
        );
        assert_eq!(Startup::from_timestamps(0, 0, 0, 0), None);
        // a virtual machine's firmware says nothing of itself, so those spans are left out
        let vm = Startup::from_timestamps(0, 0, 2_000_000, 9_000_000).unwrap();
        assert_eq!(vm.sentence(), "2.0 s of kernel and 7.0 s of userspace");
        assert_eq!(vm.took(), "9.0 s");
        assert_eq!(seconds(0), "0.0 s");
        assert_eq!(seconds(1_950_000), "2.0 s");
    }
}
