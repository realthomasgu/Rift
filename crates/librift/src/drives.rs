//! The disks a person plugs in, and the exchange partition of the drive itself.
//!
//! udisks answers on the system bus for every block device the kernel knows, and mounts the ones
//! polkit does not refuse. Rift's polkit rule refuses every udisks action whose id ends in
//! `-system`, which is the one udisks asks for when the device is internal to the machine, so a
//! host's own disks can be looked at and never mounted. What is left is removable disks and disks
//! on USB, which are the ones a person brought with them: those are what this lists.
//!
//! The drive Rift itself runs from is left out whole. On a real stick it is removable like any
//! other, so its esp, its two slots and its locked persist would all be rows to mount; they are
//! the drive's own, not disks to open.
//!
//! Its exchange partition is the one exception, and it is a shape of its own rather than a row of
//! the list: [`Exchange`]. An ordinary boot mounts it at [`EXCHANGE`] before anyone logs in, so it
//! is a folder like the ones in home. A Ghost boot mounts nothing of the drive, because mounting a
//! vfat writes to it, so there it is a place the sidebar offers and the owner asks for (ADR-0085).
//! Whether the drive has one at all is read off its partition table, which is root's to do, so
//! Vault answers it and mounts it; everything here reads is the mount table.
//!
//! Nothing here mounts anything by itself. A disk that is plugged in appears, and it is mounted
//! when the owner asks for it.

use std::path::{Path, PathBuf};

/// udisks on the system bus.
pub const SERVICE: &str = "org.freedesktop.UDisks2";
/// Where udev names the partitions of the drive the system started from.
const DESIGNATORS: &str = "/dev/disk/by-designator";
/// What the kernel says about a block device.
const SYSFS: &str = "/sys/class/block";
/// Where the system mounts the exchange partition of the drive, when the drive has one.
pub const EXCHANGE: &str = "/exchange";
/// What the sidebar calls that folder.
pub const EXCHANGE_NAME: &str = "Exchange";

/// A disk, or a partition of one, that the sidebar lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// What names it on the bus, and what `--set drive` takes.
    pub id: String,
    /// What the sidebar calls it: the name of its file system, the disk's own model, or how big
    /// it is.
    pub name: String,
    /// The device the kernel gave it, `/dev/sda1`.
    pub device: String,
    /// The kind of file system on it, `exfat` or `ext4`, or `crypto_LUKS` for a locked one.
    pub fs: String,
    /// How big it is, in bytes.
    pub size: u64,
    /// Where it is mounted, when it is.
    pub mount: Option<PathBuf>,
    /// Whether it holds an encrypted volume that has not been unlocked.
    pub locked: bool,
    /// The disk it is a part of, which is what is ejected.
    pub drive: String,
    /// Whether that disk can be ejected or switched off.
    pub eject: bool,
    /// The symbolic icon the sidebar draws it with.
    pub icon: &'static str,
}

impl Volume {
    /// Whether it is mounted now.
    #[must_use]
    pub const fn mounted(&self) -> bool {
        self.mount.is_some()
    }
}

/// The drive's own exchange partition, as the sidebar has it.
///
/// It is not a [`Volume`]: there is nothing to eject, nothing to unlock and no disk to switch off,
/// and it is never one of the disks a person plugged in. What a row needs to know about it is
/// whether the drive has one and whether it is mounted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Exchange {
    /// The drive has none, or nothing has asked Vault yet.
    #[default]
    None,
    /// The drive has one and nothing has mounted it, which is what a Ghost boot leaves. A press
    /// mounts it.
    There,
    /// Mounted, at this folder.
    At(PathBuf),
}

impl Exchange {
    /// Where it is mounted, when it is.
    #[must_use]
    pub fn mount(&self) -> Option<&Path> {
        match self {
            Self::At(path) => Some(path),
            Self::None | Self::There => None,
        }
    }

