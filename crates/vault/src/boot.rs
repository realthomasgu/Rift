//! The esp of the drive this system started from: the boot style on it, and the ukis it holds.
//!
//! The esp is root's, mounted at /boot for root alone, so the owner can neither write the word
//! that says how the next boot looks nor see which versions the drive is able to start. Vault owns
//! the drive and runs as root, so it does both for them: the methods on the bus mount the esp under
//! the service's own runtime directory, read or write it, and let it go again. The initrd reads the
//! same file before plymouth starts.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use librift::boot::Style;
use librift::disk::run::Mounted;

/// Numbers the mount points, so two calls at once never pick the same folder.
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The name udev gives the esp of the drive the running system is on. It names no host disk's.
const ESP: &str = "esp";

/// Where the ukis are on the esp, the folder systemd-boot reads its entries from.
const LINUX: &str = "EFI/Linux";

/// Where the esp is, and where it is mounted while Vault reads or writes it.
pub struct Esp {
    /// Where udev names the running drive's partitions, `/dev/disk/by-designator`.
    designators: PathBuf,
    /// Where the esp is mounted while Vault uses it, `/run/vault`.
    run: PathBuf,
}

impl Esp {
    /// The esp named under `designators`, mounted under `run` while it is read or written.
    #[must_use]
    pub fn new(designators: PathBuf, run: PathBuf) -> Self {
        Self { designators, run }
    }

    /// The boot style the drive holds, text when it holds none.
    ///
    /// # Errors
    ///
    /// A sentence when the esp is not there or could not be mounted.
    pub fn style(&self) -> Result<Style, String> {
        let esp = self.mount()?;
        let style = Style::read(esp.path());
        esp.unmount()?;
        Ok(style)
    }

    /// Writes the boot style onto the drive.
    ///
    /// # Errors
    ///
    /// A sentence when the esp is not there, could not be mounted, or could not be written.
    pub fn set_style(&self, style: Style) -> Result<(), String> {
        let esp = self.mount()?;
        style.write(esp.path())?;
        esp.unmount()
    }

    /// The file names in the esp's `EFI/Linux`, which are the ukis systemd-boot lists at the
    /// start, each with the boot counter it has left.
    ///
    /// # Errors
    ///
    /// A sentence when the esp is not there, could not be mounted, or has no `EFI/Linux`.
    pub fn ukis(&self) -> Result<Vec<String>, String> {
        let esp = self.mount()?;
        let read = std::fs::read_dir(esp.path().join(LINUX))
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                    .collect()
            })
            .map_err(|e| format!("Could not read the boot partition: {e}"));
        esp.unmount()?;
        read
    }

    /// Mounts the esp under the runtime directory. It is mounted at /boot as well, on an automount
    /// that comes and goes, and the same vfat mounted twice is one file system either way.
    ///
    /// It is mounted for writing even to read, which is not a choice: the kernel refuses a second
    /// mount of a block device whose read only state would differ from the first ("would change RO
    /// state"), and /boot is up for two minutes after anything touches it. A vfat mounted for
    /// writing is written as it is mounted, so in a Ghost boot the methods that would come here
    /// refuse instead, which is how that mode mounts no part of the drive at all (ADR-0084).
    fn mount(&self) -> Result<Mounted, String> {
        let device = self.designators.join(ESP);
        if !device.exists() {
            return Err("The drive this system started from has no boot partition.".into());
        }
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = self
            .run
            .join(format!("esp-{}-{number}", std::process::id()));
        Mounted::new(&device, path, "vfat", None)
    }
}
