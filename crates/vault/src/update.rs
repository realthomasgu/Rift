//! Writing a new version into the slot that is not running.
//!
//! A version is published as an index per file rather than as the file: the list of chunks it is
//! made of, each named by its hash, with the chunks in one directory beside the versions. So this
//! reads the index, works out which chunks the drive is missing by comparing it with the index of
//! the version in the slot it is running, and hands desync the two: desync takes what it can from
//! the running partition and only the rest from the chunk store, and writes the free slot's store
//! partition directly, with no copy in between (ADR-0089).
//!
//! The verity tree is not published at all. It is a pure function of the store image, the block
//! sizes and the salt, and the salt is the same in every Rift version, so `veritysetup format` on
//! the drive comes to the same tree the build made. The root hash the build published is the proof:
//! the slot's two partitions are named and its uki goes on the esp only once the tree matches it,
//! so a slot that was written wrong is never bootable.
//!
//! `systemd-sysupdate` still installs a version from a directory of whole files, untouched, and
//! `systemd-sysupdate list` is still how the drive reads back what is installed.

use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use librift::disk::partition_node;
use librift::disk::run;
use librift::update::{self, Index, Plan, Slots, Written};
use librift::verity::{self, SUPERBLOCK_BYTES, Verity};

use crate::boot::Esp;
use crate::clone::os_release;
use crate::slots::{drive_table, slot_partitions};

/// Where the ukis are on the esp.
const LINUX: &str = "EFI/Linux";
/// How many boots a new version gets to prove itself, which is what the transfer files give one.
const TRIES: u32 = 3;
/// How many versions the esp keeps a uki for, which is what the transfer files allow.
const UKIS: usize = 2;
/// The chunk sizes both ends work in, in kibibytes: desync's own, and what the build published
/// with.
const CHUNK_SIZES: &str = "16:64:256";
/// The index of the uki of the running version, made while the esp is mounted.
const RUNNING_UKI_INDEX: &str = "running.efi.caibx";

/// Writing the free slot: where the drive names its partitions, what the running system calls
/// itself, where the indexes of installed versions are kept, and where the work happens.
pub struct Updater {
    /// Where udev names the running drive's partitions, `/dev/disk/by-designator`.
    pub designators: PathBuf,
    /// What the running system calls itself, `/etc/os-release`.
    pub release: PathBuf,
    /// The indexes of the versions this drive installed, on persist. The one of the version in the
    /// running slot is what an update seeds from.
    pub indexes: PathBuf,
    /// Where an index made on the spot goes, `/run/vault`.
    pub run: PathBuf,
}

/// Everything an update needs, read off the drive and out of the folder updates come from.
struct Ready {
    /// The image id, `rift`.
    id: String,
    /// The version waiting.
    version: String,
    /// The version running, and the slot it is in.
    running: String,
    running_slot: String,
    /// The uki of the running version on the esp, which the new one is seeded from. Empty when
    /// there is none.
    running_uki: String,
    /// The slot the new version goes into.
    slot: String,
    /// Where updates come from, and the chunks every published version is made of.
    from: PathBuf,
    chunks: PathBuf,
    /// The published index of the new version's store and of its uki, and the root hash of the
    /// verity tree over that store.
    store_index: PathBuf,
    uki_index: PathBuf,
    root_hash: String,
    /// The drive itself, whose table says which partition is which.
    disk: PathBuf,
    /// The store and verity partitions of the slot that is running: what an update reads.
    seed: PathBuf,
    seed_verity: PathBuf,
    /// And of the slot it writes, each with the number it has in the table.
    store: (usize, PathBuf),
    verity: (usize, PathBuf),
    /// How many bytes that store partition is, which is what the new version has to fit in.
    room: u64,
}

impl Updater {
    /// What an update would do, with nothing written: which version is waiting, how much of it
    /// this drive already holds and how much has to be fetched.
    ///
    /// # Errors
    ///
    /// A sentence when nothing is waiting, when updates come from somewhere this version cannot
    /// fetch from, or when the drive could not be read.
    pub fn next(&self, slots: &Slots, esp: &Esp) -> Result<Plan, String> {
        // nothing newer waiting is an answer, not a failure: the plan says what is running and
        // where updates come from, and the page and the command print that
        if slots.newer().is_none() {
            return Ok(Plan::nothing(slots));
        }
        let ready = self.ready(slots)?;
        let seed = self.seed(&ready)?;
        let counted = self.counted(&ready, &seed, esp)?;
        Ok(Plan {
            version: ready.version,
            running: ready.running,
            running_slot: ready.running_slot,
            slot: ready.slot,
            from: ready.from.display().to_string(),
            total: counted.total,
            fetch: counted.fetch,
        })
    }

