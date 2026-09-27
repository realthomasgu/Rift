//! The Owner page: the name the lock screen greets the owner by, the password it asks for, how the
//! owner gets to the desktop, whether the apps that were open come back at the next login, and what
//! opens the drive besides its passphrase, which `unlocking` draws.
//!
//! Vault keeps the name and the password on persist and answers the owner and root alone, so the
//! page asks it as it comes up and after each change, on a thread of its own. Nothing else changes
//! either while the page is up, so it does not ask again on a timer.

use std::thread;

use iced::futures::channel::oneshot;
use iced::widget::{column, row, text};
use iced::{Center, Element, Fill, Task};
use librift::owner::{self as account, Owner};

use crate::ai::said;
use crate::theme::Colors;
use crate::ui::{Message, Settings};
use crate::widgets::{
    GAP, TEXT_SIZE, action, fact, field, focus, group, heading, note, setting, switch,
};

/// The field the name is typed into.
pub const NAME_FIELD: &str = "owner-name";
/// The three the password is.
const CURRENT_FIELD: &str = "owner-current";
const NEW_FIELD: &str = "owner-new";
const AGAIN_FIELD: &str = "owner-again";

/// The names `rift-settings --set` takes for this page: `owner-name <name>`, and `owner-password
/// <current> <new>`, two words.
pub const NAMES: [&str; 3] = ["owner-name", "owner-password", "session-restore"];

/// What is typed on the page, and whether a change is on its way to Vault.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Form {
    /// The name, once its field has been typed into. Until then the field holds the owner's name.
    pub name: Option<String>,
    /// The three password fields, while they are open.
    pub password: Option<Passwords>,
    /// Whether a change is on its way, which holds the buttons until Vault answers.
    pub busy: bool,
}

/// What the three password fields hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Passwords {
    /// The password the owner has now.
    pub current: String,
    /// The one they want.
    pub new: String,
    /// The one they want, typed again.
    pub again: String,
}

/// One of the three password fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The password the owner has now.
    Current,
    /// The new one.
    New,
    /// The new one again.
    Again,
}

/// What the owner asked for on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// The name's field was typed into.
    Name(String),
    /// Keep the name in the field.
    SaveName,
    /// Open the password fields, or close them without a change.
    Password(bool),
    /// One of the password fields was typed into.
    Typed(Field, String),
    /// Change the password to the new one in the fields.
    ChangePassword,
    /// How a change went. The page asks Vault again after it.
    Done(Result<(), String>),
}

/// Ask Vault about the owner, on a thread of its own.
pub fn read() -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(account::read());
    });
    Task::perform(receiver, |answered| {
        Message::Owner(answered.unwrap_or_else(|_| Err(STOPPED.to_string())))
    })
}

/// Do what the owner asked. A change goes to Vault on a thread of its own, and when it has
/// answered the page asks again, so it shows what Vault kept rather than what was typed.
pub fn asked(state: &mut Settings, asked: Asked) -> Task<Message> {
    let form = &mut state.signing;
    match asked {
        Asked::Name(typed) => {
            form.name = Some(typed);
            state.problem = None;
        }
        Asked::SaveName => return save_name(state),
        Asked::Password(open) => {
            form.password = open.then(Passwords::default);
            state.problem = None;
            if open {
                return focus(CURRENT_FIELD);
            }
        }
        Asked::Typed(which, typed) => {
            if let Some(typing) = form.password.as_mut() {
                match which {
                    Field::Current => typing.current = typed,
                    Field::New => typing.new = typed,
                    Field::Again => typing.again = typed,
                }
                state.problem = None;
            }
        }
        Asked::ChangePassword => return change_password(state),
        Asked::Done(done) => {
            form.busy = false;
            match done {
                Ok(()) => {
                    form.name = None;
                    form.password = None;
                    state.problem = None;
                }
                Err(why) => state.problem = Some(why),
            }
            return read();
        }
    }
    Task::none()
}

