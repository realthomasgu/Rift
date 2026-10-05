//! The two slots of the drive: which version is in which, what each version's uki on the esp has
//! left, where updates come from, and what one is made of.
//!
//! A slot is a store partition and the verity tree beside it. A new version goes into the slot that
//! is not running and its uki goes on the esp with three tries. systemd-boot takes a try off the
//! file name each time it starts that uki, and systemd-bless-boot drops the counter once the boot
//! is good, so three boots that never come up start the version in the other slot again. Vault
//! reads all of this as root and the Updates page draws it.
//!
//! A version is published as an index per file rather than as the file: the list of chunks it is
//! made of, each named by its hash, with the chunks themselves in one directory beside the
//! versions (ADR-0089). [`Index`] reads one, and [`Plan`] is what two of them say about an update
//! before it starts: how much of the new version this drive already holds in the slot it is
//! running, and how much has to be fetched.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// One slot of the drive.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Slot {
    /// Which slot it is, `a` or `b`.
    pub slot: String,
    /// The version its partitions hold, empty when the slot is empty.
    pub version: String,
    /// The name of that version's uki on the esp, empty when there is none.
    pub uki: String,
}

impl Slot {
    /// How many more times systemd-boot will start this version before it gives up on it. `None`
    /// when the uki has no counter: a boot of it was marked good, or it never had one.
    #[must_use]
    pub fn tries(&self) -> Option<u32> {
        counter(&self.uki).map(|(left, _)| left)
    }

    /// Whether the slot holds a version.
    #[must_use]
    pub fn filled(&self) -> bool {
        !self.version.is_empty()
    }
}

/// What the drive holds and what is waiting to go onto it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Slots {
    /// The version running now.
    pub running: String,
    /// The slots, slot a first.
    pub slots: Vec<Slot>,
    /// Where updates come from, the way the transfer files have it.
    pub source: String,
    /// The versions in that folder, oldest first. Empty when updates come from somewhere this
    /// machine cannot look in.
    pub waiting: Vec<String>,
}

impl Slots {
    /// The slot the running version is in, when one of them holds it.
    #[must_use]
    pub fn running_slot(&self) -> Option<&str> {
        self.slots
            .iter()
            .find(|slot| slot.version == self.running && !self.running.is_empty())
            .map(|slot| slot.slot.as_str())
    }

    /// What Vault answers on the bus.
    #[must_use]
    pub fn answer(&self) -> Answer {
        (
            self.running.clone(),
            self.slots
                .iter()
                .map(|slot| (slot.slot.clone(), slot.version.clone(), slot.uki.clone()))
                .collect(),
            self.source.clone(),
            self.waiting.clone(),
        )
    }

    /// The picture in what Vault answered.
    #[must_use]
    pub fn from_answer(answer: Answer) -> Self {
        let (running, slots, source, waiting) = answer;
        Self {
            running,
            slots: slots
                .into_iter()
                .map(|(slot, version, uki)| Slot { slot, version, uki })
                .collect(),
            source,
            waiting,
        }
    }

    /// The newest version waiting where updates come from, when it is newer than every version on
    /// the drive.
    #[must_use]
    pub fn newer(&self) -> Option<&str> {
        let newest = self.waiting.iter().max_by(|a, b| compare(a, b))?;
        let on_the_drive = self
            .slots
            .iter()
            .filter(|slot| slot.filled())
            .map(|slot| slot.version.as_str())
            .chain(Some(self.running.as_str()).filter(|running| !running.is_empty()))
            .max_by(|a, b| compare(a, b));
        match on_the_drive {
            Some(held) if compare(newest, held) != Ordering::Greater => None,
            _ => Some(newest.as_str()),
        }
    }
}

/// What Vault's `Slots` method carries on the bus: the version running, the slot, version and uki
/// name of each slot, where updates come from, and the versions waiting there.
pub type Answer = (String, Vec<(String, String, String)>, String, Vec<String>);

/// The boot counter in a uki's name: `rift_0.2.0+3-0.efi` has three tries left and none done,
/// `rift_0.2.0+2.efi` has two left, and `rift_0.2.0.efi` has no counter at all.
#[must_use]
pub fn counter(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_suffix(".efi")?;
    counted(rest.rsplit_once('+')?.1)
}

/// The two numbers of a counter, `3-0` or `2`, and nothing else.
fn counted(text: &str) -> Option<(u32, u32)> {
    let (left, done) = text.split_once('-').unwrap_or((text, "0"));
    Some((number(left)?, number(done)?))
}