    /// Whether there is a row for it at all.
    #[must_use]
    pub const fn listed(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// What `--state` prints after `exchange`: where it is, that it is there to be mounted, or
    /// that the drive has none.
    #[must_use]
    pub fn word(&self) -> String {
        match self {
            Self::None => "none".to_string(),
            Self::There => "there".to_string(),
            Self::At(path) => path.display().to_string(),
        }
    }

    /// The same partition after the mount table has been read again: a mount makes it [`Self::At`],
    /// and an unmount leaves it [`Self::There`] rather than gone, since the drive still has it.
    #[must_use]
    pub fn again(&self) -> Self {
        match (exchange(), self) {
            (Some(path), _) => Self::At(path),
            (None, Self::None) => Self::None,
            (None, _) => Self::There,
        }
    }
}

/// The exchange partition of the drive, when the drive has one and something has mounted it.
#[must_use]
pub fn exchange() -> Option<PathBuf> {
    let top = crate::files::top_of(Path::new(EXCHANGE))?;
    (top == Path::new(EXCHANGE)).then_some(top)
}

/// The disk the system started from: the one the esp is a partition of. Everything on it belongs
/// to the drive and is never a row to mount.
#[must_use]
pub fn own_disk() -> Option<PathBuf> {
    let esp = std::fs::canonicalize(Path::new(DESIGNATORS).join("esp")).ok()?;
    let known = std::fs::canonicalize(Path::new(SYSFS).join(esp.file_name()?)).ok()?;
    Some(Path::new("/dev").join(known.parent()?.file_name()?))
}

/// The icon for a disk, by the way it is attached.
#[cfg(any(feature = "bus", test))]
#[must_use]
fn icon_of(bus: &str, locked: bool) -> &'static str {
    if locked {
        "changes-prevent-symbolic"
    } else if bus == "usb" {
        "drive-removable-media-symbolic"
    } else {
        "drive-harddisk-symbolic"
    }
}

#[cfg(feature = "bus")]
mod asking {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use zbus::zvariant::{Array, OwnedObjectPath, OwnedValue};

    use super::{SERVICE, Volume, icon_of, own_disk};
    use crate::bus;

    /// What a person calls udisks in a sentence about something going wrong.
    const NAME: &str = "the disk service";
    /// Where it hangs its objects.
    const ROOT: &str = "/org/freedesktop/UDisks2";

    /// How long a call may take. Mounting a file system reads it first, and an unmount writes
    /// everything that is waiting, so these are not as quick as reading a property.
    const TIMEOUT: Duration = Duration::from_secs(120);

    /// What `GetManagedObjects` answers: an object, its interfaces, and each one's properties.
    type Interfaces = HashMap<String, HashMap<String, OwnedValue>>;
    type Objects = HashMap<OwnedObjectPath, Interfaces>;

    const BLOCK: &str = "org.freedesktop.UDisks2.Block";
    const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
    const DRIVE: &str = "org.freedesktop.UDisks2.Drive";
    const ENCRYPTED: &str = "org.freedesktop.UDisks2.Encrypted";

    /// Everything udisks knows, in one call.
    fn managed(connection: &zbus::blocking::Connection) -> Result<Objects, String> {
        bus::object(
            connection,
            SERVICE,
            ROOT,
            "org.freedesktop.DBus.ObjectManager",
        )
        .and_then(|proxy| proxy.call("GetManagedObjects", &()))
        .map_err(|e| bus::sentence_for(NAME, e))
    }

    fn text(properties: &Interfaces, interface: &str, key: &str) -> String {
        properties
            .get(interface)
            .and_then(|found| found.get(key))
            .and_then(|value| String::try_from(value.try_clone().ok()?).ok())
            .unwrap_or_default()
    }

    fn flag(properties: &Interfaces, interface: &str, key: &str) -> bool {
        properties
            .get(interface)
            .and_then(|found| found.get(key))
            .and_then(|value| bool::try_from(value.try_clone().ok()?).ok())
            .unwrap_or(false)
    }

    fn number(properties: &Interfaces, interface: &str, key: &str) -> u64 {
        properties
            .get(interface)
            .and_then(|found| found.get(key))
            .and_then(|value| u64::try_from(value.try_clone().ok()?).ok())
            .unwrap_or(0)
    }