/// Give the owner the name in the field, when it is one.
fn save_name(state: &mut Settings) -> Task<Message> {
    let Some(name) = state.signing.name.clone() else {
        return Task::none();
    };
    if state.signing.busy {
        return Task::none();
    }
    if let Some(why) = account::name_problem(&name) {
        state.problem = Some(why.to_string());
        return Task::none();
    }
    state.problem = None;
    state.signing.busy = true;
    to_vault(move || account::set_name(&name))
}

/// Give the owner the new password in the fields, when it was typed the same twice.
fn change_password(state: &mut Settings) -> Task<Message> {
    let Some(typed) = state.signing.password.clone() else {
        return Task::none();
    };
    if state.signing.busy {
        return Task::none();
    }
    let problem = if typed.current.is_empty() {
        Some("Type the current password.")
    } else if typed.new != typed.again {
        Some("The two new passwords are not the same.")
    } else {
        account::password_problem(&typed.new)
    };
    if let Some(why) = problem {
        state.problem = Some(why.to_string());
        return Task::none();
    }
    state.problem = None;
    state.signing.busy = true;
    to_vault(move || account::set_password(&typed.current, &typed.new))
}

/// Ask Vault for a change on a thread of its own, and say how it went.
fn to_vault(change: impl FnOnce() -> Result<(), String> + Send + 'static) -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(change());
    });
    Task::perform(receiver, |done| {
        Message::Owning(Asked::Done(
            done.unwrap_or_else(|_| Err(STOPPED.to_string())),
        ))
    })
}

/// What `rift-settings --set` asks of this page, the way typing into it and pressing it would:
/// `owner-name <name>` types the name and keeps it, and `owner-password <current> <new>` opens the
/// password fields, types the current password and the new one twice, and changes it. The steps
/// go in that order, which a batch does not promise.
pub fn named(name: &str, value: &str) -> Task<Message> {
    if name == "session-restore" {
        return Task::done(Message::Restore(!value.trim().eq_ignore_ascii_case("off")));
    }
    let steps = match name {
        "owner-name" => vec![Asked::Name(value.trim().to_string()), Asked::SaveName],
        "owner-password" => {
            let Some((current, new)) = value.trim().split_once(' ') else {
                return Task::none();
            };
            let new = new.trim().to_string();
            vec![
                Asked::Password(true),
                Asked::Typed(Field::Current, current.to_string()),
                Asked::Typed(Field::New, new.clone()),
                Asked::Typed(Field::Again, new),
                Asked::ChangePassword,
            ]
        }
        _ => return Task::none(),
    };
    steps
        .into_iter()
        .map(|asked| Task::done(Message::Owning(asked)))
        .reduce(Task::chain)
        .unwrap_or_else(Task::none)
}

/// The lines `rift-settings --state` prints, once Vault has answered: the account, the name, and
/// whether the password is still the image's or one the owner chose. `owner-user none` is Vault not
/// answering.
#[must_use]
pub fn state(state: &Settings) -> Vec<String> {
    match &state.owner {
        None => Vec::new(),
        Some(Err(_)) => vec!["owner-user none".to_string()],
        Some(Ok(owner)) => vec![
            format!("owner-user {}", owner.user),
            format!("owner-name {}", owner.name),
            format!(
                "owner-password {}",
                if owner.image_password { "image" } else { "own" }
            ),
        ],
    }
}

/// The page.
pub fn view(state: &Settings, look: Colors) -> Element<'_, Message> {
    let answered = match &state.owner {
        None => return note(look, "Asking Vault about the owner."),
        Some(answered) => answered.as_ref(),
    };
    let mut page = column![
        the_account(state, look, answered),
        the_password(state, look, answered),
        column![
            heading(look, "Logging in"),
            group(
                look,
                vec![
                    setting(look, "Automatic login", Some(AUTOMATIC), said(look, "On")),
                    setting(
                        look,
                        "Bring the session back",
                        Some(RESTORE),
                        switch(look, state.restore, Message::Restore),
                    ),
                ]
            ),
        ]
        .spacing(8),
        crate::unlocking::view(state, look),
    ]
    .spacing(GAP)
    .width(Fill);
    if let Some(why) = &state.problem {
        page = page.push(text(why).size(TEXT_SIZE).color(look.error));
    }
    page.into()
}

