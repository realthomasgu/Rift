//! The start page, with the mark, what Welcome is for and the way on, and the page that takes its
//! place when there is no network: it says so, offers to open Welcome later, and turns into the
//! start page by itself once there is one. In Ghost mode the page says that nothing it sets is
//! kept, since Welcome only opens there when somebody asks for it.

use std::path::Path;

use iced::widget::{column, container, image, row, text};
use iced::{Center, Element, Fill, Length};
use librift::paths;

use crate::page::Page;
use crate::theme::Colors;
use crate::ui::{Message, Welcome};
use crate::widgets::{BOLD, GAP, TEXT_SIZE, TITLE_SIZE, action, primary};

/// How wide the mark is drawn: the width the About page in Settings gives it.
const MARK: f32 = 180.0;
/// How wide the sentences are, so a line stays short enough to read.
const WIDE: f32 = 460.0;

/// The page.
pub fn view(state: &Welcome, look: Colors) -> Element<'_, Message> {
    let (title, said, buttons) = if state.page == Page::Offline {
        (
            "No network",
            [
                "Welcome installs apps from Flathub, and this computer is not connected to a \
                 network.",
                "Connect from the menu at the right end of the bar and this page goes on by \
                 itself, or open Welcome later: it opens again at the next login.",
            ],
            row![
                action(look, "Open later", Some(Message::Later)),
                primary(look, "Continue", Some(Message::Next)),
            ],
        )
    } else {
        (
            "Welcome to Rift",
            [
                "Rift is running from this drive. The next pages set how the desktop looks, \
                 install apps from Flathub, and show how to run more programming languages.",
                // a Ghost session keeps nothing, so the second sentence says that instead of
                // saying it can all be changed later. Welcome does not open by itself here, so
                // anyone reading this asked for it from the Applications menu
                if librift::ghost::on() {
                    "This is a Ghost session, so none of it is kept: the drive stays locked and \
                     everything here is in memory until the machine goes off."
                } else {
                    "All of it can be changed later, and Welcome stays in the Applications menu."
                },
            ],
            row![
                action(look, "Close", Some(Message::Close)),
                primary(look, "Next", Some(Message::Next)),
            ],
        )
    };
    let mut page = column![
        mark(),
        text(title).size(TITLE_SIZE).font(BOLD).color(look.text)
    ]
    .spacing(GAP)
    .align_x(Center);
    for sentence in said {
        page = page.push(
            text(sentence)
                .size(TEXT_SIZE)
                .color(look.text)
                .align_x(Center)
                .width(Length::Fixed(WIDE)),
        );
    }
    if let Some(why) = &state.problem {
        page = page.push(
            text(why)
                .size(TEXT_SIZE)
                .color(look.error)
                .align_x(Center)
                .width(Length::Fixed(WIDE)),
        );
    }
    container(page.push(container(buttons.spacing(8)).padding([8, 0])))
        .width(Fill)
        .center_x(Fill)
        .padding([32, 20])
        .into()
}

/// The mark, when the image has it.
fn mark<'a>() -> Element<'a, Message> {
    let path = Path::new(paths::LOGO_MARK);
    if path.is_file() {
        container(image(image::Handle::from_path(path)).width(Length::Fixed(MARK)))
            .padding([8, 0])
            .into()
    } else {
        container(iced::widget::space().height(0.0)).into()
    }
}