    fn object(properties: &Interfaces, interface: &str, key: &str) -> String {
        properties
            .get(interface)
            .and_then(|found| found.get(key))
            .and_then(|value| OwnedObjectPath::try_from(value.try_clone().ok()?).ok())
            .map(|path| path.to_string())
            .unwrap_or_default()
    }

    /// A string of bytes with the nothing at its end taken off, which is how udisks gives a device
    /// name and a mount point.
    fn path_of(bytes: &zbus::zvariant::Value<'_>) -> Option<PathBuf> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let array = Array::try_from(bytes.try_clone().ok()?).ok()?;
        let mut found = Vec::<u8>::try_from(array).ok()?;
        while found.last() == Some(&0) {
            found.pop();
        }
        (!found.is_empty()).then(|| PathBuf::from(OsString::from_vec(found)))
    }

    fn device(properties: &Interfaces) -> String {
        properties
            .get(BLOCK)
            .and_then(|block| block.get("Device"))
            .and_then(|device| path_of(device))
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    }

    /// Where a file system is mounted, the first place when it is mounted in more than one.
    fn mount_of(properties: &Interfaces) -> Option<PathBuf> {
        let points = properties.get(FILESYSTEM)?.get("MountPoints")?;
        let array = Array::try_from(points.try_clone().ok()?).ok()?;
        array.iter().find_map(path_of)
    }

    /// The disks a person plugged in, in the order the sidebar lists them.
    ///
    /// # Errors
    ///
    /// A sentence when udisks cannot be reached or does not answer.
    pub fn volumes() -> Result<Vec<Volume>, String> {
        let connection = bus::connect(TIMEOUT)?;
        let objects = managed(&connection)?;
        Ok(listed(&objects, own_disk().as_deref()))
    }

    /// The object a block names, when it names one at all.
    fn named(properties: &Interfaces, interface: &str, key: &str) -> Option<String> {
        let path = object(properties, interface, key);
        (!path.is_empty() && path != "/").then_some(path)
    }

    /// The disk a block is on: its own, or, when it has none, the disk the volume it came out of
    /// is on. udisks gives the cleartext device of an unlocked disk no drive of its own, and
    /// without this the file system that came out of a locked disk would be listed nowhere.
    fn drive_of(objects: &Objects, properties: &Interfaces) -> String {
        if let Some(drive) = named(properties, BLOCK, "Drive") {
            return drive;
        }
        named(properties, BLOCK, "CryptoBackingDevice")
            .and_then(|backing| objects.get(&OwnedObjectPath::try_from(backing.as_str()).ok()?))
            .and_then(|backing| named(backing, BLOCK, "Drive"))
            .unwrap_or_default()
    }

    /// What the sidebar lists, out of everything udisks knows.
    fn listed(objects: &Objects, own: Option<&Path>) -> Vec<Volume> {
        let ours = own.and_then(|disk| {
            objects
                .iter()
                .find(|(_, properties)| device(properties) == disk.display().to_string())
                .map(|(_, properties)| object(properties, BLOCK, "Drive"))
        });
        // a locked volume that has been unlocked shows as the file system inside it, not twice
        let unlocked: Vec<String> = objects
            .values()
            .map(|properties| object(properties, BLOCK, "CryptoBackingDevice"))
            .filter(|backing| !backing.is_empty() && backing != "/")
            .collect();
        let mut found: Vec<Volume> = objects
            .iter()
            .filter_map(|(path, properties)| {
                let drive = drive_of(objects, properties);
                let disk = objects.get(&OwnedObjectPath::try_from(drive.as_str()).ok()?)?;
                let volume = one(path.as_str(), properties, disk, &drive)?;
                let mine = ours.as_ref() == Some(&drive);
                let shown = !mine && !unlocked.contains(&volume.id);
                shown.then_some(volume)
            })
            .collect();
        found.sort_by(|one, other| {
            crate::files::natural(&one.name, &other.name).then_with(|| one.id.cmp(&other.id))
        });
        found
    }