/// The name with its field and Save, the account's own name, and that it is an administrator.
fn the_account<'a>(
    state: &'a Settings,
    look: Colors,
    answered: Result<&'a Owner, &'a String>,
) -> Element<'a, Message> {
    let rows = match answered {
        Err(why) => vec![fact(look, "Name", why.clone())],
        Ok(owner) => {
            let typed = state.signing.name.as_deref().unwrap_or(&owner.name);
            let changed = state
                .signing
                .name
                .as_deref()
                .is_some_and(|name| !name.trim().is_empty() && name.trim() != owner.name);
            let save = (changed && !state.signing.busy).then_some(Message::Owning(Asked::SaveName));
            vec![
                setting(
                    look,
                    "Name",
                    Some(NAME),
                    row![
                        field(
                            look,
                            "Name",
                            typed,
                            false,
                            NAME_FIELD,
                            |typed| Message::Owning(Asked::Name(typed)),
                            Message::Owning(Asked::SaveName),
                        ),
                        action(look, "Save", save),
                    ]
                    .spacing(8)
                    .align_y(Center),
                ),
                setting(look, "Account name", Some(ACCOUNT), said(look, &owner.user)),
                setting(
                    look,
                    "Administrator",
                    Some(ADMINISTRATOR),
                    said(look, "Yes"),
                ),
            ]
        }
    };
    column![heading(look, "Account"), group(look, rows)]
        .spacing(8)
        .into()
}

/// The password: a button that opens the three fields, or the fields and the buttons under them.
fn the_password<'a>(
    state: &'a Settings,
    look: Colors,
    answered: Result<&'a Owner, &'a String>,
) -> Element<'a, Message> {
    let busy = state.signing.busy;
    let rows = match (answered, &state.signing.password) {
        (Err(why), _) => vec![fact(look, "Password", why.clone())],
        (Ok(owner), None) => vec![setting(
            look,
            "Password",
            owner.image_password.then_some(IMAGE_PASSWORD),
            action(
                look,
                "Change password",
                (!busy).then_some(Message::Owning(Asked::Password(true))),
            ),
        )],
        (Ok(_), Some(typed)) => {
            let secret = |label: &'a str, value: &'a str, id: &'static str, which: Field| {
                setting(
                    look,
                    label,
                    None,
                    field(
                        look,
                        label,
                        value,
                        true,
                        id,
                        move |typed| Message::Owning(Asked::Typed(which, typed)),
                        Message::Owning(Asked::ChangePassword),
                    ),
                )
            };
            let ready = !busy
                && !typed.current.is_empty()
                && !typed.new.is_empty()
                && !typed.again.is_empty();
            vec![
                secret(
                    "Current password",
                    &typed.current,
                    CURRENT_FIELD,
                    Field::Current,
                ),
                secret("New password", &typed.new, NEW_FIELD, Field::New),
                secret(
                    "New password again",
                    &typed.again,
                    AGAIN_FIELD,
                    Field::Again,
                ),
                setting(
                    look,
                    if busy { "Changing the password." } else { HINT },
                    None,
                    row![
                        action(
                            look,
                            "Cancel",
                            (!busy).then_some(Message::Owning(Asked::Password(false)))
                        ),
                        action(
                            look,
                            "Change password",
                            ready.then_some(Message::Owning(Asked::ChangePassword))
                        ),
                    ]
                    .spacing(8),
                ),
            ]
        }
    };
    column![
        heading(look, "Password"),
        group(look, rows),
        note(look, PASSWORD)
    ]
    .spacing(8)
    .into()
}

/// What a thread that stopped before it answered says.
const STOPPED: &str = "It stopped before it finished.";
/// Under the name.
const NAME: &str = "The lock screen shows it above the password.";
/// Under the account's own name.
const ACCOUNT: &str = "The name of the home folder, which cannot be changed.";
/// Under Administrator.
const ADMINISTRATOR: &str = "sudo runs a command as root without asking for the password.";
/// Under the password while it is the image's.
const IMAGE_PASSWORD: &str = "It is still rift, the password every drive starts with.";
/// Beside the buttons under the password fields.
const HINT: &str = "Type the current password, then the new one twice.";
/// Under the password.
const PASSWORD: &str = "The lock screen asks for this password. The passphrase asked for when \
                        the machine starts belongs to the drive, and a new password leaves it as \
                        it is.";