    /// Writes the version that is waiting into the slot that is not running: its store, the verity
    /// tree over it, the names of the two partitions, and its uki on the esp.
    ///
    /// # Errors
    ///
    /// A sentence at the step that failed. None of them touches the running slot, and the uki that
    /// makes the new slot bootable is written last of all.
    pub fn write(
        &self,
        slots: &Slots,
        esp: &Esp,
        say: &mut impl FnMut(String),
    ) -> Result<Written, String> {
        let ready = self.ready(slots)?;
        let seed = self.seed(&ready)?;
        let counted = self.counted(&ready, &seed, esp)?;
        let slot = update::slot_name(&ready.slot);
        if counted.store > ready.room {
            return Err(format!(
                "Version {} needs {} of store and slot {slot} of this drive holds {}. It was                  written for a smaller version, and nothing was changed.",
                ready.version,
                librift::size(counted.store),
                librift::size(ready.room)
            ));
        }
        say(format!(
            "Writing {} into slot {slot}: {} to fetch, {} from the slot this drive runs.",
            ready.version,
            librift::size(counted.fetch),
            librift::size(counted.total.saturating_sub(counted.fetch))
        ));

        // the store, straight into the partition: what desync finds in the running slot it takes
        // from there and only the rest comes from the chunks
        let stats = run::tool(
            Command::new("desync")
                .arg("extract")
                .args(["--in-place", "--print-stats"])
                .args(["--store".as_ref(), ready.chunks.as_os_str()])
                .arg("--seed")
                .arg(seeded(&seed, &ready.seed))
                .args([&ready.store_index, &ready.store.1]),
        )?;
        println!("vault: desync wrote the store: {}", one_line(&stats));
        say(format!("Slot {slot} holds the store of {}.", ready.version));

        // the tree over it, made here and checked against the hash the build published
        let made = Self::format_verity(&ready, counted.store)?;
        if made != ready.root_hash {
            return Err(format!(
                "The store written into slot {slot} hashes to {made}, and version {} was \
                 published with the root hash {}. Nothing went onto the boot partition, so that \
                 slot is not started.",
                ready.version, ready.root_hash
            ));
        }
        say(format!(
            "Its hash tree matches the root hash of version {}.",
            ready.version
        ));

        // the two partitions carry the uuids the new uki looks them up by, so they are named
        // before the uki that names them goes on
        Self::name_slot(&ready)?;
        let dropped = self.write_uki(&ready, esp)?;
        for old in dropped {
            say(format!("Took {old} off the boot partition."));
        }
        self.keep_index(&ready);
        Ok(Written {
            version: ready.version,
            slot: ready.slot,
            fetched: counted.fetch,
            seeded: counted.total.saturating_sub(counted.fetch),
        })
    }

    /// Everything the update needs, or the sentence that says why there is nothing to do.
    fn ready(&self, slots: &Slots) -> Result<Ready, String> {
        let text = fs::read_to_string(&self.release)
            .map_err(|e| format!("Could not read {}: {e}", self.release.display()))?;
        let (id, _) =
            os_release(&text).ok_or("The running system does not say its image id and version.")?;
        let running_slot = slots
            .running_slot()
            .ok_or(
                "This drive is not running from either of its slots, so there is no slot an \
                 update could safely be written into.",
            )?
            .to_string();
        let slot = update::free_slot(slots)
            .ok_or("This drive has no second slot for an update to go into.")?
            .to_string();
        let from = update::folder(&slots.source)
            .map(PathBuf::from)
            .ok_or_else(|| {
                if slots.source.is_empty() {
                    "Nothing is set up to bring updates in.".to_string()
                } else {
                    format!(
                        "Updates come from {}, and this version installs them from a folder on \
                         this machine only.",
                        slots.source.trim()
                    )
                }
            })?;
        let version = slots
            .newer()
            .ok_or_else(|| {
                format!(
                    "Nothing newer than this drive holds is waiting in {}.",
                    from.display()
                )
            })?
            .to_string();

        let (store_index, uki_index, root_hash) = published(&from, &id, &version)?;
        let (disk, table) = drive_table(&self.designators)?;
        let in_table = slot_partitions(&table)?;
        let numbers = |which: &str| {
            slots
                .slots
                .iter()
                .position(|slot| slot.slot == which)
                .and_then(|at| in_table.get(at).copied())
                .ok_or_else(|| format!("This drive has no slot {}.", update::slot_name(which)))
        };
        let node = |number: usize| PathBuf::from(partition_node(&disk.to_string_lossy(), number));
        let (verity, store) = numbers(&slot)?;
        let (seed_verity, seed_store) = numbers(&running_slot)?;
        let room = table
            .partitions
            .get(store - 1)
            .map(|partition| partition.size * table.sectorsize)
            .unwrap_or_default();
        Ok(Ready {
            id,
            version,
            running: slots.running.clone(),
            running_uki: slots
                .slots
                .iter()
                .find(|slot| slot.slot == running_slot)
                .map(|slot| slot.uki.clone())
                .unwrap_or_default(),
            running_slot,
            slot,
            chunks: from.join(update::CHUNKS),
            from,
            store_index,
            uki_index,
            root_hash,
            seed: node(seed_store),
            seed_verity: node(seed_verity),
            store: (store, node(store)),
            verity: (verity, node(verity)),
            room,
            disk,
        })
    }

