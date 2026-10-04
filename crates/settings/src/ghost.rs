//! What a page says in Ghost mode, where a row would be.
//!
//! In a Ghost boot persist stays locked and nothing of the drive is mounted, so a page that reads
//! or writes what the drive keeps has nothing to show and nothing to write (ADR-0082). Every one of
//! them says the mode in the place the row would be, in the same two shapes: a row of its own with
//! the one sentence that names what cannot be done, or a line under a group whose setting does work
//! and goes when the machine does (ADR-0084).
//!
//! The words come from `librift::ghost`, which is the one place they are written and the one place
//! the kernel command line is read. A sentence is built here and not in a page's state, because a
//! label and a note are borrowed and only the value of a row may be owned.

use iced::Element;
use librift::ghost;

use crate::ai::said;
use crate::theme::Colors;
use crate::widgets::fact;

/// Whether this session is a Ghost one, which is what every page asks before it reads.
#[must_use]
pub fn on() -> bool {
    ghost::on()
}

/// The one row a group has in place of what it cannot read or write: the name of the row at the
/// left, and the sentence that says what cannot be done and why filling the rest of it.
pub fn row<'a, M: 'a>(look: Colors, label: &'a str, what: &str) -> Element<'a, M> {
    fact(look, label, ghost::cannot(what))
}

/// The line under a group whose setting is written under home: it holds for this login and is on no
/// drive afterwards, which is worth saying where a page would otherwise say nothing.
pub fn only_now<'a, M: 'a>(look: Colors, what: &str) -> Element<'a, M> {
    said(look, &ghost::not_kept(what))
}
