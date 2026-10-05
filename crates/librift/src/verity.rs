//! The dm-verity superblock at the front of a slot's verity partition: what the tree over that
//! slot's store was made with.
//!
//! An update is published without its verity tree. The tree is a pure function of the store image,
//! the block sizes and the salt, and systemd-repart derives the salt and the superblock uuid from
//! `image.repart.seed`, which is fixed so an image build is reproducible. So every Rift version
//! carries the same salt, and the drive can make the tree for the slot it writes and check the root
//! hash against the signed one (ADR-0089).
//!
//! The parameters are read out of the running slot's own superblock rather than compiled in, so a
//! drive whose build changed the seed still works. The superblock is a plain struct at the front of
//! the partition, little endian.

/// What the first bytes of a verity partition say.
const SIGNATURE: &[u8] = b"verity\0\0";
/// How many bytes of it are read: the struct is 512, and everything named here is in the first 120.
pub const SUPERBLOCK_BYTES: usize = 512;

/// What a slot's verity tree was made with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verity {
    /// The uuid in the superblock, which repart derives from the image's seed.
    pub uuid: String,
    /// The hash the tree is made of, `sha256`.
    pub algorithm: String,
    /// How many bytes of the store one leaf of the tree covers.
    pub data_block: u32,
    /// How many bytes one block of the tree is.
    pub hash_block: u32,
    /// How many data blocks the tree covers, which is the size of the store image, not of the
    /// partition it lies in.
    pub data_blocks: u64,
    /// The salt, as hex, the way veritysetup takes it.
    pub salt: String,
}

impl Verity {
    /// How many bytes of the store the tree covers.
    #[must_use]
    pub fn data_bytes(&self) -> u64 {
        self.data_blocks * u64::from(self.data_block)
    }
}

/// What the verity superblock at the front of a partition says.
///
/// # Errors
///
/// A sentence when the bytes are not a verity superblock, or name a version or sizes this cannot
/// work with.
pub fn read_superblock(bytes: &[u8]) -> Result<Verity, String> {
    let not_one = || "That partition holds no verity tree.".to_string();
    if bytes.len() < 120 || bytes.get(..SIGNATURE.len()) != Some(SIGNATURE) {
        return Err(not_one());
    }
    let four = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap_or_default());
    let eight = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap_or_default());
    let (version, hash_type) = (four(8), four(12));
    if version != 1 {
        return Err(format!(
            "That verity tree is of version {version}, and this one reads version 1."
        ));
    }
    if hash_type != 1 {
        return Err(format!(
            "That verity tree is of hash type {hash_type}, and this one reads type 1."
        ));
    }
    let salt_size = usize::from(u16::from_le_bytes(
        bytes[80..82].try_into().unwrap_or_default(),
    ));
    let salt = bytes
        .get(88..88 + salt_size)
        .filter(|_| salt_size <= 256)
        .ok_or_else(|| format!("That verity tree says its salt is {salt_size} bytes."))?;
    let algorithm: String = bytes[32..64]
        .iter()
        .take_while(|&&byte| byte != 0)
        .map(|&byte| byte as char)
        .collect();
    if algorithm.is_empty() || !algorithm.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(not_one());
    }
    Ok(Verity {
        uuid: uuid(&bytes[16..32]),
        algorithm,
        data_block: four(64),
        hash_block: four(68),
        data_blocks: eight(72),
        salt: hex(salt),
    })
}

/// The 16 bytes of a uuid as text, in the order they lie in: the superblock keeps them the way
/// `uuid_parse` reads them, not the way a partition table does.
fn uuid(bytes: &[u8]) -> String {
    let text = hex(bytes);
    let part = |from: usize, to: usize| text.get(from..to).unwrap_or_default();
    format!(
        "{}-{}-{}-{}-{}",
        part(0, 8),
        part(8, 12),
        part(12, 16),
        part(16, 20),
        part(20, 32)
    )
}

/// Bytes as lower case hex.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A superblock with the parameters Rift's own image carries, measured in probe run
    /// 37310922600.
    fn superblock() -> Vec<u8> {
        let mut bytes = vec![0; SUPERBLOCK_BYTES];
        bytes[..8].copy_from_slice(SIGNATURE);
        bytes[8..12].copy_from_slice(&1_u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&1_u32.to_le_bytes());
        for (at, byte) in [
            0xad, 0x6a, 0x62, 0x64, 0x89, 0x01, 0x4e, 0x9e, 0xa7, 0x5a, 0x7a, 0x1d, 0x70, 0x96,
            0xca, 0x95,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[16 + at] = byte;
        }
        bytes[32..38].copy_from_slice(b"sha256");
        bytes[64..68].copy_from_slice(&4096_u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&4096_u32.to_le_bytes());
        bytes[72..80].copy_from_slice(&1_452_169_u64.to_le_bytes());
        bytes[80..82].copy_from_slice(&32_u16.to_le_bytes());
        for at in 0..32 {
            bytes[88 + at] = 0xa3;
        }
        bytes
    }

    #[test]
    fn the_superblock_says_what_the_tree_was_made_with() {
        let read = read_superblock(&superblock()).unwrap();
        assert_eq!(read.uuid, "ad6a6264-8901-4e9e-a75a-7a1d7096ca95");
        assert_eq!(read.algorithm, "sha256");
        assert_eq!((read.data_block, read.hash_block), (4096, 4096));
        assert_eq!(read.data_blocks, 1_452_169);
        assert_eq!(read.data_bytes(), 5_948_084_224);
        assert_eq!(read.salt, "a3".repeat(32));
    }

    #[test]
    fn bytes_that_are_not_a_superblock_are_refused() {
        assert!(read_superblock(&[]).is_err());
        assert!(read_superblock(&[0; SUPERBLOCK_BYTES]).is_err());
        let short = superblock();
        assert!(read_superblock(&short[..119]).is_err());
        let mut version = superblock();
        version[8] = 2;
        let why = read_superblock(&version).unwrap_err();
        assert!(why.contains("version 2"), "{why}");
        let mut kind = superblock();
        kind[12] = 0;
        assert!(read_superblock(&kind).unwrap_err().contains("hash type 0"));
        let mut salt = superblock();
        salt[80..82].copy_from_slice(&300_u16.to_le_bytes());
        assert!(read_superblock(&salt).unwrap_err().contains("300 bytes"));
        // a superblock whose hash has no name is not one either
        let mut nameless = superblock();
        nameless[32..38].fill(0);
        assert!(read_superblock(&nameless).is_err());
    }
}