    /// The index of the version in the running slot, which an update seeds from.
    ///
    /// In order: the one this drive kept when it installed that version, the one published beside
    /// it where updates come from, or one made here from the partition itself, which is what a
    /// drive that was flashed and never updated has to do.
    fn seed(&self, ready: &Ready) -> Result<PathBuf, String> {
        let kept = self
            .indexes
            .join(format!("{}_{}.store.caibx", ready.id, ready.running));
        if read_index(&kept).is_ok() {
            return Ok(kept);
        }
        if let Some(name) =
            update::store_index_of(&read_names(&ready.from)?, &ready.id, &ready.running)
        {
            let published = ready.from.join(name);
            if read_index(&published).is_ok() {
                return Ok(published);
            }
        }
        fs::create_dir_all(&self.indexes)
            .map_err(|e| format!("Could not make {}: {e}", self.indexes.display()))?;
        println!(
            "vault: reading slot {} to work out what version {} needs",
            update::slot_name(&ready.running_slot),
            ready.version
        );
        make_index(&kept, &ready.seed)?;
        Ok(kept)
    }

    /// How many bytes the new version is and how many of them are not on this drive yet: its store
    /// against the index of the slot that is running, and its uki against the uki of the running
    /// version on the esp.
    fn counted(&self, ready: &Ready, seed: &Path, esp: &Esp) -> Result<Counted, String> {
        let (new, old) = (read_index(&ready.store_index)?, read_index(seed)?);
        let uki = read_index(&ready.uki_index)?;
        let (store, fetch) = (new.size(), new.missing_from(&old));
        Ok(Counted {
            store,
            total: store + uki.size(),
            fetch: fetch + self.uki_missing(ready, &uki, esp)?,
        })
    }

    /// The bytes of the new uki that the uki of the running version cannot give. The index of that
    /// uki stays in the runtime directory, so the extract below seeds from it.
    fn uki_missing(&self, ready: &Ready, uki: &Index, esp: &Esp) -> Result<u64, String> {
        if ready.running_uki.is_empty() {
            return Ok(uki.size());
        }
        let mounted = esp.mounted()?;
        let made = self.index_running_uki(ready, mounted.path());
        mounted.unmount()?;
        made.map(|old| uki.missing_from(&old))
    }

    /// The index of the running version's uki, made from the mounted esp. A uki is 67 MiB, so this
    /// costs a moment and saves about half of the next one.
    fn index_running_uki(&self, ready: &Ready, esp: &Path) -> Result<Index, String> {
        let index = self.run.join(RUNNING_UKI_INDEX);
        let _ = fs::remove_file(&index);
        make_index(&index, &esp.join(LINUX).join(&ready.running_uki))?;
        read_index(&index)
    }

