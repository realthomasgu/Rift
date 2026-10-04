//! The Appearance page: the wallpaper, dark or light, the accent, the interface text size, the
//! gaps between windows, the corner radius of a window, the terminal colours, the terminal greeting
//! and the boot style. Every change but the last is written to the owner's files and handed to the
//! apps, the compositor and the shell at once; the boot style is on the drive's esp, which only
//! root can write, so Vault does it and the next boot draws it.

use std::thread;

use iced::futures::channel::oneshot;
use iced::widget::{column, container, text};
use iced::{Border, Element, Fill, Length, Task, Theme};
use librift::appearance::{
    GAPS_MOST, RADIUS_MOST, Scheme, TEXT_LEAST, TEXT_MOST, TEXT_STEP, Theme as Mode,
};
use librift::boot::Style;

use crate::ghost;
use crate::theme::{Colors, hex};
use crate::ui::{Message, Settings};
use crate::widgets::{
    GAP, MONO, TEXT_SIZE, choice, group, heading, note, setting, steps, swatches, switch,
};

/// How wide and tall the two letters of a terminal colour scheme are drawn in it.
const SAMPLE: (f32, f32) = (44.0, 22.0);

/// Ask Vault how this drive starts, on a thread of its own: the system bus takes a moment, and the
/// window opens without waiting for it.
pub fn ask_vault() -> Task<Message> {
    answered(librift::vault::boot_style)
}

/// Write a boot style through Vault, on a thread of its own, and say what it answered.
pub fn write_style(style: Style) -> Task<Message> {
    answered(move || librift::vault::set_boot_style(style).map(|()| style))
}

/// What Vault says, from a thread, as the message the window takes it in.
fn answered(ask: impl FnOnce() -> Result<Style, String> + Send + 'static) -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(ask());
    });
    Task::perform(receiver, |said| {
        Message::BootStyle(said.unwrap_or_else(|_| Err("Vault did not answer.".to_string())))
    })
}

/// The page.
pub fn view(state: &Settings, look: Colors) -> Element<'_, Message> {
    let mut page = column![
        themes(state, look),
        accents(state, look),
        wallpapers(state, look),
        text_size(state, look),
        windows(state, look),
        terminal(state, look),
        boot(state, look),
    ]
    .spacing(GAP)
    .width(Fill);
    if let Some(why) = &state.problem {
        page = page.push(text(why).size(TEXT_SIZE).color(look.error));
    }
    page.into()
}

/// The wallpapers: every photograph Rift ships, then the flat colours.
fn wallpapers(state: &Settings, look: Colors) -> Element<'_, Message> {
    let current = &state.look.wallpaper;
    let rows: Vec<Element<'_, Message>> = state
        .choices
        .iter()
        .enumerate()
        .map(|(at, one)| {
            choice(
                look,
                &one.title,
                (!one.credit.is_empty()).then_some(one.credit.as_str()),
                None,
                &one.wallpaper == current,
                Message::Wallpaper(at),
            )
        })
        .collect();
    let mut part = column![heading(look, "Wallpaper")].spacing(8);
    // a picture of the owner's own, set from the rift command, is not in the list
    if state.choices.iter().all(|one| &one.wallpaper != current) {
        part = part.push(note(
            look,
            "The desktop has a picture of your own. Choosing one below takes its place.",
        ));
    }
    part.push(group(look, rows)).into()
}

/// Dark or light.
fn themes(state: &Settings, look: Colors) -> Element<'_, Message> {
    let rows = [Mode::Dark, Mode::Light]
        .into_iter()
        .map(|mode| {
            choice(
                look,
                mode.label(),
                None,
                None,
                mode == state.look.theme,
                Message::Mode(mode),
            )
        })
        .collect();
    column![heading(look, "Theme"), group(look, rows)]
        .spacing(8)
        .into()
}

/// The nine accents, in a box of their own.
fn accents(state: &Settings, look: Colors) -> Element<'_, Message> {
    let inside = container(swatches(
        look,
        state.look.theme,
        state.look.accent,
        Message::Accent,
    ))
    .width(Fill)
    .padding([10, 12])
    .style(move |_: &Theme| container::Style {
        background: Some(look.view.into()),
        border: Border {
            color: look.line,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    });
    column![heading(look, "Accent colour"), inside]
        .spacing(8)
        .into()
}

