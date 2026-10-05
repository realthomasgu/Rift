//! What the two slots of the drive hold, for the Updates page.
//!
//! A slot is a store partition and the verity tree beside it, and systemd-sysupdate writes a new
//! version into the one that is not running. None of it can be read without root: the esp is
//! mounted for root alone and the labels that say which version is where are on the drive itself.
//! Vault owns the drive and runs as root, so it reads it and answers on the bus.

use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use librift::disk::{GPT_BYTES, Partition, Table, USR_TYPE, USR_VERITY_TYPE, read_gpt};
use librift::update::{self, Slot, Slots};

use crate::boot::Esp;
use crate::clone::os_release;

/// The name udev gives the esp of the drive the running system is on.
const ESP: &str = "esp";
/// Where the kernel keeps what it knows about a block device.
const SYSFS: &str = "/sys/class/block";
/// The slots, in the order the drive lays them out.
const SLOTS: [&str; 2] = ["a", "b"];
/// What a store partition's label starts with, before the version it holds.
const STORE: &str = "store_";

/// The drive the running system started from.
pub struct Drive {
    /// Where udev names its partitions, `/dev/disk/by-designator`.
    designators: PathBuf,
    /// What the running system calls itself, `/etc/os-release`.
    release: PathBuf,
    /// The systemd-sysupdate transfer files, `/etc/sysupdate.d`, which say where updates come
    /// from.
    transfers: PathBuf,
}

impl Drive {
    /// The drive named under `designators`, with os-release at `release` and the transfer files
    /// in `transfers`.
    #[must_use]
    pub fn new(designators: PathBuf, release: PathBuf, transfers: PathBuf) -> Self {
        Self {
            designators,
            release,
            transfers,
        }
    }

    /// What the slots hold, which version is running, where updates come from and what is waiting
    /// there. The esp is read through `esp`, which mounts it for as long as it takes.
    ///
    /// # Errors
    ///
    /// A sentence when the running system does not say which version it is, or the drive's
    /// partition table or esp could not be read.
    pub fn slots(&self, esp: &Esp) -> Result<Slots, String> {
        let text = fs::read_to_string(&self.release)
            .map_err(|e| format!("Could not read {}: {e}", self.release.display()))?;
        let (id, running) =
            os_release(&text).ok_or("The running system does not say its image id and version.")?;
        let table = self.table()?;
        let ukis = esp.ukis()?;
        let slots = SLOTS
            .iter()
            .zip(stores(&table)?)
            .map(|(&slot, store)| Slot {
                slot: slot.to_string(),
                uki: update::uki_of(&ukis, &id, &store).unwrap_or_default(),
                version: store,
            })
            .collect();
        let source = self.source();
        Ok(Slots {
            running,
            slots,
            waiting: waiting(&source, &id),
            source,
        })
    }

    /// The partition table of the drive the esp is on, read off the drive itself.
    fn table(&self) -> Result<Table, String> {
        Ok(drive_table(&self.designators)?.1)
    }

    /// Where updates come from, out of the transfer files. Every part of a version comes from the
    /// same place, so the first file that says where is the answer. Empty when nothing says.
    fn source(&self) -> String {
        let mut files: Vec<PathBuf> = fs::read_dir(&self.transfers)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        files.sort();
        files
            .iter()
            .filter_map(|file| fs::read_to_string(file).ok())
            .find_map(|text| update::source_of(&text).map(ToString::to_string))
            .unwrap_or_default()
    }
}

/// The drive the running system started from and its partition table, read off the drive itself.
/// sfdisk would say the same, but this asks nothing of a program that may not be there.
///
/// # Errors
///
/// A sentence when the drive has no boot partition, or its table could not be read.
pub fn drive_table(designators: &Path) -> Result<(PathBuf, Table), String> {
    let esp = fs::canonicalize(designators.join(ESP))
        .map_err(|_| "The drive this system started from has no boot partition.".to_string())?;
    let disk = disk_of(&esp)?;
    let mut bytes = vec![0; GPT_BYTES];
    File::open(&disk)
        .and_then(|mut drive| drive.read_exact(&mut bytes))
        .map_err(|e| format!("Could not read {}: {e}", disk.display()))?;
    let table = read_gpt(&bytes)
        .map_err(|why| format!("Could not read the partitions of {}. {why}", disk.display()))?;
    Ok((disk, table))
}

/// The two partitions of each slot, slot a first: the verity partition and the store beside it, in
/// the order the drive lays them out, with the number each has in the table.
///
/// # Errors
///
/// A sentence when the drive is not laid out in two slots.
pub fn slot_partitions(table: &Table) -> Result<Vec<(usize, usize)>, String> {
    let of = |kind: &str| -> Vec<usize> {
        table
            .partitions
            .iter()
            .enumerate()
            .filter(|(_, partition)| partition.kind.eq_ignore_ascii_case(kind))
            .map(|(index, _)| index + 1)
            .collect()
    };
    let (verity, store) = (of(USR_VERITY_TYPE), of(USR_TYPE));
    if verity.len() != SLOTS.len() || store.len() != SLOTS.len() {
        return Err(format!(
            "The drive this system started from has {} stores and {} hash trees, and a Rift drive \
             has two of each.",
            store.len(),
            verity.len()
        ));
    }
    Ok(verity.into_iter().zip(store).collect())
}