/// A whole number of digits and nothing else.
fn number(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Whether `name` is the uki of `version` of the image `id`, with or without a boot counter.
#[must_use]
pub fn names_uki(name: &str, id: &str, version: &str) -> bool {
    let Some(rest) = name
        .strip_prefix(&format!("{id}_{version}"))
        .and_then(|rest| rest.strip_suffix(".efi"))
    else {
        return false;
    };
    rest.is_empty()
        || rest
            .strip_prefix('+')
            .is_some_and(|left| counted(left).is_some())
}

/// The uki of `version` among the file names in the esp's `EFI/Linux`: `rift_0.2.0.efi`, or one
/// with a boot counter, `rift_0.2.0+2.efi` or `rift_0.2.0+1-2.efi`. The one without a counter
/// comes first, since a version that has booted well keeps that name.
#[must_use]
pub fn uki_of(names: &[String], id: &str, version: &str) -> Option<String> {
    let mut found: Vec<&String> = names
        .iter()
        .filter(|name| names_uki(name, id, version))
        .collect();
    found.sort_by_key(|name| (name.len(), name.as_str()));
    found.first().map(|name| (*name).clone())
}

/// The version an update file in the source folder is of: `rift_0.3.0.efi` is version 0.3.0. The
/// store and its verity tree have the partition uuid in the name as well, so only the uki counts.
#[must_use]
pub fn version_of<'a>(name: &'a str, id: &str) -> Option<&'a str> {
    let version = name.strip_prefix(&format!("{id}_"))?.strip_suffix(".efi")?;
    (!version.is_empty() && !version.contains('_') && counter(name).is_none()).then_some(version)
}

/// Two versions in the order the drive puts them in: by their numbers where they are numbers, and
/// as text where they are not.
#[must_use]
pub fn compare(one: &str, other: &str) -> Ordering {
    let mut ours = one.split('.');
    let mut theirs = other.split('.');
    loop {
        match (ours.next(), theirs.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ours), Some(theirs)) => {
                let order = match (number(ours), number(theirs)) {
                    (Some(ours), Some(theirs)) => ours.cmp(&theirs),
                    _ => ours.cmp(theirs),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// The `Path=` of the `[Source]` section of a systemd-sysupdate transfer file, which says where
/// the version's files come from.
#[must_use]
pub fn source_of(text: &str) -> Option<&str> {
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = section.trim().eq_ignore_ascii_case("source");
        } else if inside
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("path")
            && !value.trim().is_empty()
        {
            return Some(value.trim());
        }
    }
    None
}

/// The folder a source names when it is one on this machine: `file:///var/lib/rift/updates/` is
/// `/var/lib/rift/updates`. `None` when updates come from somewhere else.
#[must_use]
pub fn folder(source: &str) -> Option<&str> {
    let path = source.trim().strip_prefix("file://")?;
    let trimmed = path.trim_end_matches('/');
    Some(if trimmed.is_empty() { "/" } else { trimmed })
}

/// Where updates come from, in the words the page shows: the folder of a source on this machine,
/// or the source as it stands.
#[must_use]
pub fn where_from(source: &str) -> &str {
    folder(source).unwrap_or_else(|| source.trim())
}

/// Where the chunks every published version is made of are kept, beside the versions themselves.
/// One directory serves them all: a chunk two versions share is stored once (ADR-0089).
pub const CHUNKS: &str = "chunks";

/// What a version's store index is called where updates come from. The uuid its store partition
/// gets is in the name, the way the whole file had it.
#[must_use]
pub fn store_index(id: &str, version: &str, uuid: &str) -> String {
    format!("{id}_{version}_{uuid}.store.caibx")
}

/// What a version's uki index is called.
#[must_use]
pub fn uki_index(id: &str, version: &str) -> String {
    format!("{id}_{version}.efi.caibx")
}

/// What the file holding the root hash of a version's verity tree is called. The tree itself is
/// not published: the drive makes it and this says what it has to come to (ADR-0089).
#[must_use]
pub fn root_hash_file(id: &str, version: &str) -> String {
    format!("{id}_{version}.store.roothash")
}

/// The store index of `version` among the file names where updates come from, whatever uuid is in
/// its name.
#[must_use]
pub fn store_index_of(names: &[String], id: &str, version: &str) -> Option<String> {
    let start = format!("{id}_{version}_");
    names
        .iter()
        .find(|name| {
            name.starts_with(&start)
                && name.ends_with(".store.caibx")
                && !name[start.len()..].contains('_')
        })
        .cloned()
}

/// A verity root hash, when the text is one: the 64 hex digits the build publishes, lower case,
/// with nothing else on the line.
#[must_use]
pub fn root_hash(text: &str) -> Option<String> {
    let hash = text.trim();
    (hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| hash.to_ascii_lowercase())
}

/// The uuids the two partitions of a slot carry: the first half of the root hash of its verity
/// tree for the store, the second for the tree.
///
/// A version's uki names its root hash on its command line, and the initrd looks the two
/// partitions up by those uuids, which is why a slot Rift writes itself has to carry them. They
/// are also what the published file names have always said.
#[must_use]
pub fn slot_uuids(hash: &str) -> Option<(String, String)> {
    let hash = root_hash(hash)?;
    let uuid = |half: &str| {
        let part = |from: usize, to: usize| &half[from..to];
        format!(
            "{}-{}-{}-{}-{}",
            part(0, 8),
            part(8, 12),
            part(12, 16),
            part(16, 20),
            part(20, 32)
        )
    };
    Some((uuid(&hash[..32]), uuid(&hash[32..])))
}

/// One chunk of a file: the hash it is named by in the chunk store, and how many bytes of the file
/// it stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    /// The hash, as the chunk store names it.
    pub id: [u8; 32],
    /// How many bytes of the file this chunk is.
    pub bytes: u64,
}