/// The size of the text of the interface, which the apps and the shell both follow.
fn text_size(state: &Settings, look: Colors) -> Element<'_, Message> {
    let rows = vec![setting(
        look,
        "Interface text size",
        Some("Apps, the bar, the dock and the menus are all drawn at this size."),
        steps(
            look,
            TEXT_LEAST..=TEXT_MOST,
            TEXT_STEP,
            state.look.text,
            "%",
            Message::Text,
            Message::Wrote,
        ),
    )];
    column![heading(look, "Text"), group(look, rows)]
        .spacing(8)
        .into()
}

/// The gap between windows and the corner radius of one.
fn windows(state: &Settings, look: Colors) -> Element<'_, Message> {
    let rows = vec![
        setting(
            look,
            "Gap between windows",
            None,
            steps(
                look,
                0..=GAPS_MOST,
                1,
                state.look.gaps,
                "px",
                Message::Gaps,
                Message::Wrote,
            ),
        ),
        setting(
            look,
            "Corner radius",
            None,
            steps(
                look,
                0..=RADIUS_MOST,
                1,
                state.look.radius,
                "px",
                Message::Radius,
                Message::Wrote,
            ),
        ),
    ];
    column![heading(look, "Windows"), group(look, rows)]
        .spacing(8)
        .into()
}

/// The terminal colours and the greeting.
fn terminal(state: &Settings, look: Colors) -> Element<'_, Message> {
    let mut rows: Vec<Element<'_, Message>> = Scheme::ALL
        .into_iter()
        .map(|scheme| {
            choice(
                look,
                scheme.label(),
                None,
                Some(sample(scheme, look)),
                scheme == state.look.terminal,
                Message::Terminal(scheme),
            )
        })
        .collect();
    rows.push(setting(
        look,
        "Greeting",
        Some("The first shell of a session shows the logo and what the machine is."),
        switch(look, state.greeting, Message::Greeting),
    ));
    column![heading(look, "Terminal"), group(look, rows)]
        .spacing(8)
        .into()
}

/// How the drive looks while it starts. It is the one setting of this page that is not in home and
/// does not take effect where it stands: plymouth draws the passphrase prompt before persist is
/// open, so the word lives on the esp and the next boot is the one that draws it.
fn boot(state: &Settings, look: Colors) -> Element<'_, Message> {
    if ghost::on() {
        // the word is on the esp, and a Ghost boot mounts no part of the drive and writes nothing
        // to it, so there is nothing to read and nothing to choose (ADR-0084)
        return column![
            heading(look, "Boot"),
            group(look, vec![ghost::row(look, "Boot", NO_BOOT_STYLE)]),
        ]
        .spacing(8)
        .into();
    }
    let chosen = match &state.boot {
        Some(Ok(style)) => Some(*style),
        _ => None,
    };
    let rows = Style::ALL
        .into_iter()
        .map(|style| {
            choice(
                look,
                style.label(),
                Some(style.note()),
                None,
                Some(style) == chosen,
                Message::Boot(style),
            )
        })
        .collect();
    let under: Element<'_, Message> = match &state.boot {
        None => note(look, "Asking Vault how this drive starts."),
        Some(Ok(_)) => note(look, "The next start of this drive draws it."),
        Some(Err(why)) => text(why).size(TEXT_SIZE).color(look.error).into(),
    };
    column![heading(look, "Boot"), group(look, rows), under]
        .spacing(8)
        .into()
}

/// What a Ghost boot cannot do with the word on the esp.
const NO_BOOT_STYLE: &str = "How the next boot looks cannot be read or changed";

/// Two letters in a colour scheme, drawn on it: what a terminal in it looks like.
fn sample<'a>(scheme: Scheme, look: Colors) -> Element<'a, Message> {
    let colour = hex(scheme.background());
    container(
        text("Ab")
            .size(TEXT_SIZE)
            .font(MONO)
            .color(hex(scheme.foreground())),
    )
    .center_x(Length::Fixed(SAMPLE.0))
    .center_y(Length::Fixed(SAMPLE.1))
    .style(move |_: &Theme| container::Style {
        background: Some(colour.into()),
        border: Border {
            color: look.line,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}
