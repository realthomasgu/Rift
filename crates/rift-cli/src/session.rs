//! `rift session`: the windows that were open, one row each, in the order they opened. The shell
//! writes the journal down as windows open, move and close, so this is what a session would come
//! back as on another machine.

use std::process::ExitCode;

use librift::apps::{self, App};
use librift::ghost;
use librift::session::{self, Passed, Window};

use crate::text;

const USAGE: &str = "Usage: rift session";

const HELP: &str = "Shows the windows that were open, with the app that opened each one and where \
it stood. The shell writes this down as windows open, move and close, and keeps it under home, so \
it travels with the drive. The apps come back at the next login unless the Owner page in Settings \
says not to.";

/// What the sentence under the rows says in a Ghost boot. The journal is written under home, which
/// is memory there, so these windows are real and the next login is a different machine as far as
/// they are concerned: nothing of them is on the drive to come back from.
const NOT_KEPT: &str = "These do not come back";

pub fn run(args: &[String]) -> ExitCode {
    match args {
        [] => {}
        [arg] if arg == "--help" || arg == "-h" => {
            println!("{USAGE}\n\n{HELP}");
            return ExitCode::SUCCESS;
        }
        [arg, ..] => return text::unknown("session", arg, USAGE),
    }
    let Some(windows) = session::kept() else {
        println!("The shell has not written the session down yet.");
        return ExitCode::SUCCESS;
    };
    if windows.is_empty() {
        println!("No windows were open.");
        return ExitCode::SUCCESS;
    }
    let apps = apps::load();
    print!("{}", rows(&windows, &apps));
    let (_, passed) = session::to_open(&windows, &apps);
    println!("\n{}", coming(&passed));
    for over in &passed {
        println!("{}.", over.line());
    }
    ExitCode::SUCCESS
}

/// The sentence under the rows: whether these windows come back at the next login. It is the only
/// place a person is told that the drive brings the session with it.
fn coming(passed: &[Passed]) -> String {
    if ghost::on() {
        return ghost::not_kept(NOT_KEPT);
    }
    if !session::restores() {
        return "These do not come back at the next login. The Owner page in Settings turns that on."
            .to_string();
    }
    if passed.is_empty() {
        return "These come back at the next login.".to_string();
    }
    "These come back at the next login, apart from the ones below.".to_string()
}

/// One row for each window: what it is, where it stood, and what it was showing.
fn rows(windows: &[Window], apps: &[App]) -> String {
    let named: Vec<(String, String, &str)> = windows
        .iter()
        .map(|win| (name(win, apps), place(win), win.title.as_str()))
        .collect();
    let first = named
        .iter()
        .map(|row| row.0.chars().count())
        .max()
        .unwrap_or(0);
    let second = named
        .iter()
        .map(|row| row.1.chars().count())
        .max()
        .unwrap_or(0);
    let mut text = String::new();
    for (app, place, title) in &named {
        let row = format!("{app:<first$}  {place:<second$}  {title}");
        text.push_str(row.trim_end());
        text.push('\n');
    }
    text
}

/// What to call a window: the name of the entry that opened it, then the id of that entry when it
/// is not installed here, then the app id the window carried.
fn name(win: &Window, apps: &[App]) -> String {
    apps.iter()
        .find(|app| app.id == win.app)
        .map(|app| app.name.clone())
        .or_else(|| Some(win.app.clone()).filter(|app| !app.is_empty()))
        .or_else(|| Some(win.window.clone()).filter(|window| !window.is_empty()))
        .unwrap_or_else(|| "A window".to_string())
}

/// Where a window stood, in the words the desktop uses for it. A column holds one window unless
/// windows were stacked in it, so the place in the column is named only when it is not the first.
fn place(win: &Window) -> String {
    let mut where_it_was = match win.workspace {
        0 => String::new(),
        number => format!("Workspace {number}"),
    };
    let rest = match (win.floating, win.column, win.tile) {
        (true, _, _) => "floating".to_string(),
        (false, Some(column), Some(1) | None) => format!("column {column}"),
        (false, Some(column), Some(tile)) => format!("column {column}, window {tile}"),
        (false, None, _) => String::new(),
    };
    if rest.is_empty() {
        return where_it_was;
    }
    if where_it_was.is_empty() {
        let mut first = rest;
        first[..1].make_ascii_uppercase();
        return first;
    }
    where_it_was.push_str(", ");
    where_it_was.push_str(&rest);
    where_it_was
}

#[cfg(test)]
mod tests {
    use super::*;
    use librift::apps::Category;

    fn app(id: &str, name: &str) -> App {
        App {
            id: id.to_string(),
            name: name.to_string(),
            exec: vec![id.to_string()],
            terminal: false,
            icon: None,
            wm_class: None,
            category: Category::Accessories,
            types: Vec::new(),
            line: String::new(),
        }
    }

    #[test]
    fn a_place_reads_the_way_the_desktop_names_it() {
        let win = |workspace, column, tile, floating| Window {
            workspace,
            column,
            tile,
            floating,
            ..Window::default()
        };
        assert_eq!(
            place(&win(1, Some(2), Some(1), false)),
            "Workspace 1, column 2"
        );
        assert_eq!(
            place(&win(3, Some(1), Some(2), false)),
            "Workspace 3, column 1, window 2"
        );
        assert_eq!(place(&win(2, None, None, true)), "Workspace 2, floating");
        // a window the compositor said nothing about the place of
        assert_eq!(place(&win(4, None, None, false)), "Workspace 4");
        assert_eq!(place(&win(0, Some(2), Some(1), false)), "Column 2");
        assert_eq!(place(&win(0, None, None, false)), "");
    }

    #[test]
    fn a_window_is_named_after_the_entry_that_opened_it() {
        let apps = [app("org.gnome.Nautilus", "Files")];
        let window = |entry: &str, app_id: &str| Window {
            app: entry.to_string(),
            window: app_id.to_string(),
            ..Window::default()
        };
        assert_eq!(
            name(&window("org.gnome.Nautilus", "nautilus"), &apps),
            "Files"
        );
        // an entry this machine does not have is still the truest thing said about the window
        assert_eq!(
            name(&window("com.example.App", "app"), &apps),
            "com.example.App"
        );
        assert_eq!(name(&window("", "xterm"), &apps), "xterm");
        assert_eq!(name(&window("", ""), &apps), "A window");
    }

    #[test]
    fn the_rows_line_up_and_carry_the_title() {
        let apps = [app("org.gnome.Nautilus", "Files")];
        let windows = [
            Window {
                app: "org.gnome.Nautilus".to_string(),
                title: "Home".to_string(),
                workspace: 1,
                column: Some(1),
                tile: Some(1),
                ..Window::default()
            },
            Window {
                window: "com.mitchellh.ghostty".to_string(),
                workspace: 2,
                floating: true,
                ..Window::default()
            },
        ];
        assert_eq!(
            rows(&windows, &apps),
            "Files                  Workspace 1, column 1  Home\n\
             com.mitchellh.ghostty  Workspace 2, floating\n"
        );
    }
}