    /// One row, when this object is a volume the sidebar lists: a file system or a locked volume,
    /// on a disk that is removable or on USB, that udisks does not say to hide.
    fn one(path: &str, properties: &Interfaces, disk: &Interfaces, drive: &str) -> Option<Volume> {
        if !properties.contains_key(BLOCK) || flag(properties, BLOCK, "HintIgnore") {
            return None;
        }
        let encrypted = properties.contains_key(ENCRYPTED);
        if !properties.contains_key(FILESYSTEM) && !encrypted {
            return None;
        }
        let bus = text(disk, DRIVE, "ConnectionBus");
        if !flag(disk, DRIVE, "Removable") && bus != "usb" {
            return None;
        }
        let size = number(properties, BLOCK, "Size");
        if size == 0 {
            return None;
        }
        let mount = mount_of(properties);
        let locked = encrypted && mount.is_none();
        let label = text(properties, BLOCK, "IdLabel");
        let model = [text(disk, DRIVE, "Vendor"), text(disk, DRIVE, "Model")]
            .join(" ")
            .trim()
            .to_string();
        let name = if label.is_empty() {
            if model.is_empty() {
                format!("{} volume", crate::files::size_words(size))
            } else {
                model
            }
        } else {
            label
        };
        Some(Volume {
            id: path.to_string(),
            name,
            device: device(properties),
            fs: text(properties, BLOCK, "IdType"),
            size,
            mount,
            locked,
            drive: drive.to_string(),
            eject: flag(disk, DRIVE, "Ejectable") || flag(disk, DRIVE, "CanPowerOff"),
            icon: icon_of(&bus, locked),
        })
    }