    /// The verity tree over the store just written, into the free slot's verity partition. Returns
    /// the root hash veritysetup came to.
    fn format_verity(ready: &Ready, image: u64) -> Result<String, String> {
        let was = Self::running_verity(ready)?;
        let blocks = image
            .checked_div(u64::from(was.data_block))
            .ok_or("The verity tree of the slot this drive runs says its blocks are 0 bytes.")?;
        let printed = run::tool(
            Command::new("veritysetup")
                .arg("format")
                .args([
                    format!("--data-block-size={}", was.data_block),
                    format!("--hash-block-size={}", was.hash_block),
                    format!("--data-blocks={blocks}"),
                    format!("--hash={}", was.algorithm),
                    format!("--salt={}", was.salt),
                    format!("--uuid={}", was.uuid),
                ])
                .args([&ready.store.1, &ready.verity.1]),
        )?;
        println!("vault: veritysetup said: {}", one_line(&printed));
        printed
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(key, _)| key.trim().eq_ignore_ascii_case("root hash"))
            .and_then(|(_, hash)| update::root_hash(hash))
            .ok_or_else(|| {
                format!(
                    "veritysetup made a tree over slot {} and said no root hash. It printed \
                     {printed:?}.",
                    update::slot_name(&ready.slot)
                )
            })
    }

    /// What the verity tree of the slot this drive runs was made with, read out of its superblock.
    /// Every Rift version has the same salt, so this is the new version's too, and reading it
    /// rather than compiling it in means a drive whose build changed the seed still works.
    fn running_verity(ready: &Ready) -> Result<Verity, String> {
        let mut bytes = vec![0; SUPERBLOCK_BYTES];
        File::open(&ready.seed_verity)
            .and_then(|mut partition| partition.read_exact(&mut bytes))
            .map_err(|e| format!("Could not read {}: {e}", ready.seed_verity.display()))?;
        verity::read_superblock(&bytes)
            .map_err(|why| format!("{why} It is {}.", ready.seed_verity.display()))
    }

    /// Names the two partitions of the slot that was written and gives them the uuids the new
    /// version's uki looks them up by.
    fn name_slot(ready: &Ready) -> Result<(), String> {
        let (store_uuid, verity_uuid) = update::slot_uuids(&ready.root_hash)
            .ok_or("The published root hash is not a root hash.")?;
        for (number, label, uuid) in [
            (
                ready.verity.0,
                format!("store-verity_{}", ready.version),
                verity_uuid,
            ),
            (
                ready.store.0,
                format!("store_{}", ready.version),
                store_uuid,
            ),
        ] {
            for args in run::name_args(&ready.disk, number, &label, &uuid) {
                run::tool(Command::new("sfdisk").args(args))?;
            }
        }
        Ok(())
    }

    /// Puts the new version's uki on the esp with all its tries, and says which older versions'
    /// were dropped to make room. This is the step that makes the slot bootable, so it comes last.
    fn write_uki(&self, ready: &Ready, esp: &Esp) -> Result<Vec<String>, String> {
        let mounted = esp.mounted()?;
        let written = self.put_uki(ready, mounted.path());
        mounted.unmount()?;
        written
    }

    /// The uki onto the mounted esp, and the names of the older versions' that were taken off.
    fn put_uki(&self, ready: &Ready, esp: &Path) -> Result<Vec<String>, String> {
        let linux = esp.join(LINUX);
        fs::create_dir_all(&linux)
            .map_err(|e| format!("Could not make {LINUX} on the boot partition: {e}"))?;
        let name = format!("{}_{}+{TRIES}-0.efi", ready.id, ready.version);
        // written under a name systemd-boot does not read, so a uki that was cut short is never
        // one of the entries it offers
        let part = linux.join(format!("{name}.part"));
        let _ = fs::remove_file(&part);
        let mut extract = Command::new("desync");
        extract
            .arg("extract")
            .args(["--store".as_ref(), ready.chunks.as_os_str()]);
        let running = self.run.join(RUNNING_UKI_INDEX);
        if !ready.running_uki.is_empty() && running.is_file() {
            extract
                .arg("--seed")
                .arg(seeded(&running, &linux.join(&ready.running_uki)));
        }
        run::tool(extract.args([&ready.uki_index, &part]))?;
        fs::rename(&part, linux.join(&name))
            .map_err(|e| format!("Could not put {name} on the boot partition: {e}"))?;

        // the esp keeps a uki for two versions, the way the transfer files have it
        let mut held: Vec<(String, String)> = fs::read_dir(&linux)
            .map_err(|e| format!("Could not read the boot partition: {e}"))?
            .filter_map(|entry| {
                let file = entry.ok()?.file_name().into_string().ok()?;
                Some((uki_version(&file, &ready.id)?, file))
            })
            .collect();
        held.sort_by(|(one, _), (other, _)| update::compare(one, other));
        held.dedup_by(|(one, _), (other, _)| one == other);
        let mut dropped = Vec::new();
        while held.len() > UKIS {
            let (_, file) = held.remove(0);
            fs::remove_file(linux.join(&file))
                .map_err(|e| format!("Could not take {file} off the boot partition: {e}"))?;
            dropped.push(file);
        }
        Ok(dropped)
    }

    /// Keeps the new version's index on persist, so the update after this one seeds from it without
    /// reading a whole partition, and drops the indexes of versions the drive no longer holds.
    fn keep_index(&self, ready: &Ready) {
        let keep: Vec<String> = [&ready.version, &ready.running]
            .iter()
            .map(|version| format!("{}_{version}.store.caibx", ready.id))
            .collect();
        if let Err(e) = fs::create_dir_all(&self.indexes)
            .and_then(|()| fs::copy(&ready.store_index, self.indexes.join(&keep[0])).map(|_| ()))
        {
            println!("vault: could not keep the index of {}: {e}", ready.version);
        }
        for entry in fs::read_dir(&self.indexes).into_iter().flatten().flatten() {
            if !keep.contains(&entry.file_name().to_string_lossy().into_owned()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// What a version published where updates come from: the index of its store, the index of its uki
/// and the root hash of the verity tree over that store.
fn published(from: &Path, id: &str, version: &str) -> Result<(PathBuf, PathBuf, String), String> {
    let names = read_names(from)?;
    let store_index = from.join(update::store_index_of(&names, id, version).ok_or_else(|| {
        format!(
            "Version {version} in {} has no index of its store, so it can only be installed from \
             its whole files, with systemd-sysupdate.",
            from.display()
        )
    })?);
    let uki_index = from.join(update::uki_index(id, version));
    if !uki_index.is_file() {
        return Err(format!(
            "Version {version} in {} has no index of its uki.",
            from.display()
        ));
    }
    let hash_file = from.join(update::root_hash_file(id, version));
    let root_hash = fs::read_to_string(&hash_file)
        .map_err(|e| format!("Could not read {}: {e}", hash_file.display()))
        .and_then(|text| {
            update::root_hash(&text)
                .ok_or_else(|| format!("{} does not hold a root hash.", hash_file.display()))
        })?;
    Ok((store_index, uki_index, root_hash))
}

/// What the two indexes come to: the store image's own size, the whole version's, and the bytes of
/// it this drive does not have.
struct Counted {
    store: u64,
    total: u64,
    fetch: u64,
}

/// An index of a file or a partition, written to `index`. A partition is longer than the image in
/// it, and the chunks of the image are the same either way, since a chunk's boundaries are decided
/// by the bytes before it.
fn make_index(index: &Path, blob: &Path) -> Result<(), String> {
    run::tool(
        Command::new("desync")
            .arg("make")
            .args(["--chunk-size", CHUNK_SIZES])
            .args([index, blob]),
    )
    .map(|_| ())
}

/// A seed for desync: the index, and the file or partition it is the index of.
fn seeded(index: &Path, blob: &Path) -> String {
    format!("{}:{}", index.display(), blob.display())
}

/// The index in a file.
fn read_index(path: &Path) -> Result<Index, String> {
    let bytes = fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    Index::read(&bytes).map_err(|why| format!("{why} It is {}.", path.display()))
}

/// The file names in the folder updates come from.
fn read_names(folder: &Path) -> Result<Vec<String>, String> {
    let read =
        fs::read_dir(folder).map_err(|e| format!("Could not read {}: {e}", folder.display()))?;
    Ok(read
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect())
}

/// What a program printed, as one line for the journal.
fn one_line(printed: &str) -> String {
    printed.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The version a uki on the esp is of, with or without its boot counter.
fn uki_version(name: &str, id: &str) -> Option<String> {
    let rest = name.strip_prefix(&format!("{id}_"))?.strip_suffix(".efi")?;
    let version = rest.split_once('+').map_or(rest, |(version, _)| version);
    (!version.is_empty() && update::names_uki(name, id, version)).then(|| version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uki_on_the_esp_says_which_version_it_is_of() {
        assert_eq!(
            uki_version("rift_0.2.0.efi", "rift").as_deref(),
            Some("0.2.0")
        );
        assert_eq!(
            uki_version("rift_0.2.0+3-0.efi", "rift").as_deref(),
            Some("0.2.0")
        );
        assert_eq!(
            uki_version("rift_0.2.0+1.efi", "rift").as_deref(),
            Some("0.2.0")
        );
        for other in [
            "rift_0.2.0.efi.bak",
            "rift_0.2.0+a.efi",
            "other_0.2.0.efi",
            "rift_.efi",
            "SHA256SUMS",
        ] {
            assert_eq!(uki_version(other, "rift"), None, "{other}");
        }
    }

    #[test]
    fn a_seed_is_an_index_and_the_partition_it_is_of() {
        assert_eq!(
            seeded(
                Path::new("/var/lib/rift/vault/rift_0.1.0.store.caibx"),
                Path::new("/dev/nvme0n1p3")
            ),
            "/var/lib/rift/vault/rift_0.1.0.store.caibx:/dev/nvme0n1p3"
        );
        assert_eq!(
            one_line("Root hash:\t 6ab4\nHash device size:  45 MiB\n"),
            "Root hash: 6ab4 Hash device size: 45 MiB"
        );
    }
}