/// The version each slot holds, slot a first: the store partitions of the table, which are the
/// ones of the usr type, in the order the drive lays them out. A slot that holds no version is
/// labelled `_empty`, and its version here is nothing.
fn stores(table: &Table) -> Result<Vec<String>, String> {
    let stores: Vec<&Partition> = table
        .partitions
        .iter()
        .filter(|partition| partition.kind.eq_ignore_ascii_case(USR_TYPE))
        .collect();
    if stores.len() != SLOTS.len() {
        return Err(format!(
            "The drive this system started from has {} slots, and a Rift drive has two.",
            stores.len()
        ));
    }
    Ok(stores
        .iter()
        .map(|store| {
            store
                .name
                .as_deref()
                .and_then(|label| label.strip_prefix(STORE))
                .unwrap_or_default()
                .to_string()
        })
        .collect())
}

/// The versions waiting where updates come from, oldest first. A source this machine cannot look
/// in, a channel on the network, has none until it is asked.
fn waiting(source: &str, id: &str) -> Vec<String> {
    let Some(folder) = update::folder(source) else {
        return Vec::new();
    };
    let mut found: Vec<String> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            update::version_of(name.to_str()?, id).map(ToString::to_string)
        })
        .collect();
    found.sort_by(|one, other| update::compare(one, other));
    found.dedup();
    found
}

/// The disk a partition is on: `/sys/class/block/<partition>` is a link into the device tree, and
/// the folder above it there is the disk the partition belongs to.
pub fn disk_of(partition: &Path) -> Result<PathBuf, String> {
    let name = partition
        .file_name()
        .ok_or_else(|| format!("{} is not a partition.", partition.display()))?;
    let known = fs::canonicalize(Path::new(SYSFS).join(name))
        .map_err(|e| format!("The kernel says nothing about {}: {e}", partition.display()))?;
    let disk = known
        .parent()
        .and_then(Path::file_name)
        .ok_or_else(|| format!("{} is not a partition of a disk.", partition.display()))?;
    Ok(Path::new("/dev").join(disk))
}

#[cfg(test)]
mod tests {
    use super::*;
    use librift::disk::{ESP_TYPE, LINUX_TYPE};

    fn partition(kind: &str, name: &str) -> Partition {
        Partition {
            node: name.to_string(),
            start: 0,
            size: 0,
            kind: kind.to_string(),
            uuid: None,
            name: Some(name.to_string()),
            attrs: None,
        }
    }

    fn table(a: &str, b: &str) -> Table {
        Table {
            id: None,
            sectorsize: 512,
            partitions: vec![
                partition(ESP_TYPE, "esp"),
                partition(USR_VERITY_TYPE, &format!("store-verity_{a}")),
                partition(USR_TYPE, a),
                partition(USR_VERITY_TYPE, "_empty"),
                partition(USR_TYPE, b),
                partition(LINUX_TYPE, "persist"),
            ],
        }
    }

    #[test]
    fn the_two_partitions_of_each_slot_are_numbered_the_way_the_drive_lays_them_out() {
        assert_eq!(
            slot_partitions(&table("store_0.1.0", "store_0.2.0")).unwrap(),
            [(2, 3), (4, 5)]
        );
        let mut one = table("store_0.1.0", "_empty");
        one.partitions.remove(4);
        let why = slot_partitions(&one).unwrap_err();
        assert!(why.contains("has 1 stores and 2 hash trees"), "{why}");
    }

    #[test]
    fn the_stores_of_the_table_are_the_two_slots() {
        assert_eq!(
            stores(&table("store_0.1.0", "store_0.2.0")).unwrap(),
            ["0.1.0", "0.2.0"]
        );
        // an empty slot is labelled _empty and holds no version
        assert_eq!(
            stores(&table("store_0.1.0", "_empty")).unwrap(),
            ["0.1.0", ""]
        );
    }

    #[test]
    fn a_drive_that_is_not_laid_out_in_two_slots_says_so() {
        let mut one = table("store_0.1.0", "_empty");
        one.partitions.remove(4);
        let why = stores(&one).unwrap_err();
        assert!(why.contains("has 1 slots"), "{why}");
        let none = Table {
            partitions: Vec::new(),
            ..one
        };
        assert!(stores(&none).is_err());
    }

    #[test]
    fn nothing_is_waiting_on_a_source_this_machine_cannot_look_in() {
        assert!(waiting("https://rift.example/updates/", "rift").is_empty());
        assert!(waiting("", "rift").is_empty());
        // and a folder that is not there holds nothing
        assert!(waiting("file:///no/such/folder", "rift").is_empty());
    }
}