    /// A proxy for one interface of one udisks object.
    fn on(
        connection: &zbus::blocking::Connection,
        id: &str,
        interface: &'static str,
    ) -> Result<zbus::blocking::Proxy<'static>, String> {
        bus::object(connection, SERVICE, id, interface).map_err(|e| bus::sentence_for(NAME, e))
    }

    /// Mount a volume, and say where it went. udisks picks the folder, `/run/media/<account>/<the
    /// volume's name>`, and gives a file system with no owners of its own to the account that
    /// asked for it.
    ///
    /// # Errors
    ///
    /// A sentence when udisks refuses or the file system cannot be read.
    pub fn mount(id: &str) -> Result<PathBuf, String> {
        let connection = bus::connect(TIMEOUT)?;
        let options: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
        let where_it_went: String = on(&connection, id, FILESYSTEM)?
            .call("Mount", &(options,))
            .map_err(|e| refusal(e, "mount"))?;
        Ok(PathBuf::from(where_it_went))
    }

    /// Unlock an encrypted volume with a passphrase, and say what came out of it: the object of
    /// the file system inside, which is the one to mount. udisks holds the passphrase only as long
    /// as the call takes.
    ///
    /// # Errors
    ///
    /// A sentence when the passphrase does not open it, when udisks refuses, or when the volume is
    /// not there.
    pub fn unlock(id: &str, passphrase: &str) -> Result<String, String> {
        let connection = bus::connect(TIMEOUT)?;
        let options: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
        let inside: OwnedObjectPath = on(&connection, id, ENCRYPTED)?
            .call("Unlock", &(passphrase, options))
            .map_err(|e| wrong_passphrase(e, "unlock"))?;
        Ok(inside.to_string())
    }

    /// The sentence for an unlock that did not happen. A passphrase that does not open the volume
    /// is the everyday answer, and udisks says so in a message of its own.
    fn wrong_passphrase(error: zbus::Error, doing: &str) -> String {
        if let zbus::Error::MethodError(_, Some(said), _) = &error
            && (said.contains("No key available")
                || said.contains("Failed to activate device")
                || said.contains("wrong passphrase")
                || said.contains("Wrong passphrase"))
        {
            return "That passphrase does not open this disk.".to_string();
        }
        refusal(error, doing)
    }

    /// Unmount a volume, writing out everything that was waiting.
    ///
    /// # Errors
    ///
    /// A sentence when udisks refuses, or something still has the volume open.
    pub fn unmount(id: &str) -> Result<(), String> {
        let connection = bus::connect(TIMEOUT)?;
        unmount_on(&connection, id)
    }

    fn unmount_on(connection: &zbus::blocking::Connection, id: &str) -> Result<(), String> {
        let options: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
        on(connection, id, FILESYSTEM)?
            .call::<_, _, ()>("Unmount", &(options,))
            .map_err(|e| refusal(e, "unmount"))
    }

    /// Unmount everything on the disk this volume is on, lock whatever was unlocked on it, then
    /// eject it or switch it off, so the stick can be pulled out.
    ///
    /// # Errors
    ///
    /// A sentence when something still has a file system on it open, or udisks refuses.
    pub fn eject(id: &str) -> Result<(), String> {
        let connection = bus::connect(TIMEOUT)?;
        let objects = managed(&connection)?;
        let drive = objects
            .get(
                &OwnedObjectPath::try_from(id)
                    .map_err(|_| "That disk is not there.".to_string())?,
            )
            .map(|properties| drive_of(&objects, properties))
            .filter(|drive| !drive.is_empty())
            .ok_or_else(|| "That disk is not there.".to_string())?;
        let on_disk = |properties: &Interfaces| drive_of(&objects, properties) == drive;
        for (path, properties) in &objects {
            if on_disk(properties) && mount_of(properties).is_some() {
                unmount_on(&connection, path.as_str())?;
            }
        }
        // an encrypted volume that is open holds its disk, so it is shut before the disk goes
        for (path, properties) in &objects {
            if on_disk(properties) && named(properties, ENCRYPTED, "CleartextDevice").is_some() {
                let options: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
                on(&connection, path.as_str(), ENCRYPTED)?
                    .call::<_, _, ()>("Lock", &(options,))
                    .map_err(|e| refusal(e, "close"))?;
            }
        }
        let disk = objects
            .get(
                &OwnedObjectPath::try_from(drive.as_str())
                    .map_err(|_| "That disk is not there.")?,
            )
            .ok_or_else(|| "That disk is not there.".to_string())?;
        let options: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
        if flag(disk, DRIVE, "Ejectable") {
            on(&connection, &drive, DRIVE)?
                .call::<_, _, ()>("Eject", &(options.clone(),))
                .map_err(|e| refusal(e, "eject"))?;
        }
        if flag(disk, DRIVE, "CanPowerOff") {
            on(&connection, &drive, DRIVE)?
                .call::<_, _, ()>("PowerOff", &(options,))
                .map_err(|e| refusal(e, "switch off"))?;
        }
        Ok(())
    }

    /// The sentence for a call udisks would not do. A refusal from polkit is the rule that keeps
    /// the disks of the machine as they are, and it is worth saying so.
    fn refusal(error: zbus::Error, doing: &str) -> String {
        if bus::refused(&error) {
            return format!(
                "Rift does not {doing} a disk that belongs to this computer, only one you plugged in."
            );
        }
        bus::sentence_for(NAME, error)
    }

    /// Call `each` whenever udisks has something new to say: a disk plugged in or pulled out, a
    /// volume mounted or unmounted. Blocks, so the caller runs it on a thread of its own.
    ///
    /// # Errors
    ///
    /// When the bus cannot be reached or closes the connection.
    pub fn watch<F: FnMut() -> bool>(each: F) -> Result<(), String> {
        bus::signals(SERVICE, each)
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        fn value(of: impl Into<zbus::zvariant::Value<'static>>) -> OwnedValue {
            OwnedValue::try_from(of.into()).expect("a property")
        }

        /// One object's interfaces, each with the properties named.
        fn object_of(pairs: &[(&str, &[(&str, OwnedValue)])]) -> Interfaces {
            pairs
                .iter()
                .map(|(interface, properties)| {
                    (
                        (*interface).to_string(),
                        properties
                            .iter()
                            .map(|(key, held)| {
                                ((*key).to_string(), held.try_clone().expect("a property"))
                            })
                            .collect(),
                    )
                })
                .collect()
        }

        fn path_of_str(path: &str) -> OwnedObjectPath {
            OwnedObjectPath::try_from(path).expect("an object path")
        }

        /// A disk and one file system on it, the way udisks answers.
        fn disk(
            objects: &mut Objects,
            name: &str,
            removable: bool,
            label: &str,
            mounted: Option<&str>,
        ) {
            let drive = format!("/org/freedesktop/UDisks2/drives/{name}");
            objects.insert(
                path_of_str(&drive),
                object_of(&[(
                    DRIVE,
                    &[
                        ("Removable", value(removable)),
                        ("ConnectionBus", value(String::new())),
                        ("Ejectable", value(removable)),
                        ("CanPowerOff", value(removable)),
                        ("Vendor", value("QEMU".to_string())),
                        ("Model", value("HARDDISK".to_string())),
                    ],
                )]),
            );
            let mut block: Vec<(&str, OwnedValue)> = vec![
                ("IdLabel", value(label.to_string())),
                ("IdType", value("exfat".to_string())),
                ("Size", value(64u64 * 1024 * 1024)),
                ("HintIgnore", value(false)),
                ("Drive", value(path_of_str(&drive))),
                ("Device", value(format!("/dev/{name}\0").into_bytes())),
            ];
            let mut points: Vec<(&str, OwnedValue)> = Vec::new();
            if let Some(mount) = mounted {
                points.push((
                    "MountPoints",
                    value(vec![format!("{mount}\0").into_bytes()]),
                ));
            }
            block.push(("ReadOnly", value(false)));
            objects.insert(
                path_of_str(&format!("/org/freedesktop/UDisks2/block_devices/{name}")),
                object_of(&[(BLOCK, &block), (FILESYSTEM, &points)]),
            );
        }

        /// A locked disk that has been unlocked: the volume itself, which udisks says is
        /// encrypted, and the file system that came out of it, which has no drive of its own.
        fn unlocked(objects: &mut Objects, name: &str, mounted: &str) {
            let drive = format!("/org/freedesktop/UDisks2/drives/{name}");
            objects.insert(
                path_of_str(&drive),
                object_of(&[(
                    DRIVE,
                    &[
                        ("Removable", value(true)),
                        ("ConnectionBus", value("usb".to_string())),
                        ("Ejectable", value(true)),
                        ("CanPowerOff", value(true)),
                        ("Vendor", value("QEMU".to_string())),
                        ("Model", value("HARDDISK".to_string())),
                    ],
                )]),
            );
            let volume = format!("/org/freedesktop/UDisks2/block_devices/{name}");
            objects.insert(
                path_of_str(&volume),
                object_of(&[
                    (
                        BLOCK,
                        &[
                            ("IdLabel", value("LOCKED".to_string())),
                            ("IdType", value("crypto_LUKS".to_string())),
                            ("Size", value(64u64 * 1024 * 1024)),
                            ("HintIgnore", value(false)),
                            ("Drive", value(path_of_str(&drive))),
                            ("Device", value(format!("/dev/{name}\0").into_bytes())),
                        ],
                    ),
                    (
                        ENCRYPTED,
                        &[(
                            "CleartextDevice",
                            value(path_of_str("/org/freedesktop/UDisks2/block_devices/dm_2d0")),
                        )],
                    ),
                ]),
            );
            objects.insert(
                path_of_str("/org/freedesktop/UDisks2/block_devices/dm_2d0"),
                object_of(&[
                    (
                        BLOCK,
                        &[
                            ("IdLabel", value("PRIVATE".to_string())),
                            ("IdType", value("ext4".to_string())),
                            ("Size", value(63u64 * 1024 * 1024)),
                            ("HintIgnore", value(false)),
                            // the cleartext device is on no drive udisks knows
                            ("Drive", value(path_of_str("/"))),
                            ("CryptoBackingDevice", value(path_of_str(&volume))),
                            ("Device", value(b"/dev/dm-0\0".to_vec())),
                        ],
                    ),
                    (
                        FILESYSTEM,
                        &[(
                            "MountPoints",
                            value(vec![format!("{mounted}\0").into_bytes()]),
                        )],
                    ),
                ]),
            );
        }

        #[test]
        fn what_came_out_of_a_locked_disk_is_a_row_like_any_other() {
            let mut objects = Objects::new();
            disk(&mut objects, "sda", true, "EXCHANGE", None);
            unlocked(&mut objects, "sdc", "/run/media/rift/PRIVATE");
            let ours = listed(&objects, Some(Path::new("/dev/sda")));
            let names: Vec<&str> = ours.iter().map(|volume| volume.name.as_str()).collect();
            // the locked volume is gone from the list, the file system in it is there instead
            assert_eq!(names, ["PRIVATE"]);
            assert_eq!(ours[0].device, "/dev/dm-0");
            assert_eq!(
                ours[0].mount.as_deref(),
                Some(Path::new("/run/media/rift/PRIVATE"))
            );
            // and it carries the disk it came out of, so it can be unmounted and ejected
            assert_eq!(ours[0].drive, "/org/freedesktop/UDisks2/drives/sdc");
            assert!(ours[0].eject && !ours[0].locked);
            assert_eq!(ours[0].icon, "drive-removable-media-symbolic");
        }

        #[test]
        fn only_the_disks_a_person_plugged_in_are_listed() {
            let mut objects = Objects::new();
            // the drive rift runs from, which is removable on a real stick
            disk(&mut objects, "sda", true, "EXCHANGE", None);
            // a disk of the machine, which polkit would refuse anyway
            disk(
                &mut objects,
                "nvme0n1",
                false,
                "backup",
                Some("/mnt/backup"),
            );
            // and a memory stick, mounted
            disk(
                &mut objects,
                "sdb",
                true,
                "STICK",
                Some("/run/media/rift/STICK"),
            );
            let ours = listed(&objects, Some(Path::new("/dev/sda")));
            let names: Vec<&str> = ours.iter().map(|volume| volume.name.as_str()).collect();
            assert_eq!(names, ["STICK"]);
            assert_eq!(ours[0].device, "/dev/sdb");
            assert_eq!(
                ours[0].mount.as_deref(),
                Some(Path::new("/run/media/rift/STICK"))
            );
            assert!(ours[0].eject && !ours[0].locked);
            assert_eq!(ours[0].icon, "drive-harddisk-symbolic");
            // with no drive of its own to leave out, the machine's disk is still not one
            let every = listed(&objects, None);
            let names: Vec<&str> = every.iter().map(|volume| volume.name.as_str()).collect();
            assert_eq!(names, ["EXCHANGE", "STICK"]);
        }
    }
}

