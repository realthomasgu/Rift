//! The top bar, laid out like Tails: the Applications button at the left, the clock in the middle
//! of the screen, which opens the clock menu, and the status icons at the right, which are one
//! button that opens the system menu. With two keyboard layouts or more, the one in use stands
//! before the status icons by its short name, the way GNOME shows its input source. In Ghost mode
//! the name of the mode stands before both, for the whole session. The sizes and the colours below
//! are the ones the boot test counts.

use iced::widget::{button, column, container, row, space, stack, text};
use iced::{Background, Border, Color, Element, Length, Shadow, Theme};

use crate::icons;
use crate::status::Status;
use crate::theme::Palette;
use crate::ui::Message;

/// How tall the bar is in logical pixels. It is the exclusive zone as well, so windows sit under
/// it and the desktop never jumps.
pub const HEIGHT: u32 = 32;
/// The same height where a length is wanted.
const TALL: f32 = 32.0;
/// The hairline along the bottom edge of the bar, inside its height.
const LINE: f32 = 1.0;
/// The padding at each end of the bar.
const PAD: u16 = 8;
/// How tall a button in the bar is.
const ITEM: f32 = 24.0;
/// The bar's text, 10.5 pt at 96 dpi.
pub const TEXT_SIZE: f32 = 14.0;
/// A symbolic status icon.
const ICON: f32 = 16.0;
/// The gap between two status icons.
const ICON_GAP: f32 = 8.0;
/// The gap between the keyboard layout and the status icons.
const BUTTON_GAP: f32 = 4.0;

/// The button at the left, and the menu it opens.
pub const APPLICATIONS: &str = "Applications";
/// The mark the bar carries while the screen is being recorded, and the icon of the notification
/// that names the file afterwards.
pub const RECORDING: &str = "media-record-symbolic";

/// Which of the bar's menus are showing, which marks their buttons.
#[derive(Debug, Clone, Copy, Default)]
pub struct Open {
    /// The Applications menu.
    pub applications: bool,
    /// The clock menu.
    pub clock: bool,
    /// The system menu.
    pub system: bool,
}

/// What the bar says besides the clock and the status icons.
#[derive(Debug, Clone, Copy, Default)]
pub struct Marks<'a> {
    /// Do not disturb is on, which puts its icon at the left of the clock.
    pub quiet: bool,
    /// The screen is being recorded, which makes that mark the first status icon.
    pub recording: bool,
    /// The short name of the keyboard layout in use, when there are two or more.
    pub layout: Option<&'a str>,
    /// This boot is a Ghost one, which the bar says for the whole session.
    pub ghost: bool,
}

/// The bar. With Do not disturb on, its icon is at the left of the clock; while the screen is
/// being recorded, the mark for that is the first of the status icons; while there are two
/// keyboard layouts or more, the short name of the one in use is before the status icons; in Ghost
/// mode the name of the mode is before them all.
pub fn view<'a>(
    look: Palette,
    clock: &'a str,
    status: &Status,
    open: Open,
    marks: Marks<'a>,
) -> Element<'a, Message> {
    let mut right = row![].spacing(BUTTON_GAP).align_y(iced::Center);
    if marks.ghost {
        right = right.push(mode(look));
    }
    if let Some(name) = marks.layout {
        right = right.push(layout_button(look, name));
    }
    let items = row![
        applications(look, open.applications),
        space().width(Length::Fill),
        right.push(status_button(look, status, open.system, marks.recording)),
    ]
    .align_y(iced::Center)
    .height(Length::Fill);
    // the clock sits in the middle of the screen, not of what is left over, so it is a layer of
    // its own under the buttons. the space between them takes no click, so a click there reaches
    // the clock
    let middle = container(clock_button(look, clock, open.clock, marks.quiet)).center(Length::Fill);
    let content = container(stack![middle, items])
        .width(Length::Fill)
        .height(TALL - LINE)
        .padding([0, PAD])
        .style(move |_: &Theme| container::Style {
            background: Some(look.bar.into()),
            text_color: Some(look.text),
            ..container::Style::default()
        });
    let hairline = container(space().width(Length::Fill).height(LINE)).style(move |_: &Theme| {
        container::Style {
            background: Some(look.line.into()),
            ..container::Style::default()
        }
    });
    column![content, hairline].into()
}