/// The index of a published file: the chunks it is made of, in the order they lie in it.
///
/// This is casync's index format, which desync writes: a header, a table of one 40 byte record per
/// chunk holding the offset the chunk ends at and its hash, and a tail. Rift reads it to work out
/// what an update will cost before it starts one; desync does the fetching and the writing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Index {
    /// The chunks, in order.
    pub chunks: Vec<Chunk>,
}

/// The first eight bytes of an index say it is one.
const INDEX_MARKER: u64 = 0x9682_4d9c_7b12_9ff9;
/// And the eight after its header say the table of chunks follows.
const TABLE_MARKER: u64 = 0xe75b_9e11_2f17_417d;
/// The last eight bytes of the file.
const TAIL_MARKER: u64 = 0x4b4f_050e_5549_ecd1;
/// The index header, and the size of one record of the table.
const HEADER_BYTES: usize = 64;
const RECORD_BYTES: usize = 40;

impl Index {
    /// The index in these bytes.
    ///
    /// # Errors
    ///
    /// A sentence when the bytes are not an index, or name chunks that do not follow one another.
    pub fn read(bytes: &[u8]) -> Result<Self, String> {
        let number = |at: usize| {
            bytes
                .get(at..at + 8)
                .and_then(|eight| eight.try_into().ok())
                .map(u64::from_le_bytes)
        };
        let not_one = || "That is not the index of a published file.".to_string();
        if number(0) != Some(48)
            || number(8) != Some(INDEX_MARKER)
            || number(48) != Some(u64::MAX)
            || number(56) != Some(TABLE_MARKER)
        {
            return Err(not_one());
        }
        let table = bytes.get(HEADER_BYTES..).ok_or_else(not_one)?;
        if table.len() < RECORD_BYTES || table.len() % RECORD_BYTES != 0 {
            return Err(not_one());
        }
        let (records, tail) = table.split_at(table.len() - RECORD_BYTES);
        if number(bytes.len() - 8) != Some(TAIL_MARKER) || tail[..8] != [0; 8] {
            return Err(not_one());
        }
        let mut chunks = Vec::with_capacity(records.len() / RECORD_BYTES);
        let mut at = 0;
        for record in records.chunks_exact(RECORD_BYTES) {
            let ends = u64::from_le_bytes(record[..8].try_into().unwrap_or_default());
            let bytes = ends
                .checked_sub(at)
                .filter(|&bytes| bytes > 0)
                .ok_or("That index names chunks that do not follow one another.".to_string())?;
            let mut id = [0; 32];
            id.copy_from_slice(&record[8..]);
            chunks.push(Chunk { id, bytes });
            at = ends;
        }
        Ok(Self { chunks })
    }

    /// How many bytes the file it stands for is.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.chunks.iter().map(|chunk| chunk.bytes).sum()
    }

    /// The bytes of the chunks this index names that `seed` names nowhere, which are the ones that
    /// have to be fetched. A chunk that appears more than once is counted once, since it is
    /// fetched once.
    #[must_use]
    pub fn missing_from(&self, seed: &Self) -> u64 {
        let held: HashSet<[u8; 32]> = seed.chunks.iter().map(|chunk| chunk.id).collect();
        let missing: HashMap<[u8; 32], u64> = self
            .chunks
            .iter()
            .filter(|chunk| !held.contains(&chunk.id))
            .map(|chunk| (chunk.id, chunk.bytes))
            .collect();
        missing.values().sum()
    }
}

