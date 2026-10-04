//! The exchange partition of the drive: the plain one that Windows, macOS and Linux can all read,
//! written when the drive was made with one. Nothing else on the drive is a partition a person
//! opens, so nothing else is mounted here.
//!
//! udisks does not mount it. On the machines Rift runs on the drive is a USB stick, so udisks
//! would let the owner mount any partition of it, including the ones the system runs from; in a
//! virtual machine the drive is an internal disk and udisks refuses the mount outright, which is
//! the rule that keeps a host's disks as they are. Either way it is the drive's own partition, so
//! the system mounts it, at boot, at [`FOLDER`].
//!
//! It is found the way a clone finds the running drive: udev names the esp of the drive the system
//! started from, the partition table of the disk that esp is on says which partition is called
//! `exchange`, and its partition uuid names the device. A partition of that name on any other disk
//! is never touched. udev has no name of its own for it, the way it has `esp` for the esp, because
//! the designators it knows are the ones in the discoverable partitions specification and this is
//! basic data, so the table is the only place the name is written down.
//!
//! A Ghost boot does not mount it, because mounting a vfat writes to it, and the owner can still
//! ask for it from Files, which is what [`Exchange::there`] and the `MountExchange` method on the
//! bus are for (ADR-0085). Reading the table to say the drive has one writes nothing.

use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use librift::disk::{GPT_BYTES, read_gpt};

use crate::restore::group_of;
use crate::slots::disk_of;

/// The name the partition has in the drive's table.
const LABEL: &str = "exchange";
/// Where it is mounted.
pub const FOLDER: &str = "/exchange";
/// The unit that mounts it when the owner asks, which is the twin of the one the boot runs: it
/// carries no condition and nothing wants it, so it runs only when it is started by hand. Vault
/// answers the bus inside a mount namespace of its own, where a mount would be invisible to
/// everything else on the machine, the way vault-owner.service writes the password files outside it.
const ASKED: &str = "vault-exchange-asked.service";
/// The file systems that keep no owners of their own and take the mount's instead. exFAT is what
/// a new drive's exchange partition is formatted as, and the other three are what a person who
/// formats it on another system is likely to leave.
const NO_OWNERS: [&str; 4] = ["exfat", "vfat", "ntfs", "ntfs3"];

/// Where the exchange partition is looked for and mounted.
#[derive(Debug, Clone)]
pub struct Exchange {
    /// udev's names for the partitions of the drive the system started from.
    pub designators: PathBuf,
    /// Where udev names every partition by its own uuid.
    pub partuuids: PathBuf,
    /// Where the partition is mounted.
    pub folder: PathBuf,
    /// The password file the owner's account is in.
    pub passwd: PathBuf,
    /// The account the partition belongs to, since exFAT keeps no owners.
    pub user: String,
}

impl Exchange {
    /// Whether this drive has an exchange partition with a file system on it, which is the one
    /// thing a sidebar needs to know that only root can read: the partition table of the drive.
    ///
    /// False for a drive written without one, and for one whose partition has no file system yet,
    /// which is what a drive written on macOS or Windows has until its first boot formats it.
    ///
    /// # Errors
    ///
    /// A sentence when the drive's table or the partition could not be read.
    pub fn there(&self) -> Result<bool, String> {
        let Some(device) = self.device()? else {
            return Ok(false);
        };
        Ok(!fstype(&device)?.is_empty())
    }

    /// Mount it, and say so. Nothing when the drive has no exchange partition, when it holds no
    /// file system yet, or when it is mounted already.
    ///
    /// # Errors
    ///
    /// A sentence when the partition is there and the mount failed.
    pub fn mount(&self) -> Result<Option<String>, String> {
        let Some(device) = self.device()? else {
            return Ok(None);
        };
        let kind = fstype(&device)?;
        if kind.is_empty() {
            return Ok(None);
        }
        if librift::drives::exchange().is_some() {
            return Ok(Some(format!(
                "The exchange partition is already at {}.",
                self.folder.display()
            )));
        }
        fs::create_dir_all(&self.folder)
            .map_err(|e| format!("Could not make {}: {e}", self.folder.display()))?;
        let options = self.options(&kind);
        let done = Command::new("mount")
            .args(["-t", &kind, "-o", &options])
            .arg(&device)
            .arg(&self.folder)
            .output()
            .map_err(|e| format!("Could not run mount: {e}"))?;
        if !done.status.success() {
            let said = String::from_utf8_lossy(&done.stderr);
            return Err(format!(
                "Could not mount the exchange partition, {}: {}",
                device.display(),
                said.trim()
            ));
        }
        Ok(Some(format!(
            "Mounted the exchange partition, {} ({kind}), at {}.",
            device.display(),
            self.folder.display()
        )))
    }

