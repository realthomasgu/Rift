//! Whether this boot is a Ghost one, and the words every surface says about it.
//!
//! Ghost mode is the drive's second boot entry: persist stays locked, home and var are in memory,
//! nothing of the drive is mounted and nothing the session does is on it when the machine goes off
//! (ADR-0082). One word on the kernel command line is the whole of how it is told apart, and this
//! is the one place that word is read. The shell, Settings, the rift command, Welcome and the
//! services all ask here, so they all agree and none of them reads /proc for itself.
//!
//! It is read once, the first time anything asks: a kernel command line cannot change while the
//! machine is on.

use std::fs;
use std::sync::OnceLock;

/// The word on the kernel command line that makes a boot a Ghost one. The ghost profile's
/// `.cmdline` carries it and the ordinary one does not.
pub const WORD: &str = "rift.ghost";

/// Where the kernel's own command line is.
pub const CMDLINE: &str = "/proc/cmdline";

/// The name of the mode, as the bar, the pages and the rift command print it. Sentence case, two
/// words, the same two everywhere.
pub const NAME: &str = "Ghost mode";

/// What it means, in one sentence. The notification at login and the guide both say it.
pub const SENTENCE: &str =
    "The drive stays locked and this session is in memory. Nothing you save is kept.";

/// The short of it, for a row that has no room for the sentence.
pub const SHORT: &str = "Nothing is kept";

/// Whether this boot is a Ghost one.
///
/// Read from the kernel command line once and remembered. A machine that is not running Rift at all
/// has no such word, so this is false everywhere else, which is what every caller wants.
#[must_use]
pub fn on() -> bool {
    *ON.get_or_init(|| names(&fs::read_to_string(CMDLINE).unwrap_or_default()))
}

/// The answer, read once and remembered.
static ON: OnceLock<bool> = OnceLock::new();

/// Makes this process answer as though it had booted in Ghost mode, which is how a page is drawn
/// for a picture on a machine that cannot boot a Rift drive at all. It has to be called before
/// anything else asks, and it is only in a debug build: the image is built in release, so nothing
/// on the drive carries it and the kernel command line stays the only way a boot is a Ghost one.
#[cfg(debug_assertions)]
pub fn pretend() {
    let _ = ON.set(true);
}

/// Whether a kernel command line names the word. The word itself, not a prefix of a longer one:
/// `rift.ghostly` is not it. A value after it is allowed, the way systemd's own
/// `ConditionKernelCommandLine` allows one, so `rift.ghost=1` counts too.
#[must_use]
pub fn names(cmdline: &str) -> bool {
    cmdline.split_whitespace().any(|word| {
        word == WORD
            || word
                .strip_prefix(WORD)
                .is_some_and(|rest| rest.starts_with('='))
    })
}

/// One sentence for something that cannot be done in Ghost mode, naming the mode and why. `what` is
/// what was asked for, as a sentence with no full stop: "The drive cannot be updated".
///
/// Every command and every page that writes to the drive says it this way, so the reason reads the
/// same wherever it comes from.
#[must_use]
pub fn cannot(what: &str) -> String {
    format!("{what} in {NAME}: persist stays locked and nothing is written to the drive.")
}

/// One sentence for something that does work in Ghost mode and goes when the machine does. `what`
/// is what can be done, as a sentence with no full stop: "A size can be set for this session".
///
/// A setting kept under home is one of these: home is memory here, so it holds for the login and
/// is not on the drive afterwards. A page or a command says this where it would otherwise say
/// nothing and leave a person thinking a choice had been written down.
#[must_use]
pub fn not_kept(what: &str) -> String {
    format!(
        "{what}. In {NAME} nothing is kept: everything here is in memory until the machine goes off."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_names_the_word_or_it_does_not() {
        assert!(names("init=/init rift.ghost rd.luks=0"));
        assert!(names("rift.ghost"));
        assert!(names("quiet rift.ghost=1 splash"));
        assert!(!names(""));
        assert!(!names("init=/init quiet splash usrhash=abc"));
        // a longer word that starts with it is not it
        assert!(!names("rift.ghostly"));
        assert!(!names("norift.ghost"));
        // and neither is it inside another word's value
        assert!(!names("systemd.setenv=WORDS=rift.ghost"));
    }

    #[test]
    fn the_words_read_the_same_everywhere() {
        assert_eq!(NAME, "Ghost mode");
        assert!(SENTENCE.ends_with('.'));
        assert!(!SHORT.ends_with('.'));
        let said = cannot("The drive cannot be updated");
        assert!(
            said.starts_with("The drive cannot be updated in Ghost mode:"),
            "{said}"
        );
        assert!(said.ends_with('.'));
        let for_now = not_kept("A size can be set for this session");
        assert!(
            for_now
                .starts_with("A size can be set for this session. In Ghost mode nothing is kept:"),
            "{for_now}"
        );
        assert!(for_now.ends_with('.'));
        for sentence in [NAME, SENTENCE, SHORT, &said, &for_now] {
            assert!(sentence.is_ascii(), "{sentence}");
        }
    }

    #[test]
    fn a_machine_that_is_not_in_it_says_so() {
        // the Mac and every runner have no such word, and the image's ordinary boot has none either
        assert_eq!(
            on(),
            names(&fs::read_to_string(CMDLINE).unwrap_or_default())
        );
    }
}