/// What an update would do, worked out before anything is written: which version is waiting, how
/// much of it this drive already holds in the slot it is running, and how much has to be fetched.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// The version waiting.
    pub version: String,
    /// The version running now.
    pub running: String,
    /// The slot it is running from, `a` or `b`.
    pub running_slot: String,
    /// The slot the new version goes into, the one that is not running.
    pub slot: String,
    /// Where it comes from, in the words a person reads.
    pub from: String,
    /// How many bytes the version is: its store and its uki together.
    pub total: u64,
    /// How many of those bytes are not on this drive yet.
    pub fetch: u64,
}

impl Plan {
    /// The plan when nothing newer than the drive holds is waiting: what is running, and where
    /// updates come from, with no version.
    #[must_use]
    pub fn nothing(slots: &Slots) -> Self {
        Self {
            running: slots.running.clone(),
            running_slot: slots.running_slot().unwrap_or_default().to_string(),
            slot: free_slot(slots).unwrap_or_default().to_string(),
            from: where_from(&slots.source).to_string(),
            ..Self::default()
        }
    }

    /// Whether there is a version to install.
    #[must_use]
    pub fn waiting(&self) -> bool {
        !self.version.is_empty()
    }

    /// How many bytes of the new version this drive already holds.
    #[must_use]
    pub fn have(&self) -> u64 {
        self.total.saturating_sub(self.fetch)
    }

    /// What `rift update` prints about an update before it writes anything. The Updates page says
    /// the middle line of it under its button.
    #[must_use]
    pub fn said(&self) -> Vec<String> {
        let running = format!(
            "This drive runs {} from slot {}.",
            self.running,
            slot_name(&self.running_slot)
        );
        if !self.waiting() {
            return vec![
                running,
                format!("Nothing newer is waiting in {}.", self.from),
            ];
        }
        vec![
            running,
            format!("Version {} is waiting in {}.", self.version, self.from),
            format!("It is {}", self.line()),
            format!(
                "It goes into slot {}, and {} stays where it is.",
                slot_name(&self.slot),
                self.running
            ),
        ]
    }

    /// What an update costs, as the value of a row on the Updates page and as the middle of the
    /// sentence the command prints.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "{}, and this drive already has {} of it, so {} has to be fetched.",
            crate::size(self.total),
            crate::size(self.have()),
            crate::size(self.fetch)
        )
    }

    /// What Vault answers on the bus.
    #[must_use]
    pub fn answer(&self) -> Waiting {
        (
            self.version.clone(),
            self.running.clone(),
            self.running_slot.clone(),
            self.slot.clone(),
            self.from.clone(),
            self.total,
            self.fetch,
        )
    }

    /// The plan in what Vault answered.
    #[must_use]
    pub fn from_answer(answer: Waiting) -> Self {
        let (version, running, running_slot, slot, from, total, fetch) = answer;
        Self {
            version,
            running,
            running_slot,
            slot,
            from,
            total,
            fetch,
        }
    }
}

/// What Vault's `NextVersion` method carries on the bus: the version waiting, the version running
/// and the slot it is in, the slot the new one goes into, where it comes from, how many bytes it
/// is and how many of them have to be fetched. The version is empty when nothing is waiting.
pub type Waiting = (String, String, String, String, String, u64, u64);

/// What an update wrote.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Written {
    /// The version that is in the slot now.
    pub version: String,
    /// The slot it went into.
    pub slot: String,
    /// The bytes that came from where updates come from.
    pub fetched: u64,
    /// The bytes that came from the slot this drive is running.
    pub seeded: u64,
}

impl Written {
    /// What `rift update` prints once the slot is written.
    #[must_use]
    pub fn said(&self) -> Vec<String> {
        vec![
            format!(
                "Version {} is in slot {} now, with {} fetched and {} taken from the slot this \
                 drive runs.",
                self.version,
                slot_name(&self.slot),
                crate::size(self.fetched),
                crate::size(self.seeded)
            ),
            "It has three boots to prove itself. Restart when you are ready.".to_string(),
        ]
    }

    /// What Vault answers on the bus.
    #[must_use]
    pub fn answer(&self) -> Installed {
        (
            self.version.clone(),
            self.slot.clone(),
            self.fetched,
            self.seeded,
        )
    }