#[cfg(feature = "bus")]
pub use asking::{eject, mount, unlock, unmount, volumes, watch};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disk_is_drawn_by_the_way_it_is_attached() {
        assert_eq!(icon_of("usb", false), "drive-removable-media-symbolic");
        assert_eq!(icon_of("sdio", false), "drive-harddisk-symbolic");
        assert_eq!(icon_of("usb", true), "changes-prevent-symbolic");
    }

    #[test]
    fn the_exchange_partition_is_a_folder_of_its_own_or_nothing() {
        // the folder is not mounted here, and a folder on the same file system as its parent is
        // not a partition
        assert_eq!(exchange(), None);
    }

    #[test]
    fn a_partition_the_drive_has_stays_a_row_when_it_is_unmounted() {
        // nothing is mounted at /exchange on the machine the tests run on, so again() is what a
        // ghost session reads: a drive that has one keeps its row, a drive that has none has no row
        assert_eq!(Exchange::There.again(), Exchange::There);
        assert_eq!(
            Exchange::At(PathBuf::from(EXCHANGE)).again(),
            Exchange::There
        );
        assert_eq!(Exchange::None.again(), Exchange::None);
        assert_eq!(Exchange::None.word(), "none");
        assert_eq!(Exchange::There.word(), "there");
        assert_eq!(Exchange::At(PathBuf::from("/exchange")).word(), "/exchange");
        assert!(!Exchange::None.listed());
        assert!(Exchange::There.listed() && Exchange::There.mount().is_none());
        let mounted = Exchange::At(PathBuf::from("/exchange"));
        assert_eq!(mounted.mount(), Some(Path::new("/exchange")));
    }
}