    /// The device the exchange partition of this drive is, when the drive has one.
    fn device(&self) -> Result<Option<PathBuf>, String> {
        let Ok(esp) = fs::canonicalize(self.designators.join("esp")) else {
            return Ok(None);
        };
        let disk = disk_of(&esp)?;
        let mut bytes = vec![0; GPT_BYTES];
        File::open(&disk)
            .and_then(|mut drive| drive.read_exact(&mut bytes))
            .map_err(|e| format!("Could not read {}: {e}", disk.display()))?;
        let table = read_gpt(&bytes)
            .map_err(|why| format!("Could not read the partitions of {}. {why}", disk.display()))?;
        let Some(uuid) = table
            .partitions
            .iter()
            .find(|partition| partition.name.as_deref() == Some(LABEL))
            .and_then(|partition| partition.uuid.as_deref())
        else {
            return Ok(None);
        };
        Ok(fs::canonicalize(self.partuuids.join(uuid.to_lowercase())).ok())
    }

    /// How it is mounted: a file system with no owners of its own is the owner's alone, since the
    /// drive has one person on it and the partition is not encrypted.
    fn options(&self, kind: &str) -> String {
        if !NO_OWNERS.contains(&kind) {
            return "noatime".to_string();
        }
        let (uid, gid) = self.account().unwrap_or((0, 0));
        format!("noatime,uid={uid},gid={gid},fmask=0177,dmask=0077")
    }

    /// The owner's account and its group.
    fn account(&self) -> Option<(u32, u32)> {
        let passwd = fs::read_to_string(&self.passwd).ok()?;
        let account = librift::owner::account(&passwd, &self.user)?;
        let gid = group_of(&passwd, account.uid)?;
        Some((account.uid, gid))
    }
}

/// Have the unit mount it now, and say where it went. What the `MountExchange` method on the bus
/// runs, and the only thing that mounts the partition in a Ghost boot.
///
/// # Errors
///
/// A sentence when systemctl could not be run or the unit failed.
pub fn mount_now() -> Result<String, String> {
    let done = Command::new("systemctl")
        .args(["start", ASKED])
        .output()
        .map_err(|e| format!("Could not run systemctl: {e}"))?;
    if !done.status.success() {
        return Err(format!(
            "Could not mount the exchange partition: {}",
            String::from_utf8_lossy(&done.stderr).trim()
        ));
    }
    Ok(FOLDER.to_string())
}

/// The kind of file system on a device, as the kernel's own probe reads it. Empty when there is
/// none, which is what a drive written on macOS or Windows has until its first boot formats it.
fn fstype(device: &Path) -> Result<String, String> {
    let done = Command::new("lsblk")
        .args(["--noheadings", "--output", "FSTYPE"])
        .arg(device)
        .output()
        .map_err(|e| format!("Could not run lsblk: {e}"))?;
    if !done.status.success() {
        return Err(format!(
            "Could not read what is on {}: {}",
            device.display(),
            String::from_utf8_lossy(&done.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&done.stdout).trim().to_string())
}

impl Default for Exchange {
    fn default() -> Self {
        Self {
            designators: PathBuf::from(crate::DESIGNATORS),
            partuuids: PathBuf::from("/dev/disk/by-partuuid"),
            folder: PathBuf::from(FOLDER),
            passwd: PathBuf::from("/etc/passwd"),
            user: librift::owner::USER.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_system_with_no_owners_of_its_own_is_the_owners() {
        let exchange = Exchange {
            passwd: PathBuf::from("/nowhere/passwd"),
            ..Exchange::default()
        };
        assert_eq!(
            exchange.options("exfat"),
            "noatime,uid=0,gid=0,fmask=0177,dmask=0077"
        );
        assert_eq!(exchange.options("ext4"), "noatime");
    }

    #[test]
    fn a_drive_with_no_esp_of_its_own_has_nothing_to_mount() {
        let exchange = Exchange {
            designators: PathBuf::from("/nowhere/by-designator"),
            ..Exchange::default()
        };
        assert_eq!(exchange.device(), Ok(None));
        assert_eq!(exchange.mount(), Ok(None));
        assert_eq!(exchange.there(), Ok(false));
    }
}