    /// What an update wrote, in what Vault answered.
    #[must_use]
    pub fn from_answer(answer: Installed) -> Self {
        let (version, slot, fetched, seeded) = answer;
        Self {
            version,
            slot,
            fetched,
            seeded,
        }
    }
}

/// What Vault's `Update` method carries on the bus: the version it installed, the slot it went
/// into, the bytes it fetched and the bytes it took from the drive.
pub type Installed = (String, String, u64, u64);

/// A slot's name as a person reads it: `A` or `B`.
#[must_use]
pub fn slot_name(slot: &str) -> String {
    slot.to_ascii_uppercase()
}

/// The slot that is not running, which is where an update goes. `None` when the running version is
/// in neither of them, since then there is no slot that is safe to write.
#[must_use]
pub fn free_slot(slots: &Slots) -> Option<&str> {
    let running = slots.running_slot()?;
    slots
        .slots
        .iter()
        .map(|slot| slot.slot.as_str())
        .find(|&slot| slot != running)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(of: &[&str]) -> Vec<String> {
        of.iter().map(|name| (*name).to_string()).collect()
    }

    /// A real index, written by desync over 200 KiB of random bytes: the header, seven chunks and
    /// the tail, 384 bytes in all. The sizes below are what desync chose.
    const INDEX: &str = concat!(
        "3000000000000000f99f127b9c4d829600000000000000a000400000000000000080000000000000",
        "0000010000000000ffffffffffffffff7d41172f119e5be7d74b000000000000bbad7c92f1751136",
        "26fb9990dfd07cd17d342e154fb38b7e4a4ffc454a5149f870a200000000000074160adf11fc7979",
        "f0a4fefac3ba59b8652ad27e2366c655600b52fcc0685ae7ed1a010000000000c137f6d31059fe86",
        "da41553493863982603b1f351809603b43357326aa037f88608401000000000082369158175b6054",
        "c2e9e58c75ad68a939b502f83be1fe6f414148dddfb45267e218020000000000b21a5fdd99115d4e",
        "4d40df569ad6dbb64be49884ec456a87699d87943e73ea41c77f020000000000eecd192eb5e647eb",
        "969aadf65e689db18e4c62154b7c335a58f3cc3f1d357d550020030000000000c9356e20735af48e",
        "cdefef61dc29ef39bd5a8b627f3b9362a3a5068a67bc4915000000000000000000000000000000",
        "0030000000000000005001000000000000d1ec49550e054f4b",
    );

    fn from_hex(text: &str) -> Vec<u8> {
        let digit = |byte: &u8| u8::try_from((*byte as char).to_digit(16).unwrap_or_default());
        text.as_bytes()
            .chunks(2)
            .map(|pair| {
                let (high, low) = (digit(&pair[0]).unwrap(), digit(&pair[1]).unwrap());
                high * 16 + low
            })
            .collect()
    }

    /// An index of chunks of these sizes, the first byte of each chunk's hash telling it apart.
    fn index(chunks: &[(u8, u64)]) -> Index {
        Index {
            chunks: chunks
                .iter()
                .map(|&(mark, bytes)| {
                    let mut id = [0; 32];
                    id[0] = mark;
                    Chunk { id, bytes }
                })
                .collect(),
        }
    }

    #[test]
    fn an_index_says_which_chunks_a_file_is_made_of() {
        let read = Index::read(&from_hex(INDEX)).unwrap();
        assert_eq!(
            read.chunks
                .iter()
                .map(|chunk| chunk.bytes)
                .collect::<Vec<_>>(),
            [19415, 22169, 30845, 26995, 38018, 26341, 41017]
        );
        assert_eq!(read.size(), 204_800);
        assert_eq!(read.chunks[0].id[..4], [0xbb, 0xad, 0x7c, 0x92]);
    }

    #[test]
    fn bytes_that_are_not_an_index_are_refused() {
        let whole = from_hex(INDEX);
        assert!(Index::read(&[]).is_err());
        assert!(Index::read(&whole[..whole.len() - 1]).is_err());
        assert!(Index::read(&whole[8..]).is_err());
        // the header and the tail without a single chunk
        assert!(Index::read(&whole[..64]).is_err());
        let mut tail = whole.clone();
        let last = tail.len() - 1;
        tail[last] = 0;
        assert!(Index::read(&tail).is_err());
        // a chunk that ends before the one before it
        let mut backwards = whole.clone();
        backwards[104..112].copy_from_slice(&1_u64.to_le_bytes());
        assert!(Index::read(&backwards).is_err());
    }

    #[test]
    fn an_update_fetches_the_chunks_the_drive_does_not_have() {
        let old = index(&[(1, 100), (2, 200), (3, 300)]);
        let new = index(&[(1, 100), (4, 400), (3, 300), (5, 500)]);
        assert_eq!(new.size(), 1300);
        assert_eq!(new.missing_from(&old), 900);
        // a chunk the new version names twice is fetched once
        let twice = index(&[(4, 400), (4, 400), (1, 100)]);
        assert_eq!(twice.size(), 900);
        assert_eq!(twice.missing_from(&old), 400);
        // nothing to seed from, and nothing missing
        assert_eq!(new.missing_from(&Index::default()), 1300);
        assert_eq!(old.missing_from(&old), 0);
    }

    #[test]
    fn the_files_of_a_version_are_named_after_it() {
        assert_eq!(
            store_index("rift", "0.2.0", "6ab4281a-4ab2-22c4-1971-d2ec8da372ee"),
            "rift_0.2.0_6ab4281a-4ab2-22c4-1971-d2ec8da372ee.store.caibx"
        );
        assert_eq!(uki_index("rift", "0.2.0"), "rift_0.2.0.efi.caibx");
        assert_eq!(root_hash_file("rift", "0.2.0"), "rift_0.2.0.store.roothash");
        let published = names(&[
            "SHA256SUMS",
            "rift_0.2.0.efi",
            "rift_0.2.0.efi.caibx",
            "rift_0.2.0.store.roothash",
            "rift_0.2.0_6ab4281a-4ab2-22c4-1971-d2ec8da372ee.store.caibx",
            "rift_0.3.0_8e4b1c2d-0000-0000-0000-000000000000.store.caibx",
        ]);
        assert_eq!(
            store_index_of(&published, "rift", "0.2.0").as_deref(),
            Some("rift_0.2.0_6ab4281a-4ab2-22c4-1971-d2ec8da372ee.store.caibx")
        );
        assert_eq!(store_index_of(&published, "rift", "0.9.0"), None);
        // the version in the name is the whole version, not the start of another
        assert_eq!(store_index_of(&published, "rift", "0.2"), None);
        // and a published index of a version says which version it is of
        assert_eq!(version_of("rift_0.2.0.efi.caibx", "rift"), None);
        assert_eq!(version_of("rift_0.2.0.store.roothash", "rift"), None);
    }

    #[test]
    fn the_two_partitions_of_a_slot_are_named_by_the_root_hash() {
        // run 37284246792 published these two names for this root hash
        let hash = "6ab4281a4ab222c41971d2ec8da372eed3ff922985886a2304d0006fff85d0f6";
        assert_eq!(
            slot_uuids(hash),
            Some((
                "6ab4281a-4ab2-22c4-1971-d2ec8da372ee".to_string(),
                "d3ff9229-8588-6a23-04d0-006fff85d0f6".to_string()
            ))
        );
        assert_eq!(root_hash(&format!("{hash}\n")).as_deref(), Some(hash));
        assert_eq!(root_hash(&hash.to_ascii_uppercase()).as_deref(), Some(hash));
        for other in ["", "nothing", &hash[..63], &format!("{hash}0")] {
            assert_eq!(root_hash(other), None, "{other}");
            assert_eq!(slot_uuids(other), None, "{other}");
        }
    }

    #[test]
    fn a_plan_says_what_an_update_costs_before_it_starts() {
        let plan = Plan {
            version: "0.2.0".to_string(),
            running: "0.1.0".to_string(),
            running_slot: "a".to_string(),
            slot: "b".to_string(),
            from: "/var/lib/rift/updates".to_string(),
            total: 6_015_943_552,
            fetch: 521_248_768,
        };
        assert_eq!(plan.have(), 5_494_694_784);
        assert_eq!(
            plan.said(),
            [
                "This drive runs 0.1.0 from slot A.",
                "Version 0.2.0 is waiting in /var/lib/rift/updates.",
                "It is 5.6 GiB, and this drive already has 5.1 GiB of it, so 498 MiB has to be \
                 fetched.",
                "It goes into slot B, and 0.1.0 stays where it is.",
            ]
        );
        assert_eq!(Plan::from_answer(plan.answer()), plan);
        assert!(plan.waiting());
        // and with nothing waiting, what is running and where it would come from
        let slots = Slots {
            running: "0.1.0".to_string(),
            slots: vec![
                Slot {
                    slot: "a".to_string(),
                    version: "0.1.0".to_string(),
                    uki: "rift_0.1.0.efi".to_string(),
                },
                Slot {
                    slot: "b".to_string(),
                    ..Slot::default()
                },
            ],
            source: "file:///var/lib/rift/updates/".to_string(),
            waiting: Vec::new(),
        };
        let nothing = Plan::nothing(&slots);
        assert!(!nothing.waiting());
        assert_eq!(
            nothing.said(),
            [
                "This drive runs 0.1.0 from slot A.",
                "Nothing newer is waiting in /var/lib/rift/updates.",
            ]
        );
        assert_eq!(nothing.slot, "b");
        let written = Written {
            version: "0.2.0".to_string(),
            slot: "b".to_string(),
            fetched: 521_248_768,
            seeded: 5_494_694_784,
        };
        assert_eq!(
            written.said()[0],
            "Version 0.2.0 is in slot B now, with 498 MiB fetched and 5.1 GiB taken from the slot \
             this drive runs."
        );
        assert_eq!(Written::from_answer(written.answer()), written);
    }

    #[test]
    fn an_update_goes_into_the_slot_that_is_not_running() {
        let mut slots = Slots {
            running: "0.1.0".to_string(),
            slots: vec![
                Slot {
                    slot: "a".to_string(),
                    version: "0.1.0".to_string(),
                    uki: "rift_0.1.0.efi".to_string(),
                },
                Slot {
                    slot: "b".to_string(),
                    ..Slot::default()
                },
            ],
            source: String::new(),
            waiting: Vec::new(),
        };
        assert_eq!(free_slot(&slots), Some("b"));
        slots.running = "0.2.0".to_string();
        slots.slots[1].version = "0.2.0".to_string();
        assert_eq!(free_slot(&slots), Some("a"));
        // a version no slot holds leaves no slot that is safe to write
        slots.running = "0.9.0".to_string();
        assert_eq!(free_slot(&slots), None);
    }

    #[test]
    fn a_uki_carries_the_tries_it_has_left() {
        assert_eq!(counter("rift_0.2.0+3-0.efi"), Some((3, 0)));
        assert_eq!(counter("rift_0.2.0+1-2.efi"), Some((1, 2)));
        assert_eq!(counter("rift_0.2.0+2.efi"), Some((2, 0)));
        assert_eq!(counter("rift_0.2.0.efi"), None);
        assert_eq!(counter("rift_0.2.0+.efi"), None);
        assert_eq!(counter("rift_0.2.0+a-1.efi"), None);
        assert_eq!(counter("rift_0.2.0+1-.efi"), None);
        assert_eq!(counter("rift_0.2.0+3-0.efi.bak"), None);
    }

    #[test]
    fn a_uki_of_another_version_is_never_this_version() {
        assert_eq!(
            uki_of(
                &names(&["rift_0.2.0.efi", "rift_0.3.0+0-3.efi"]),
                "rift",
                "0.2.0"
            )
            .as_deref(),
            Some("rift_0.2.0.efi")
        );
        assert_eq!(uki_of(&names(&["rift_0.10.0.efi"]), "rift", "0.1"), None);
        assert_eq!(
            uki_of(&names(&["rift_0.1.0+2+3.efi"]), "rift", "0.1.0"),
            None
        );
    }

    #[test]
    fn a_slot_says_what_is_in_it() {
        let blessed = Slot {
            slot: "a".to_string(),
            version: "0.1.0".to_string(),
            uki: "rift_0.1.0.efi".to_string(),
        };
        assert!(blessed.filled());
        assert_eq!(blessed.tries(), None);
        let counted = Slot {
            uki: "rift_0.2.0+3-0.efi".to_string(),
            ..blessed.clone()
        };
        assert_eq!(counted.tries(), Some(3));
        assert!(!Slot::default().filled());
        assert_eq!(Slot::default().tries(), None);
    }

    #[test]
    fn the_uki_of_a_version_is_found_by_its_name() {
        assert_eq!(
            uki_of(
                &names(&["rift_0.2.0.efi", "rift_0.1.0.efi"]),
                "rift",
                "0.1.0"
            )
            .as_deref(),
            Some("rift_0.1.0.efi")
        );
        assert_eq!(
            uki_of(&names(&["rift_0.1.0+2-1.efi"]), "rift", "0.1.0").as_deref(),
            Some("rift_0.1.0+2-1.efi")
        );
        assert_eq!(
            uki_of(&names(&["rift_0.1.0+3.efi"]), "rift", "0.1.0").as_deref(),
            Some("rift_0.1.0+3.efi")
        );
        // the one without a counter is the one a good boot left behind
        assert_eq!(
            uki_of(
                &names(&["rift_0.1.0+3-0.efi", "rift_0.1.0.efi"]),
                "rift",
                "0.1.0"
            )
            .as_deref(),
            Some("rift_0.1.0.efi")
        );
        for other in [
            "rift_0.1.0.1.efi",
            "rift_0.1.0+.efi",
            "rift_0.1.0+a-1.efi",
            "rift_0.1.0+1-.efi",
            "rift_0.1.0.efi.bak",
            "other_0.1.0.efi",
        ] {
            assert_eq!(uki_of(&names(&[other]), "rift", "0.1.0"), None, "{other}");
        }
    }

    #[test]
    fn an_update_file_says_which_version_it_is_of() {
        assert_eq!(version_of("rift_0.3.0.efi", "rift"), Some("0.3.0"));
        for other in [
            "SHA256SUMS",
            "rift_0.3.0_8e4b1c2d.store.zst",
            "rift_0.3.0+3-0.efi",
            "other_0.3.0.efi",
            "rift_.efi",
        ] {
            assert_eq!(version_of(other, "rift"), None, "{other}");
        }
    }

    #[test]
    fn versions_are_ordered_by_their_numbers() {
        assert_eq!(compare("0.1.0", "0.2.0"), Ordering::Less);
        assert_eq!(compare("0.10.0", "0.9.0"), Ordering::Greater);
        assert_eq!(compare("0.2.0", "0.2.0"), Ordering::Equal);
        assert_eq!(compare("0.2", "0.2.0"), Ordering::Less);
        assert_eq!(compare("0.2.0-rc1", "0.2.0"), Ordering::Greater);
    }

    #[test]
    fn the_source_comes_out_of_the_transfer_file() {
        let transfer = "[Transfer]\nProtectVersion=%A\n\n[Source]\nType=url-file\n\
                        Path=file:///var/lib/rift/updates/\nMatchPattern=rift_@v.efi\n\n\
                        [Target]\nType=regular-file\nPath=/EFI/Linux\n";
        assert_eq!(source_of(transfer), Some("file:///var/lib/rift/updates/"));
        assert_eq!(
            folder("file:///var/lib/rift/updates/"),
            Some("/var/lib/rift/updates")
        );
        assert_eq!(
            where_from("file:///var/lib/rift/updates/"),
            "/var/lib/rift/updates"
        );
        // a target path is not the source, and a file with no source says nothing
        assert_eq!(source_of("[Target]\nPath=/EFI/Linux\n"), None);
        assert_eq!(source_of(""), None);
        assert_eq!(folder("https://rift.example/updates/"), None);
        assert_eq!(
            where_from("https://rift.example/updates/ "),
            "https://rift.example/updates/"
        );
    }

    #[test]
    fn the_drive_says_what_is_running_and_what_is_waiting() {
        let slots = Slots {
            running: "0.1.0".to_string(),
            slots: vec![
                Slot {
                    slot: "a".to_string(),
                    version: "0.1.0".to_string(),
                    uki: "rift_0.1.0.efi".to_string(),
                },
                Slot {
                    slot: "b".to_string(),
                    version: "0.2.0".to_string(),
                    uki: "rift_0.2.0+3-0.efi".to_string(),
                },
            ],
            source: "file:///var/lib/rift/updates/".to_string(),
            waiting: vec!["0.2.0".to_string()],
        };
        assert_eq!(slots.running_slot(), Some("a"));
        assert_eq!(Slots::from_answer(slots.answer()), slots);
        // what is waiting is already on the drive
        assert_eq!(slots.newer(), None);
        let newer = Slots {
            waiting: vec!["0.2.0".to_string(), "0.3.0".to_string()],
            ..slots.clone()
        };
        assert_eq!(newer.newer(), Some("0.3.0"));
        let empty_b = Slots {
            slots: vec![
                slots.slots[0].clone(),
                Slot {
                    slot: "b".to_string(),
                    ..Slot::default()
                },
            ],
            ..slots.clone()
        };
        assert_eq!(empty_b.newer(), Some("0.2.0"));
        assert_eq!(empty_b.slots[1].tries(), None);
        // a version no slot holds is running from nowhere the drive knows
        let stray = Slots {
            running: "0.9.0".to_string(),
            ..slots
        };
        assert_eq!(stray.running_slot(), None);
        assert_eq!(Slots::default().running_slot(), None);
        assert_eq!(Slots::default().newer(), None);
    }
}