fn applications(look: Palette, open: bool) -> Element<'static, Message> {
    // a button lays its content out at the top of its box, so the label is centred by hand
    let label = container(text(APPLICATIONS).size(TEXT_SIZE)).center_y(Length::Fill);
    button(label)
        .height(ITEM)
        .padding([0, PAD])
        .on_press(Message::ToggleMenu)
        .style(move |_: &Theme, state| fill(look, open, state))
        .into()
}

/// The clock, a button that opens the clock menu and is marked while it is open.
fn clock_button(look: Palette, clock: &str, open: bool, quiet: bool) -> Element<'_, Message> {
    let mut line = row![].spacing(ICON_GAP).align_y(iced::Center);
    if quiet {
        line = line.push(icons::symbolic(
            look.text,
            "notifications-disabled-symbolic",
            ICON,
        ));
    }
    line = line.push(text(clock).size(TEXT_SIZE).color(look.text));
    button(container(line).center_y(Length::Fill))
        .height(ITEM)
        .padding([0, PAD])
        .on_press(Message::ToggleClock)
        .style(move |_: &Theme, state| fill(look, open, state))
        .into()
}

/// The name of the mode this session is in, for the whole session. There is nothing to click: it
/// is a statement about the boot, not a setting, and it goes where every desktop puts what the
/// system is doing, at the left of the status icons. Ghost mode is the only one there is, and an
/// ordinary boot shows nothing here.
fn mode(look: Palette) -> Element<'static, Message> {
    container(text(librift::ghost::NAME).size(TEXT_SIZE).color(look.text))
        .center_y(Length::Fill)
        .padding([0, PAD])
        .into()
}

/// The keyboard layout in use, by its short name. A click switches to the next one, the way
/// Mod+Shift+Space does.
fn layout_button(look: Palette, name: &str) -> Element<'_, Message> {
    button(container(text(name).size(TEXT_SIZE).color(look.text)).center_y(Length::Fill))
        .height(ITEM)
        .padding([0, PAD])
        .on_press(Message::NextLayout)
        .style(move |_: &Theme, state| fill(look, false, state))
        .into()
}

/// The status icons, in the order every desktop puts them: the mark for a screen recording while
/// one is running, then the network, Bluetooth while a device is connected, the volume, then the
/// battery. An icon is there only when the system has something to say. Together they are one
/// button, which opens the system menu and is marked while it is open.
fn status_button(
    look: Palette,
    status: &Status,
    open: bool,
    recording: bool,
) -> Element<'static, Message> {
    let mut line = row![].spacing(ICON_GAP).align_y(iced::Center);
    let names = recording
        .then(|| RECORDING.to_string())
        .into_iter()
        .chain(status.icons());
    for name in names {
        if icons::find(&name).is_some() {
            line = line.push(icons::symbolic(look.text, &name, ICON));
        }
    }
    button(container(line).center_y(Length::Fill))
        .height(ITEM)
        .padding([0, PAD])
        .on_press(Message::ToggleSystem)
        .style(move |_: &Theme, state| fill(look, open, state))
        .into()
}

/// The fill behind a button of the bar: the pressed one while its menu is open, otherwise what the
/// pointer is doing.
fn fill(look: Palette, open: bool, state: button::Status) -> button::Style {
    let fill = if open {
        Some(look.press)
    } else {
        match state {
            button::Status::Hovered => Some(look.hover),
            button::Status::Pressed => Some(look.press),
            button::Status::Active | button::Status::Disabled => None,
        }
    };
    button::Style {
        background: fill.map(Background::from),
        text_color: look.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_height_is_the_same_number_twice() {
        assert!((f64::from(HEIGHT) - f64::from(TALL)).abs() < f64::EPSILON);
        assert!(
            f64::from(LINE) < f64::from(ITEM),
            "the hairline is not a row"
        );
    }
}