/// Under automatic login.
const AUTOMATIC: &str = "The drive's passphrase is asked for when the machine starts, and the \
                         desktop opens after it without the password.";
/// Under bringing the session back.
const RESTORE: &str = "At the first login after a boot, the apps that were open are opened again, \
                       each on the workspace it was on and in the column it stood in. An app this \
                       machine does not have is passed over.";

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(image_password: bool) -> Settings {
        let mut state = Settings::bare();
        state.owner = Some(Ok(Owner {
            user: "rift".to_string(),
            name: "Rift owner".to_string(),
            image_password,
        }));
        state
    }

    #[test]
    fn the_state_says_the_account_the_name_and_the_password() {
        assert!(state(&Settings::bare()).is_empty());
        assert_eq!(
            state(&settings(true)),
            [
                "owner-user rift",
                "owner-name Rift owner",
                "owner-password image"
            ]
        );
        assert_eq!(
            state(&settings(false)).last().map(String::as_str),
            Some("owner-password own")
        );
        let mut unanswered = Settings::bare();
        unanswered.owner = Some(Err("Vault is not running.".to_string()));
        assert_eq!(state(&unanswered), ["owner-user none"]);
    }

    #[test]
    fn a_name_that_cannot_be_kept_is_said_and_not_sent() {
        let mut state = settings(true);
        let _ = asked(&mut state, Asked::SaveName);
        assert!(!state.signing.busy, "nothing typed, nothing to keep");
        let _ = asked(&mut state, Asked::Name("Taylor, Sam".into()));
        let _ = asked(&mut state, Asked::SaveName);
        assert!(!state.signing.busy);
        assert_eq!(
            state.problem.as_deref(),
            Some("A name cannot have a colon, a comma or a line break in it.")
        );
        // typing again takes the sentence away
        let _ = asked(&mut state, Asked::Name("Sam Taylor".into()));
        assert_eq!(state.problem, None);
    }

    #[test]
    fn the_new_password_has_to_be_typed_the_same_twice() {
        let mut state = settings(true);
        let _ = asked(&mut state, Asked::Password(true));
        for (which, typed) in [
            (Field::Current, "rift"),
            (Field::New, "one"),
            (Field::Again, "two"),
        ] {
            let _ = asked(&mut state, Asked::Typed(which, typed.into()));
        }
        let _ = asked(&mut state, Asked::ChangePassword);
        assert!(!state.signing.busy);
        assert_eq!(
            state.problem.as_deref(),
            Some("The two new passwords are not the same.")
        );
        // closing the fields forgets what was typed
        let _ = asked(&mut state, Asked::Password(false));
        assert_eq!(state.signing.password, None);
        let _ = asked(&mut state, Asked::Password(true));
        assert_eq!(state.signing.password, Some(Passwords::default()));
    }

    #[test]
    fn a_change_that_went_through_clears_the_form() {
        let mut state = settings(true);
        state.signing = Form {
            name: Some("Sam Taylor".into()),
            password: Some(Passwords::default()),
            busy: true,
        };
        let _ = asked(&mut state, Asked::Done(Ok(())));
        assert_eq!(state.signing, Form::default());
        state.signing.busy = true;
        let _ = asked(
            &mut state,
            Asked::Done(Err("The current password is incorrect.".into())),
        );
        assert!(!state.signing.busy);
        assert_eq!(
            state.problem.as_deref(),
            Some("The current password is incorrect.")
        );
    }

    #[test]
    fn the_sentences_are_sentences() {
        for sentence in [
            STOPPED,
            NAME,
            ACCOUNT,
            ADMINISTRATOR,
            IMAGE_PASSWORD,
            HINT,
            PASSWORD,
            AUTOMATIC,
            RESTORE,
        ] {
            assert!(sentence.ends_with('.'), "{sentence}");
            assert!(sentence.is_ascii(), "{sentence}");
        }
    }
}
