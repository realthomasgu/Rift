//! Whether a file is an `AppImage`, and the one program that runs one.
//!
//! An `AppImage` is a program in one file: an ELF whose header carries the letters `AI` and the
//! kind, 1 or 2, where the ABI version and two of the padding bytes sit, with the rest of the app
//! appended after it. The kernel is given those same eleven bytes to recognise one by,
//! so a file a person has made executable runs through `appimage-run` by itself. The program
//! inside asks for the loader and the libraries of an ordinary Linux root, which this system does
//! not have, so `appimage-run` is also the only thing that runs one, and Files and Airlock both
//! read the bytes rather than trust a name.

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// The program that lays out an ordinary Linux root over the store and runs an `AppImage` in it.
pub const RUN: &str = "appimage-run";

/// How many of a file's first bytes say whether it is one.
pub const HEAD: usize = 11;

/// Which kind of `AppImage` these first bytes are, 1 or 2, and `None` for anything else.
#[must_use]
pub fn kind(head: &[u8]) -> Option<u8> {
    let head: &[u8; HEAD] = head.get(..HEAD)?.try_into().ok()?;
    if &head[..4] != b"\x7fELF" || &head[8..10] != b"AI" {
        return None;
    }
    match head[10] {
        kind @ (1 | 2) => Some(kind),
        _ => None,
    }
}

/// Which kind of `AppImage` the file at this path is, read from its own first bytes. `None` for
/// anything else and for a file that cannot be read.
#[must_use]
pub fn of(path: &Path) -> Option<u8> {
    let mut head = [0u8; HEAD];
    File::open(path).ok()?.read_exact(&mut head).ok()?;
    kind(&head)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first bytes of a type 2 `AppImage`, which is what every one made since 2016 is.
    const TYPE2: [u8; 16] = [
        0x7f, b'E', b'L', b'F', 2, 1, 1, 0, b'A', b'I', 2, 0, 0, 0, 0, 0,
    ];

    #[test]
    fn the_bytes_say_which_kind_it_is() {
        assert_eq!(kind(&TYPE2), Some(2));
        let mut old = TYPE2;
        old[10] = 1;
        assert_eq!(kind(&old), Some(1));
        // an ordinary program of the system: the same header without the letters
        let mut plain = TYPE2;
        plain[8] = 0;
        plain[9] = 0;
        plain[10] = 0;
        assert_eq!(kind(&plain), None);
        // a kind nothing here knows how to run is not one either
        let mut later = TYPE2;
        later[10] = 3;
        assert_eq!(kind(&later), None);
        // and neither is a file that is not a program at all, or one too short to tell
        assert_eq!(kind(b"Minutes of the meeting\n"), None);
        assert_eq!(kind(&TYPE2[..10]), None);
        assert_eq!(kind(&[]), None);
    }

    #[test]
    fn a_file_is_read_by_its_own_first_bytes() {
        let folder = std::env::temp_dir().join(format!("librift-appimage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let one = folder.join("Thing.AppImage");
        std::fs::write(&one, TYPE2).unwrap();
        assert_eq!(of(&one), Some(2));
        // a name says nothing: this one is a note somebody renamed
        let lying = folder.join("Notes.AppImage");
        std::fs::write(&lying, "Minutes of the meeting\n").unwrap();
        assert_eq!(of(&lying), None);
        // and one of these with no name of its kind is still one
        let unnamed = folder.join("thing");
        std::fs::write(&unnamed, TYPE2).unwrap();
        assert_eq!(of(&unnamed), Some(2));
        assert_eq!(of(&folder.join("nothing")), None);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
