//! What the presses, the keys, the menus and the socket do: select, open, the clipboard, the
//! dialogs, the jobs and what they say when they are done.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use iced::futures::channel::oneshot;
use iced::widget::operation::{self, AbsoluteOffset};
use iced::{Point, Task, window};
use librift::appimage;
use librift::apps::{self, App};
use librift::defaults::Found;
use librift::drives::{self, Exchange};
use librift::files::trash::Trash;
use librift::files::{self, Entry, Kind, Sort, free_name, mime};

use crate::browser::{Browser, Location, Query, Toast};
use crate::dialogs::{Dialog, Putting, field_id};
use crate::jobs::{Step, Work};
use crate::keys::{self, Press};
use crate::list::search_id;
use crate::list::{ROW, list_id, path_id};
use crate::menus::{self, Menu, Opener, Which};
use crate::timeline;
use crate::ui::{self, Act, Clip, Files, Message};

/// How long a toast stays.
const TOAST: Duration = Duration::from_secs(6);
/// How long a job runs before the window shows how far it has got.
pub const PATIENCE: Duration = Duration::from_millis(500);
/// Where the header bar's menu opens, under its button: this much further in from the window's
/// right edge than a menu has to be, and this far down.
const MAIN_MENU: (f32, f32) = (44.0, 42.0);

/// A row was pressed: it alone is selected, or with Ctrl it joins the selection or leaves it, or
/// with Shift everything from the last press to it is selected.
pub fn press(state: &mut Files, id: window::Id, at: usize) -> Task<Message> {
    let modifiers = state.modifiers;
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    browser.menu = None;
    if modifiers.control() {
        browser.toggle(at);
    } else if modifiers.shift() {
        browser.extend_to(at);
    } else {
        browser.select_only(at);
    }
    Task::none()
}

/// A row was pressed twice: it is opened.
pub fn twice(state: &mut Files, id: window::Id, at: usize) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    browser.select_only(at);
    if browser.location == Location::Trash {
        return Task::none();
    }
    open(state, id, None)
}

/// A row was pressed with the right button: it is selected, unless it was already, and the menu
/// of the selection opens where the pointer is.
pub fn row_menu(state: &mut Files, id: window::Id, at: usize) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    let selected = browser
        .rows
        .get(at)
        .is_some_and(|entry| browser.selected.contains(&entry.name));
    if !selected {
        browser.select_only(at);
    }
    open_menu(state, id, Which::Selection, None);
    Task::none()
}

/// The empty part of the list was pressed with the right button: nothing is selected, and the
/// folder's menu opens where the pointer is.
pub fn blank_menu(state: &mut Files, id: window::Id) -> Task<Message> {
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.select_none();
    }
    open_menu(state, id, Which::Folder, None);
    Task::none()
}

/// The header bar's menu, under its button.
pub fn main_menu(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get(&id) else {
        return Task::none();
    };
    if browser
        .menu
        .as_ref()
        .is_some_and(|menu| menu.which == Which::Main)
    {
        if let Some(browser) = state.windows.get_mut(&id) {
            browser.menu = None;
        }
        return Task::none();
    }
    open_menu(
        state,
        id,
        Which::Main,
        Some(Point::new(browser.size.width, MAIN_MENU.1)),
    );
    // place() has moved the menu in from the window's right edge; it moves in a little more, to
    // stand under the button
    if let Some(menu) = state
        .windows
        .get_mut(&id)
        .and_then(|browser| browser.menu.as_mut())
    {
        menu.at.x = (menu.at.x - MAIN_MENU.0).max(0.0);
    }
    Task::none()
}

/// Open a menu at `at`, or where the last press landed, moved in from the edges. The menu of a
/// selection of files finds the apps that open them as it opens.
fn open_menu(state: &mut Files, id: window::Id, which: Which, at: Option<Point>) {
    let (default, others) = if which == Which::Selection {
        openers(state, id)
    } else {
        (None, Vec::new())
    };
    let Some(browser) = state.windows.get(&id) else {
        return;
    };
    let mut menu = Menu {
        which,
        at: at.unwrap_or(browser.pointer),
        default,
        others,
    };
    let items = menus::items(state, browser, id, &menu);
    menu.at = menus::place(menu.at, browser.size, &items);
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.menu = Some(menu);
    }
}

/// The app the selected files open with, and the others that say they open them: by the kind of
/// the first file, which is what a menu over several files offers.
fn openers(state: &Files, id: window::Id) -> (Option<Opener>, Vec<Opener>) {
    let Some(browser) = state.windows.get(&id) else {
        return (None, Vec::new());
    };
    let Some(first) = browser
        .chosen()
        .into_iter()
        .find(|entry| entry.kind != Kind::Folder)
    else {
        return (None, Vec::new());
    };
    let apps = apps::load();
    let found = Found::read();
    let default = found.opener(&first.mime, &apps, &state.types, Some(ui::APP_ID));
    let others = others(
        &apps,
        &state.types,
        &first.mime,
        default.as_ref().map(|app| app.id.as_str()),
    );
    (
        default.map(|app| (app.id, app.name)),
        others.into_iter().map(|app| (app.id, app.name)).collect(),
    )
}

/// The apps that say they open a kind of file, or a kind it is a kind of, besides the default, by
/// name.
#[must_use]
pub fn others(apps: &[App], types: &mime::Database, kind: &str, default: Option<&str>) -> Vec<App> {
    let kinds: Vec<String> = std::iter::once(types.canonical(kind).to_string())
        .chain(types.parents(kind))
        .collect();
    apps.iter()
        .filter(|app| Some(app.id.as_str()) != default && app.id != ui::APP_ID)
        .filter(|app| app.types.iter().any(|mime| kinds.contains(mime)))
        .cloned()
        .collect()
}

/// Do something to the selection or in the folder.
pub fn act(state: &mut Files, id: window::Id, act: Act) -> Task<Message> {
    match act {
        Act::Open => open(state, id, None),
        Act::OpenWith(app) => open(state, id, Some(&app)),
        Act::OpenWindow => open_windows(state, id),
        Act::NewFolder => new_folder(state, id),
        Act::Rename => rename(state, id),
        Act::Copy | Act::Cut => clip(state, id, act == Act::Cut),
        Act::Paste => paste(state, id),
        Act::Trash => trash(state, id),
        Act::Delete => {
            if let Some(browser) = state.windows.get_mut(&id) {
                let paths = browser.chosen_paths();
                if !paths.is_empty() {
                    browser.dialog = Some(Dialog::Delete {
                        paths,
                        trashless: false,
                    });
                }
            }
            Task::none()
        }
        Act::Restore | Act::Forget | Act::Empty => trash_act(state, id, &act),
        Act::Mount(drive) => disk(state, id, &drive, Doing::Mount),
        Act::Eject(drive) => disk(state, id, &drive, Doing::Eject),
        Act::MountExchange => mount_exchange(state, id),
        Act::Timeline => timeline_of(state, id),
        Act::Step { earlier } => step_to(state, id, earlier),
        Act::Now => now(state, id),
        Act::Bring => bring(state, id),
        Act::Search => open_search(state, id),
        Act::Grid(grid) => {
            if state.options.grid == grid {
                return Task::none();
            }
            state.options.grid = grid;
            let shown = rearrange(state);
            Task::batch([shown, crate::thumbs::want(state, id)])
        }
        Act::Properties => properties(state, id),
        Act::Unlock(drive) => ask_passphrase(state, id, &drive),
        Act::SelectAll => {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.select_all();
            }
            Task::none()
        }
        Act::Hidden => {
            state.options.hidden = !state.options.hidden;
            rearrange(state)
        }
        Act::Sort(sort) => {
            if state.options.sort == sort {
                state.options.reversed = !state.options.reversed;
            } else {
                state.options.sort = sort;
                state.options.reversed = false;
            }
            rearrange(state)
        }
        Act::Terminal => terminal(state, id),
        Act::NewWindow => {
            let location = state
                .windows
                .get(&id)
                .map(|browser| browser.location.clone());
            let location = location.unwrap_or_else(|| Location::Folder(PathBuf::from("/")));
            ui::open_window(state, location, None)
        }
        Act::Close => window::close(id),
        Act::Reload => reload(state, id),
        Act::Location => location(state, id),
        Act::Undo(number) => undo(state, id, number),
        Act::Stop(number) => {
            if let Some(job) = state.jobs.iter().find(|job| job.number == number) {
                job.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            Task::none()
        }
    }
}

/// The three that are about the trash: put back what is selected there, delete it for good after
/// the question, and empty the whole trash after the question.
fn trash_act(state: &mut Files, id: window::Id, act: &Act) -> Task<Message> {
    match act {
        Act::Restore => {
            let files = chosen_files(state, id);
            if files.is_empty() {
                return Task::none();
            }
            ui::start_job(state, Some(id), Work::Restore(files))
        }
        Act::Forget => {
            if let Some(browser) = state.windows.get_mut(&id)
                && browser.location == Location::Trash
            {
                let chosen = browser.chosen();
                let files: Vec<PathBuf> = chosen
                    .iter()
                    .map(|entry| PathBuf::from(&entry.name))
                    .collect();
                let labels = chosen.iter().map(|entry| entry.label.clone()).collect();
                if !files.is_empty() {
                    browser.dialog = Some(Dialog::Forget { files, labels });
                }
            }
            Task::none()
        }
        _ => {
            if let Some(browser) = state.windows.get_mut(&id)
                && state.trash_full
            {
                browser.dialog = Some(Dialog::Empty);
            }
            Task::none()
        }
    }
}

/// Read what the window shows again now, whether anything in it has changed or not.
fn reload(state: &mut Files, id: window::Id) -> Task<Message> {
    let now = state
        .windows
        .get(&id)
        .map(|browser| ui::stamp(state, &browser.location))
        .unwrap_or_default();
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.stamp = now;
    }
    ui::read(state, id)
}

/// Where each selected row lies, which in the trash is its path in the trash it is in.
fn chosen_files(state: &Files, id: window::Id) -> Vec<PathBuf> {
    state.windows.get(&id).map_or_else(Vec::new, |browser| {
        browser
            .chosen()
            .iter()
            .map(|entry| PathBuf::from(&entry.name))
            .collect()
    })
}

/// Keep the options for the next window and show every window the new way.
fn rearrange(state: &mut Files) -> Task<Message> {
    let _ = state.options.save();
    let options = state.options;
    for browser in state.windows.values_mut() {
        browser.arrange(options);
    }
    Task::none()
}

/// Open the selection: one folder by going into it, several in windows of their own, and files
/// with the apps that open them, each app once with all of its files. A file no app opens says so,
/// and one application in one file is asked for first.
fn open(state: &mut Files, id: window::Id, with: Option<&str>) -> Task<Message> {
    let Some(browser) = state.windows.get(&id) else {
        return Task::none();
    };
    let Some(place) = browser.location.place() else {
        return Task::none();
    };
    let here = browser.location.clone();
    let chosen: Vec<(OsString, Kind, String)> = browser
        .chosen()
        .iter()
        .map(|entry| (entry.name.clone(), entry.kind, entry.mime.clone()))
        .collect();
    let (folders, files): (Vec<_>, Vec<_>) = chosen
        .into_iter()
        .partition(|(_, kind, _)| *kind == Kind::Folder);
    // a folder in a moment opens at that same moment, and one a search found opens where it lies
    let inside = |name: &OsStr| {
        here.inside(name)
            .unwrap_or(Location::Folder(place.join(name)))
    };
    if with.is_none() && files.is_empty() && folders.len() == 1 {
        return ui::go(state, id, inside(&folders[0].0));
    }
    let mut tasks = Vec::new();
    if with.is_none() {
        for (name, _, _) in folders {
            let opening = inside(&name);
            tasks.push(ui::open_window(state, opening, None));
        }
    }
    if files.is_empty() {
        return Task::batch(tasks);
    }
    // an application in one file is no kind an app opens: the question is whether to run it, and
    // the file's own first bytes are what says it is one, not its name
    if let (None, [(name, Kind::File, _)]) = (with, files.as_slice()) {
        let path = place.join(name);
        if appimage::of(&path).is_some() {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.dialog = Some(Dialog::Run { path });
            }
            return Task::batch(tasks);
        }
    }
    let all = apps::load();
    let found = Found::read();
    let mut groups: Vec<(App, Vec<PathBuf>)> = Vec::new();
    let mut unopened: Vec<PathBuf> = Vec::new();
    for (name, kind, mime) in files {
        let path = place.join(&name);
        let app = match with {
            Some(wanted) => all.iter().find(|app| app.id == wanted).cloned(),
            None if kind == Kind::File => found.opener(&mime, &all, &state.types, Some(ui::APP_ID)),
            None => None,
        };
        match app {
            Some(app) => match groups.iter_mut().find(|(known, _)| known.id == app.id) {
                Some((_, paths)) => paths.push(path),
                None => groups.push((app, vec![path])),
            },
            None => unopened.push(path),
        }
    }
    let mut problem = None;
    for (app, paths) in &groups {
        if let Err(why) = apps::launch(app, paths) {
            problem.get_or_insert(format!("{why}."));
        }
    }
    let problem = problem.or_else(|| match unopened.as_slice() {
        [] => None,
        [one] => Some(format!(
            "No app opens {}.",
            one.file_name().unwrap_or_default().to_string_lossy()
        )),
        more => Some(format!("No app opens {} of these files.", more.len())),
    });
    if let Some(problem) = problem {
        tasks.push(toast(state, id, problem, None));
    }
    Task::batch(tasks)
}

/// Run an application in one file, once the question has been answered: `appimage-run` lays out
/// the ordinary Linux folders the program inside it asks for, over the store and read only, and
/// runs it in them. It goes into a scope of its own named after the file, the way every app the
/// session starts does, and it starts in the folder it lies in. The file needs no permission of
/// its own, since appimage-run reads it rather than runs it.
fn run_file(state: &mut Files, id: window::Id, path: &Path) -> Task<Message> {
    let name = path
        .file_name()
        .unwrap_or(OsStr::new("AppImage"))
        .to_string_lossy()
        .into_owned();
    let words = vec![
        appimage::RUN.to_string(),
        path.to_string_lossy().into_owned(),
    ];
    match apps::start(&name, &name, &words, path.parent()) {
        Ok(()) => Task::none(),
        Err(why) => toast(state, id, format!("{why}."), None),
    }
}

/// Each selected folder in a window of its own.
fn open_windows(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get(&id) else {
        return Task::none();
    };
    let place = browser.location.place();
    let folders: Vec<Location> = browser
        .chosen()
        .iter()
        .filter(|entry| entry.kind == Kind::Folder)
        .filter_map(|entry| {
            browser.location.inside(&entry.name).or_else(|| {
                place
                    .as_ref()
                    .map(|place| Location::Folder(place.join(&entry.name)))
            })
        })
        .collect();
    Task::batch(
        folders
            .into_iter()
            .map(|folder| ui::open_window(state, folder, None)),
    )
}

/// The dialog for a new folder, with a free name typed and selected.
fn new_folder(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    let Some(folder) = browser.location.folder() else {
        return Task::none();
    };
    let name = free_name(folder, OsStr::new("New folder"), false)
        .to_string_lossy()
        .into_owned();
    browser.dialog = Some(Dialog::NewFolder { name });
    let field = field_id(browser.number);
    Task::batch([
        operation::focus(field.clone()),
        operation::select_all(field),
    ])
}

/// The dialog for a new name for the one thing selected, with its name typed and the part before
/// its ending selected, the way GNOME's Files selects it.
fn rename(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    // nothing in the trash or in a moment is renamed where it lies
    if browser.location.folder().is_none() {
        return Task::none();
    }
    let Some(place) = browser.location.place() else {
        return Task::none();
    };
    let chosen = browser.chosen();
    let [entry] = chosen.as_slice() else {
        return Task::none();
    };
    let name = entry.label.clone();
    let path = place.join(&entry.name);
    let from = OsString::from(&name);
    let inside = path.parent().unwrap_or(&place).to_path_buf();
    let folder = entry.kind == Kind::Folder;
    let stem = if folder {
        name.chars().count()
    } else {
        name.rfind('.')
            .filter(|at| *at > 0)
            .map_or(name.chars().count(), |at| name[..at].chars().count())
    };
    browser.dialog = Some(Dialog::Rename {
        from,
        inside,
        name,
        folder,
    });
    let field = field_id(browser.number);
    Task::batch([
        operation::focus(field.clone()),
        operation::select_range(field, 0, stem),
    ])
}

/// Cut or copy the selection: the app keeps the paths, and the session's clipboard gets them as
/// text, one a line, which a terminal or an editor takes.
fn clip(state: &mut Files, id: window::Id, cut: bool) -> Task<Message> {
    let Some(browser) = state.windows.get(&id) else {
        return Task::none();
    };
    let paths = browser.chosen_paths();
    if paths.is_empty() {
        return Task::none();
    }
    let text = paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    state.clipboard = Some(Clip {
        paths,
        cut,
        text: text.clone(),
    });
    iced::clipboard::write(text)
}

/// Paste: first look at what the session's clipboard holds now.
fn paste(state: &Files, id: window::Id) -> Task<Message> {
    let in_folder = state
        .windows
        .get(&id)
        .is_some_and(|browser| browser.location.folder().is_some());
    if state.clipboard.is_none() || !in_folder {
        return Task::none();
    }
    iced::clipboard::read().map(move |text| Message::Pasted(id, text))
}

/// Paste what was cut or copied into the folder, when the session's clipboard still holds it, or
/// holds nothing this app can read. Something copied elsewhere since takes its place.
pub fn pasted(state: &mut Files, id: window::Id, text: Option<&str>) -> Task<Message> {
    let Some(clip) = state.clipboard.clone() else {
        return Task::none();
    };
    if text.is_some_and(|text| text.trim_end() != clip.text) {
        state.clipboard = None;
        return Task::none();
    }
    let Some(into) = state
        .windows
        .get(&id)
        .and_then(|browser| browser.location.folder().map(Path::to_path_buf))
    else {
        return Task::none();
    };
    // what is already there under the same name. nothing is written over without being asked.
    // something pasted into the folder it is in already is only ever a second copy of itself, so
    // it keeps both without a question, the way it did before there was one
    let taken: Vec<String> = clip
        .paths
        .iter()
        .filter(|path| path.parent() != Some(into.as_path()))
        .filter_map(|path| path.file_name())
        .filter(|name| fs::symlink_metadata(into.join(name)).is_ok())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    let putting = if clip.cut {
        Putting::Move
    } else {
        Putting::Copy
    };
    if taken.is_empty() {
        return transfer(state, id, clip.paths, into, &putting, false);
    }
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.dialog = Some(Dialog::Replace {
            from: clip.paths,
            into,
            putting,
            names: taken,
        });
    }
    Task::none()
}

/// Copy or move what was on the clipboard into a folder, or put back what a moment holds, and let
/// the clipboard go when it was cut, since a cut is pasted once.
fn transfer(
    state: &mut Files,
    id: window::Id,
    from: Vec<PathBuf>,
    into: PathBuf,
    putting: &Putting,
    replace: bool,
) -> Task<Message> {
    let work = match putting {
        Putting::Move => {
            state.clipboard = None;
            Work::Move {
                from,
                into,
                replace,
            }
        }
        Putting::Copy => Work::Copy {
            from,
            into,
            replace,
        },
        Putting::Bring(_) => Work::Bring {
            from,
            into,
            replace,
        },
    };
    ui::start_job(state, Some(id), work)
}

/// Move the selection to the trash, or ask to delete it for good when some of it is somewhere with
/// no trash.
fn trash(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    if browser.location == Location::Trash {
        return act(state, id, Act::Forget);
    }
    let paths = browser.chosen_paths();
    if paths.is_empty() {
        return Task::none();
    }
    let uid = files::uid();
    let takes = paths
        .iter()
        .all(|path| Trash::for_path(path, uid).is_some());
    if !takes {
        browser.dialog = Some(Dialog::Delete {
            paths,
            trashless: true,
        });
        return Task::none();
    }
    ui::start_job(state, Some(id), Work::Trash(paths))
}

/// A terminal in this folder, as an app of its own. Ghostty is told to start a process of its own,
/// which is the one that takes the folder.
fn terminal(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(folder) = state
        .windows
        .get(&id)
        .and_then(|browser| browser.location.folder().map(Path::to_path_buf))
    else {
        return Task::none();
    };
    let words = [
        "ghostty".to_string(),
        "--gtk-single-instance=false".to_string(),
        format!("--working-directory={}", folder.display()),
    ];
    match apps::start("com.mitchellh.ghostty", "Ghostty", &words, Some(&folder)) {
        Ok(()) => Task::none(),
        Err(why) => toast(state, id, format!("{why}."), None),
    }
}

/// The path bar as a field, with the path in it selected.
fn location(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    let typed = browser
        .location
        .about()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    browser.typing = Some(typed);
    let field = path_id(browser.number);
    Task::batch([
        operation::focus(field.clone()),
        operation::select_all(field),
    ])
}

/// Enter in the path bar: a folder is gone to, a file is selected in its folder, and anything else
/// says it is not there.
pub fn path_entered(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(typed) = state
        .windows
        .get_mut(&id)
        .and_then(|browser| browser.typing.take())
    else {
        return Task::none();
    };
    let path = files::path_of(&typed);
    go_to_path(state, id, &path).unwrap_or_else(|| {
        toast(
            state,
            id,
            format!("There is nothing at {}.", path.display()),
            None,
        )
    })
}

/// Go to a folder, or to the folder of a file with the file selected. `None` when neither is there.
fn go_to_path(state: &mut Files, id: window::Id, path: &Path) -> Option<Task<Message>> {
    if path.is_dir() {
        return Some(ui::go(state, id, Location::Folder(path.to_path_buf())));
    }
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return None;
    };
    if fs::symlink_metadata(path).is_err() || !folder.is_dir() {
        return None;
    }
    let task = ui::go(state, id, Location::Folder(folder.to_path_buf()));
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.select_after = vec![name.to_owned()];
    }
    Some(task)
}

/// Show this folder as it was: Vault lists the moments on a thread of its own, since the answer
/// comes over the bus, and the window goes to the newest of them.
fn timeline_of(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(folder) = state
        .windows
        .get(&id)
        .and_then(|browser| browser.location.about())
        .map(Path::to_path_buf)
    else {
        return Task::none();
    };
    if !timeline::covers(&folder) {
        return toast(state, id, NO_TIMELINE.to_string(), None);
    }
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(Message::Moments(id, Box::new(timeline::moments())));
    });
    Task::perform(receiver, move |said| said.unwrap_or(Message::CloseMenu(id)))
}

/// What Vault answered: the window goes to the newest moment, or says why there is none.
pub fn moments(
    state: &mut Files,
    id: window::Id,
    listed: Result<Vec<String>, String>,
) -> Task<Message> {
    let found = match listed {
        Ok(found) => found,
        Err(why) => return toast(state, id, why, None),
    };
    let Some(newest) = found.last().cloned() else {
        return toast(state, id, NO_MOMENTS.to_string(), None);
    };
    state.moments = found;
    let Some(folder) = state
        .windows
        .get(&id)
        .and_then(|browser| browser.location.about())
        .map(Path::to_path_buf)
    else {
        return Task::none();
    };
    ui::go(state, id, Location::Moment { at: newest, folder })
}

/// The moment before this one, or the one after it.
fn step_to(state: &mut Files, id: window::Id, earlier: bool) -> Task<Message> {
    let Some(Location::Moment { at, folder }) = state
        .windows
        .get(&id)
        .map(|browser| browser.location.clone())
    else {
        return Task::none();
    };
    let Some(step) = timeline::step(&state.moments, &at, earlier).cloned() else {
        return Task::none();
    };
    ui::go(state, id, Location::Moment { at: step, folder })
}

/// Out of the Timeline and back to the folder as it is now.
fn now(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(folder) = state
        .windows
        .get(&id)
        .filter(|browser| browser.location.at().is_some())
        .and_then(|browser| browser.location.about())
        .map(Path::to_path_buf)
    else {
        return Task::none();
    };
    ui::go(state, id, Location::Folder(folder))
}

/// Put back what is selected in a moment, or everything in it when nothing is selected. A copy
/// out of the snapshot, so a name that is taken is only replaced after the question, and what was
/// there goes to the trash.
fn bring(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get(&id) else {
        return Task::none();
    };
    let (Some(at), Some(folder), Some(place)) = (
        browser.location.at().map(ToString::to_string),
        browser.location.about().map(Path::to_path_buf),
        browser.location.place(),
    ) else {
        return Task::none();
    };
    let names: Vec<OsString> = browser
        .chosen()
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    // a row a search found is named by its path under the folder, and a restore has one folder to
    // put things back in, so the rows of one folder go back at a time
    let Some((under, names)) = timeline::in_one_folder(&names) else {
        return toast(state, id, MANY_FOLDERS.to_string(), None);
    };
    let (place, folder) = (place.join(&under), folder.join(&under));
    let (from, taken) = timeline::bringing(&place, &folder, &names);
    if from.is_empty() {
        return toast(
            state,
            id,
            "There is nothing here to put back.".to_string(),
            None,
        );
    }
    let putting = Putting::Bring(timeline::label(&at, librift::time::now(), state.offset));
    if taken.is_empty() {
        return transfer(state, id, from, folder, &putting, false);
    }
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.dialog = Some(Dialog::Replace {
            from,
            into: folder,
            putting,
            names: taken,
        });
    }
    Task::none()
}

/// The search field in the header bar, with the words that are in it selected.
fn open_search(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    if !searchable(browser) {
        return Task::none();
    }
    browser.typing = None;
    if browser.search.is_none() {
        browser.search = Some(Query {
            words: String::new(),
            meaning: false,
            problem: None,
        });
    }
    let field = search_id(browser.number);
    Task::batch([
        operation::focus(field.clone()),
        operation::select_all(field),
    ])
}

/// Whether what the window shows can be searched: a folder, or a folder in a moment. The trash is
/// not a folder to walk, so it has nothing to search.
fn searchable(browser: &Browser) -> bool {
    browser.location.place().is_some()
}

/// What is typed in the search field: the list follows every letter. An empty field brings the
/// folder itself back.
pub fn search_typed(state: &mut Files, id: window::Id, typed: String) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    if !searchable(browser) {
        return Task::none();
    }
    browser.search = Some(Query {
        words: typed,
        meaning: false,
        problem: None,
    });
    // the rows are about to be the ones the names find, in the list's own order again
    browser.ranked = false;
    browser.select_none();
    ui::read(state, id)
}

/// Enter in the search field: the same words, by meaning this time.
pub fn search_entered(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    let Some(query) = browser.search.as_mut() else {
        return Task::none();
    };
    if query.words.trim().is_empty() {
        return Task::none();
    }
    query.meaning = true;
    query.problem = None;
    ui::read(state, id)
}

/// What a search found. An answer for words the field no longer holds is let go, since a walk
/// and a question to Quasar both take their own time.
pub fn searched(
    state: &mut Files,
    id: window::Id,
    words: &str,
    meaning: bool,
    found: Result<Vec<Entry>, String>,
) -> Task<Message> {
    let options = state.options;
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    // what the names found is worth showing under the sentence a search by meaning could not run,
    // so it is taken as well, as long as the words are still the ones in the field and the rows
    // are not already the ones the index found
    let asked = browser.search.as_ref().is_some_and(|query| {
        query.words.trim() == words && (query.meaning == meaning || (!meaning && !browser.ranked))
    });
    if !asked {
        return Task::none();
    }
    match found {
        Ok(rows) => {
            // only an answer by meaning takes the sentence away, since what the names found can
            // arrive after it and would otherwise clear it
            if meaning && let Some(query) = browser.search.as_mut() {
                query.problem = None;
            }
            browser.show_found(rows, meaning, options);
        }
        // the rows the names found stay under the sentence that says why there are no others
        Err(why) => {
            if let Some(query) = browser.search.as_mut() {
                query.problem = Some(why);
            }
        }
    }
    Task::none()
}

/// What a selection spread over several folders says.
const MANY_FOLDERS: &str =
    "These are in different folders. Put back the ones in one folder at a time.";
/// What a folder with no snapshots behind it says.
const NO_TIMELINE: &str = "Only your home folder is snapshotted, so this folder has no Timeline.";
/// What a drive with no snapshots yet says.
const NO_MOMENTS: &str = "Vault has taken no snapshots of your home folder yet.";

/// Take back what a job did: what it moved to the trash comes back, once.
fn undo(state: &mut Files, id: window::Id, number: u64) -> Task<Message> {
    let files = state
        .jobs
        .iter_mut()
        .find(|job| job.number == number)
        .and_then(|job| job.outcome.as_mut())
        .map(|outcome| std::mem::take(&mut outcome.trashed))
        .unwrap_or_default();
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.toast = None;
    }
    if files.is_empty() {
        return Task::none();
    }
    ui::start_job(state, Some(id), Work::Restore(files))
}

/// The default button of the dialog that is open.
pub fn confirm(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    let Some(dialog) = browser.dialog.clone() else {
        return Task::none();
    };
    let folder = browser.location.folder().map(Path::to_path_buf);
    if let Some(folder) = &folder
        && !dialog.ready(folder)
    {
        return Task::none();
    }
    // the passphrase keeps its dialog: it stays up while udisks is asked, and again with the
    // sentence in it when the passphrase does not open the disk
    if matches!(dialog, Dialog::Unlock { .. }) {
        return unlock(state, id);
    }
    browser.dialog = None;
    match dialog {
        Dialog::NewFolder { name } => {
            let Some(folder) = folder else {
                return Task::none();
            };
            match fs::create_dir(folder.join(&name)) {
                Ok(()) => reread_selecting(state, id, OsString::from(name)),
                Err(e) => toast(state, id, format!("Could not make {name}: {e}."), None),
            }
        }
        Dialog::Rename {
            from, inside, name, ..
        } => {
            if from == OsStr::new(&name) {
                return Task::none();
            }
            match crate::jobs::rename_new(&inside.join(&from), &inside.join(&name)) {
                Ok(()) => reread_selecting(state, id, OsString::from(name)),
                Err(e) => toast(
                    state,
                    id,
                    format!("Could not rename {}: {e}.", from.to_string_lossy()),
                    None,
                ),
            }
        }
        Dialog::Delete { paths, .. } => ui::start_job(state, Some(id), Work::Delete(paths)),
        Dialog::Forget { files, .. } => ui::start_job(state, Some(id), Work::Forget(files)),
        Dialog::Empty => {
            let roots = state
                .trashes()
                .iter()
                .map(|trash| trash.root().to_path_buf())
                .collect();
            ui::start_job(state, Some(id), Work::Empty(roots))
        }
        Dialog::Run { path } => run_file(state, id, &path),
        Dialog::Properties(_) | Dialog::Unlock { .. } => Task::none(),
        // its default button keeps both, which is what the job does when nothing is replaced
        Dialog::Replace {
            from,
            into,
            putting,
            ..
        } => transfer(state, id, from, into, &putting, false),
    }
}

/// The other answer to the question before a name is replaced: what is there goes to the trash
/// and the new one takes its name.
pub fn replace(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(Dialog::Replace {
        from,
        into,
        putting,
        ..
    }) = state
        .windows
        .get_mut(&id)
        .and_then(|browser| browser.dialog.take())
    else {
        return Task::none();
    };
    transfer(state, id, from, into, &putting, true)
}

/// Read the folder again now, and select a name in it once it has been read.
fn reread_selecting(state: &mut Files, id: window::Id, name: OsString) -> Task<Message> {
    let now = state
        .windows
        .get(&id)
        .map(|browser| ui::stamp(state, &browser.location))
        .unwrap_or_default();
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.select_after = vec![name];
        browser.stamp = now;
    }
    ui::read(state, id)
}

/// Select what was waiting to be selected once the folder was read, and scroll to it.
pub fn select_waiting(browser: &mut Browser, grid: bool) -> Task<Message> {
    if browser.select_after.is_empty() {
        return Task::none();
    }
    let names = std::mem::take(&mut browser.select_after);
    let found: Vec<usize> = names
        .iter()
        .filter_map(|name| browser.position(name))
        .collect();
    let Some(&first) = found.first() else {
        return Task::none();
    };
    browser.select_only(first);
    for &at in &found[1..] {
        browser.toggle(at);
    }
    browser.cursor = Some(browser.rows[first].name.clone());
    scroll_to(browser, first, grid)
}

/// Scroll the list or the grid so a row is on screen, when it is not.
fn scroll_to(browser: &Browser, at: usize, grid: bool) -> Task<Message> {
    let where_to = if grid {
        crate::grid::scroll_for(browser, at)
    } else {
        browser.scroll_for(at, ROW)
    };
    match where_to {
        Some(y) => operation::scroll_to(
            list_id(browser.number),
            AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        ),
        None => Task::none(),
    }
}

/// A key the window did not use.
pub fn key(
    state: &mut Files,
    id: window::Id,
    key: &iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    if browser.dialog.is_some() || browser.typing.is_some() {
        return Task::none();
    }
    browser.menu = None;
    let trash = browser.location == Location::Trash;
    let grid = state.options.grid;
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    let across = crate::grid::across(browser.size.width);
    let stepped = |browser: &mut Browser, by: isize, extend: bool| match browser.step(by, extend) {
        Some(at) => scroll_to(browser, at, grid),
        None => Task::none(),
    };
    match keys::press(key, modifiers, trash, grid) {
        Some(Press::Act(act)) => self::act(state, id, act),
        Some(Press::Step(by, extend)) => {
            let task = stepped(browser, by, extend);
            Task::batch([task, crate::thumbs::want(state, id)])
        }
        Some(Press::Line(lines, extend)) => {
            #[allow(clippy::cast_possible_wrap)]
            let by = lines * across as isize;
            let task = stepped(browser, by, extend);
            Task::batch([task, crate::thumbs::want(state, id)])
        }
        Some(Press::Page(pages, extend)) => {
            let tall = if grid { crate::grid::TALL } else { ROW };
            let rows = (browser.viewport / tall).floor().max(1.0);
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let by = pages * rows as isize * if grid { across as isize } else { 1 };
            let task = stepped(browser, by, extend);
            Task::batch([task, crate::thumbs::want(state, id)])
        }
        Some(Press::Back) => ui::travel(state, id, Browser::go_back),
        Some(Press::Forward) => ui::travel(state, id, Browser::go_forward),
        Some(Press::Up) => ui::up(state, id),
        Some(Press::Home) => {
            let home = state.places.first().map(|place| place.path.clone());
            home.map_or_else(Task::none, |home| ui::go(state, id, Location::Folder(home)))
        }
        None => Task::none(),
    }
}

/// Escape: the dialog, the menu or the path bar closes, or else the selection goes.
pub fn escape(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    if browser.dialog.take().is_some()
        || browser.menu.take().is_some()
        || browser.typing.take().is_some()
    {
        return Task::none();
    }
    // the field goes, and the folder itself comes back under it
    if browser.search.take().is_some() {
        return reload(state, id);
    }
    browser.select_none();
    Task::none()
}

/// How a job is going. When it ends, every window reads its place again, what it made is selected
/// where it was made, and the window it was started from says how it went, or a notification does
/// when that window has closed.
pub fn job_step(state: &mut Files, number: u64, step: Step) -> Task<Message> {
    let Some(job) = state.jobs.iter_mut().find(|job| job.number == number) else {
        return Task::none();
    };
    let outcome = match step {
        Step::Counted(total) => {
            job.total = total;
            return Task::none();
        }
        Step::Moved(done) => {
            job.done = done;
            return Task::none();
        }
        Step::Finished(outcome) => *outcome,
    };
    job.done = job.total;
    let said = job.work.done(&outcome);
    let undo =
        (matches!(job.work, Work::Trash(_)) && !outcome.trashed.is_empty()).then_some(number);
    let started_in = job.window;
    let made_into = match &job.work {
        Work::Copy { into, .. } | Work::Move { into, .. } => Some(into.clone()),
        _ => None,
    };
    let made: Vec<OsString> = outcome
        .made
        .iter()
        .filter_map(|path| path.file_name().map(OsStr::to_owned))
        .collect();
    job.outcome = Some(outcome);
    let mut tasks = Vec::new();
    let ids: Vec<window::Id> = state.windows.keys().copied().collect();
    for id in ids {
        let now = state
            .windows
            .get(&id)
            .map(|browser| ui::stamp(state, &browser.location))
            .unwrap_or_default();
        if let Some(browser) = state.windows.get_mut(&id) {
            if Some(id) == started_in && made_into.as_deref() == browser.location.folder() {
                browser.select_after.clone_from(&made);
            }
            browser.stamp = now;
        }
        tasks.push(ui::read(state, id));
    }
    state.trash_full = state.anything_trashed();
    match started_in.filter(|id| state.windows.contains_key(id)) {
        Some(id) => tasks.push(toast(state, id, said, undo)),
        None => tell(&said),
    }
    if state.windows.is_empty() && !state.busy() {
        return iced::exit();
    }
    Task::batch(tasks)
}

/// Say something at the bottom of a window for a few seconds, with Undo when there is a job to
/// take back.
pub fn toast(state: &mut Files, id: window::Id, said: String, undo: Option<u64>) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    browser.toasts += 1;
    let number = browser.toasts;
    browser.toast = Some(Toast { said, undo, number });
    Task::perform(async { thread::sleep(TOAST) }, move |()| {
        Message::ToastGone(id, number)
    })
}

/// Tell the owner how a job went when its window has closed, the way the shell shows any
/// notification.
fn tell(said: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["--app-name=Files", "--icon=folder-symbolic", said])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Do what pressing something in the window in front would, for `rift-files --set`.
pub fn set(state: &mut Files, name: &str, value: &str) -> Task<Message> {
    let Some(id) = state.front_id() else {
        return Task::none();
    };
    let value = value.trim();
    let row = state
        .windows
        .get(&id)
        .and_then(|browser| browser.rows.iter().position(|entry| entry.label == value));
    let message = match (name, row) {
        ("open", _) => {
            let path = files::path_of(value);
            return go_to_path(state, id, &path).unwrap_or_else(Task::none);
        }
        ("place", _) => return place(state, id, value),
        ("drive", _) => return by_name(state, id, value, Doing::Mount),
        ("unmount", _) => return by_name(state, id, value, Doing::Unmount),
        ("eject", _) => return by_name(state, id, value, Doing::Eject),
        ("replace", _) => Message::Replace(id),
        ("select", Some(at)) => {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.select_only(at);
            }
            return Task::none();
        }
        ("also", Some(at)) => {
            if let Some(browser) = state.windows.get_mut(&id)
                && !browser.selected.contains(&browser.rows[at].name)
            {
                browser.toggle(at);
            }
            return Task::none();
        }
        ("activate", Some(at)) => Message::Twice(id, at),
        ("select-none", _) => Message::Blank(id),
        ("menu", _) => return set_menu(state, id, value),
        ("view", _) => return act(state, id, Act::Grid(value == "grid")),
        ("unlock", _) => return by_name(state, id, value, Doing::Unlock),
        ("search", _) => return search_for(state, id, value, false),
        ("meaning", _) => return search_for(state, id, value, true),
        ("new-folder" | "rename", _) => return typed_dialog(state, id, name, value),
        ("type", _) => Message::Typed(id, value.to_string()),
        ("confirm", _) => Message::Confirm(id),
        ("cancel" | "escape", _) => Message::Escape(id),
        ("hidden", _) => {
            if state.options.hidden != (value == "on") {
                return act(state, id, Act::Hidden);
            }
            return Task::none();
        }
        ("sort", _) => {
            let Some(sort) = Sort::from_word(value) else {
                return Task::none();
            };
            state.options.sort = sort;
            state.options.reversed = false;
            return rearrange(state);
        }
        ("back", _) => Message::Back(id),
        ("forward", _) => Message::Forward(id),
        ("up", _) => Message::Up(id),
        ("picture", _) => return ui::picture(state, PathBuf::from(value)),
        ("undo", _) => {
            // the last move to the trash that has not been taken back, whether its toast is still
            // up or not
            let undo = state
                .jobs
                .iter()
                .rev()
                .find(|job| {
                    matches!(job.work, Work::Trash(_))
                        && job
                            .outcome
                            .as_ref()
                            .is_some_and(|done| !done.trashed.is_empty())
                })
                .map(|job| job.number);
            return undo.map_or_else(Task::none, |number| act(state, id, Act::Undo(number)));
        }
        ("stop", _) => {
            for job in state.jobs.iter().filter(|job| job.running()) {
                job.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            return Task::none();
        }
        (word, _) => match named_act(word, value) {
            Some(act) => Message::Do(id, act),
            None => return Task::none(),
        },
    };
    Task::done(message)
}

/// The acts `--set` names with a word of their own; the value is there to be typed.
fn named_act(word: &str, value: &str) -> Option<Act> {
    Some(match word {
        "open-selected" => Act::Open,
        "open-with" => Act::OpenWith(value.to_string()),
        "copy" => Act::Copy,
        "cut" => Act::Cut,
        "paste" => Act::Paste,
        "trash" => Act::Trash,
        "delete" => Act::Delete,
        "restore" => Act::Restore,
        "forget" => Act::Forget,
        "empty" => Act::Empty,
        "select-all" => Act::SelectAll,
        "terminal" => Act::Terminal,
        "window" => Act::NewWindow,
        "close" => Act::Close,
        "reload" => Act::Reload,
        "timeline" => Act::Timeline,
        "earlier" => Act::Step { earlier: true },
        "later" => Act::Step { earlier: false },
        "now" => Act::Now,
        "bring" => Act::Bring,
        "properties" => Act::Properties,
        _ => return None,
    })
}

/// `--set search <words>` and `--set meaning <words>`: the field opens with the words in it, and
/// the list follows, by name or by meaning.
fn search_for(state: &mut Files, id: window::Id, words: &str, meaning: bool) -> Task<Message> {
    let opened = open_search(state, id);
    let typed = search_typed(state, id, words.to_string());
    if !meaning {
        return Task::batch([opened, typed]);
    }
    Task::batch([opened, typed, search_entered(state, id)])
}

/// What a press on a disk in the sidebar starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doing {
    /// Ask for its passphrase, for a disk that is locked.
    Unlock,
    /// Mount it and go there.
    Mount,
    /// Unmount it.
    Unmount,
    /// Unmount everything on it and eject it.
    Eject,
}

/// Mount, unmount or eject a disk, on a thread of its own: udisks reads the file system before it
/// answers, and an unmount writes out everything that was waiting.
fn disk(state: &mut Files, id: window::Id, drive: &str, doing: Doing) -> Task<Message> {
    let Some(volume) = state.drive(drive) else {
        return Task::none();
    };
    if volume.locked || state.working.iter().any(|busy| busy == drive) {
        return Task::none();
    }
    state.working.push(drive.to_string());
    let name = drive.to_string();
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let done = match doing {
            // a locked disk is unlocked from its dialog, where the passphrase is typed
            Doing::Unlock | Doing::Mount => drives::mount(&name).map(Some),
            Doing::Unmount => drives::unmount(&name).map(|()| None),
            Doing::Eject => drives::eject(&name).map(|()| None),
        };
        let _ = sender.send(Message::Disk(Some(id), name, Box::new(done)));
    });
    Task::perform(receiver, move |said| said.unwrap_or(Message::CloseMenu(id)))
}

/// A disk was mounted, unmounted or ejected, or it was not. A disk that was just mounted opens in
/// the window that asked for it, and udisks is asked again at once, rather than waiting for it to
/// say something on its own.
pub fn disk_done(
    state: &mut Files,
    id: Option<window::Id>,
    drive: &str,
    done: Result<Option<PathBuf>, String>,
) -> Task<Message> {
    state.working.retain(|busy| busy != drive);
    let name = state
        .drive(drive)
        .map_or_else(|| "The disk".to_string(), |volume| volume.name.clone());
    let here = id.filter(|id| state.windows.contains_key(id));
    let mut tasks = vec![look_at_disks()];
    match (done, here) {
        (Ok(Some(mount)), Some(id)) => tasks.push(ui::go(state, id, Location::Folder(mount))),
        // unmounted and ejected: everything on it is written out, so it can be pulled out
        (Ok(None), Some(id)) => {
            tasks.push(toast(state, id, format!("{name} can be taken out."), None));
        }
        (Err(why), Some(id)) => tasks.push(toast(state, id, why, None)),
        (Err(why), None) => eprintln!("rift-files: {why}"),
        (Ok(_), None) => {}
    }
    Task::batch(tasks)
}

/// What is known about what is selected, in a dialog. What takes a moment to work out, the size
/// of a folder and how many pixels across a picture is, is worked out on a thread and fills itself
/// in when it comes.
fn properties(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get(&id) else {
        return Task::none();
    };
    let Some(facts) = crate::props::facts(state, browser) else {
        return Task::none();
    };
    let measuring = crate::props::measure(id, &facts);
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.menu = None;
        browser.dialog = Some(Dialog::Properties(Box::new(facts)));
    }
    measuring
}

/// What the Properties dialog was waiting to know.
pub fn measured(
    state: &mut Files,
    id: window::Id,
    size: String,
    pixels: Option<(u32, u32)>,
) -> Task<Message> {
    if let Some(browser) = state.windows.get_mut(&id)
        && let Some(Dialog::Properties(facts)) = browser.dialog.as_mut()
    {
        facts.size = Some(size);
        facts.pixels = pixels;
    }
    Task::none()
}

/// The dialog that asks for the passphrase of a locked disk.
fn ask_passphrase(state: &mut Files, id: window::Id, drive: &str) -> Task<Message> {
    let Some(volume) = state.drive(drive) else {
        return Task::none();
    };
    if !volume.locked {
        return Task::none();
    }
    let name = volume.name.clone();
    let number = state.windows.get(&id).map_or(1, |browser| browser.number);
    if let Some(browser) = state.windows.get_mut(&id) {
        browser.menu = None;
        browser.dialog = Some(Dialog::Unlock {
            drive: drive.to_string(),
            name,
            secret: String::new(),
            problem: None,
            working: false,
        });
    }
    crate::widgets::focus(field_id(number))
}

/// Unlock a disk with the passphrase that was typed and mount what comes out of it, on a thread of
/// its own: both calls go to udisks and each takes as long as the disk does.
fn unlock(state: &mut Files, id: window::Id) -> Task<Message> {
    let Some(browser) = state.windows.get_mut(&id) else {
        return Task::none();
    };
    let Some(Dialog::Unlock {
        drive,
        secret,
        working,
        ..
    }) = browser.dialog.as_mut()
    else {
        return Task::none();
    };
    if *working || secret.is_empty() {
        return Task::none();
    }
    *working = true;
    let (drive, secret) = (drive.clone(), secret.clone());
    let named = drive.clone();
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        // udisks may still be reading the file system that came out of it when it answers, so a
        // mount that finds nothing there yet is tried once more
        let done = drives::unlock(&drive, &secret).and_then(|inside| {
            drives::mount(&inside).or_else(|_| {
                thread::sleep(Duration::from_millis(500));
                drives::mount(&inside)
            })
        });
        let _ = sender.send(Message::Unlocked(id, drive, Box::new(done)));
    });
    Task::perform(receiver, move |said| {
        said.unwrap_or_else(|_| {
            Message::Unlocked(
                id,
                named.clone(),
                Box::new(Err("The disk service said nothing.".to_string())),
            )
        })
    })
}

/// A locked disk was unlocked and what came out of it mounted, or it was not: a passphrase that
/// does not open it leaves the dialog up with the sentence in it, so it can be typed again.
pub fn unlocked(
    state: &mut Files,
    id: window::Id,
    drive: &str,
    done: Result<PathBuf, String>,
) -> Task<Message> {
    state.working.retain(|busy| busy != drive);
    match done {
        Ok(mount) => {
            if let Some(browser) = state.windows.get_mut(&id) {
                browser.dialog = None;
            }
            Task::batch([look_at_disks(), ui::go(state, id, Location::Folder(mount))])
        }
        Err(why) => {
            if let Some(browser) = state.windows.get_mut(&id)
                && let Some(Dialog::Unlock {
                    problem, working, ..
                }) = browser.dialog.as_mut()
            {
                *problem = Some(why);
                *working = false;
                return Task::none();
            }
            toast(state, id, why, None)
        }
    }
}

/// Ask Vault whether the drive has an exchange partition, on a thread of its own. Vault reads the
/// drive's own table for it, so it is asked once and the answer is kept.
pub fn look_at_exchange() -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let there = librift::vault::exchange().unwrap_or_else(|why| {
            // a machine that is not a Rift drive has no Vault, and a sidebar with no row for a
            // partition that is not there is the right answer
            eprintln!("rift-files: {why}");
            false
        });
        let _ = sender.send(Message::Exchange(there));
    });
    Task::perform(receiver, |said| said.unwrap_or(Message::Exchange(false)))
}

/// Mount the drive's own exchange partition, on a thread of its own, and go to it when it is
/// there. Vault mounts it: udisks refuses the disk Rift is running from, and on a real stick it
/// would offer every partition of the drive instead of this one.
///
/// A Ghost session is the only one that needs it, since every other boot mounts it before anyone
/// logs in. Mounting it writes to the partition, which is why a Ghost boot leaves it to the owner.
fn mount_exchange(state: &mut Files, id: window::Id) -> Task<Message> {
    if let Some(path) = state.exchange.mount() {
        return ui::go(state, id, Location::Folder(path.to_path_buf()));
    }
    if state.working.iter().any(|busy| busy == EXCHANGE) {
        return Task::none();
    }
    state.working.push(EXCHANGE.to_string());
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(Message::Mounted(
            Some(id),
            Box::new(librift::vault::mount_exchange()),
        ));
    });
    Task::perform(receiver, move |said| said.unwrap_or(Message::CloseMenu(id)))
}

/// What names the exchange partition while it is being mounted, so the row is not pressed twice.
/// Nothing on the bus is called this, so it cannot be a disk's name.
const EXCHANGE: &str = "exchange";

/// The exchange partition was mounted, or it was not. It opens in the window that asked for it.
pub fn exchange_done(
    state: &mut Files,
    id: Option<window::Id>,
    done: Result<PathBuf, String>,
) -> Task<Message> {
    state.working.retain(|busy| busy != EXCHANGE);
    state.exchange = state.exchange.again();
    let here = id.filter(|id| state.windows.contains_key(id));
    match (done, here) {
        (Ok(folder), Some(id)) => ui::go(state, id, Location::Folder(folder)),
        (Err(why), Some(id)) => toast(state, id, why, None),
        (Err(why), None) => {
            eprintln!("rift-files: {why}");
            Task::none()
        }
        (Ok(_), None) => Task::none(),
    }
}

/// Ask udisks what is there now, on a thread of its own.
fn look_at_disks() -> Task<Message> {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        let _ = sender.send(Message::Drives(drives::volumes().unwrap_or_default()));
    });
    Task::perform(receiver, |said| {
        said.unwrap_or_else(|_| Message::Drives(Vec::new()))
    })
}

/// Go to a place of the sidebar by its word.
fn place(state: &mut Files, id: window::Id, word: &str) -> Task<Message> {
    if word == "trash" {
        return ui::go(state, id, Location::Trash);
    }
    if word == "exchange" {
        return match state.exchange {
            // mounted already, or there and waiting to be asked for, which is a Ghost session
            Exchange::At(_) | Exchange::There => mount_exchange(state, id),
            Exchange::None => Task::none(),
        };
    }
    if state.drives.iter().any(|drive| drive.name == word) {
        return by_name(state, id, word, Doing::Mount);
    }
    let path = state
        .places
        .iter()
        .find(|place| place.word == word)
        .map(|place| place.path.clone());
    path.map_or_else(Task::none, |path| ui::go(state, id, Location::Folder(path)))
}

/// Mount, unmount or eject the disk the sidebar calls this, which is how `--set drive`, `--set
/// unmount` and `--set eject` name one. A disk that is mounted already is opened instead.
fn by_name(state: &mut Files, id: window::Id, name: &str, doing: Doing) -> Task<Message> {
    let Some(drive) = state.drives.iter().find(|drive| drive.name == name) else {
        return Task::none();
    };
    let (drive_id, mount) = (drive.id.clone(), drive.mount.clone());
    match (doing, mount) {
        (Doing::Unlock, _) => ask_passphrase(state, id, &drive_id),
        (Doing::Mount, Some(mount)) => ui::go(state, id, Location::Folder(mount)),
        _ => disk(state, id, &drive_id, doing),
    }
}

/// Open a menu by its word, where a press in the middle of the list would open it.
fn set_menu(state: &mut Files, id: window::Id, word: &str) -> Task<Message> {
    let Some(which) = Which::from_word(word) else {
        return Task::none();
    };
    if which == Which::Main {
        return main_menu(state, id);
    }
    let top = state
        .windows
        .get(&id)
        .map_or(0.0, |browser| crate::view::list_top(state, browser));
    let grid = state.options.grid;
    let at = state.windows.get(&id).map(|browser| {
        let chosen = browser
            .cursor
            .as_ref()
            .and_then(|name| browser.position(name))
            .unwrap_or(0);
        // in the grid a row is a row of tiles, so the one a tile is in is where the menu opens
        let (which, tall) = if grid {
            (
                chosen / crate::grid::across(browser.size.width),
                crate::grid::TALL,
            )
        } else {
            (chosen, ROW)
        };
        #[allow(clippy::cast_precision_loss)]
        let row = (which as f32 + 0.5) * tall - browser.scroll;
        Point::new(crate::view::SIDEBAR + 240.0, top + row.max(0.0))
    });
    open_menu(state, id, which, at);
    Task::none()
}

/// Open the dialog for a new folder or a rename with a name typed in it, the way a person would.
fn typed_dialog(state: &mut Files, id: window::Id, name: &str, value: &str) -> Task<Message> {
    let act = if name == "new-folder" {
        Act::NewFolder
    } else {
        Act::Rename
    };
    let opened = self::act(state, id, act);
    if value != "now"
        && let Some(dialog) = state
            .windows
            .get_mut(&id)
            .and_then(|browser| browser.dialog.as_mut())
    {
        dialog.type_in(value.to_string());
    }
    opened
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, types: &[&str]) -> App {
        apps::parse(
            id,
            &format!(
                "[Desktop Entry]\nType=Application\nName={id}\nExec={id} %F\nMimeType={};\n",
                types.join(";")
            ),
        )
        .unwrap()
    }

    #[test]
    fn a_file_opens_with_the_default_for_its_kind_or_its_parents() {
        let types = mime::Database::parse(
            "50:text/x-python:*.py\n50:text/plain:*.txt\n",
            "text/x-python text/plain\n",
            "",
            "",
        );
        let all = vec![
            app("zed", &["text/plain"]),
            app("hx", &["text/plain", "text/x-python"]),
            app("dev.rift.Files", &["inode/directory"]),
        ];
        let found = Found {
            lists: vec!["[Default Applications]\ntext/plain=zed.desktop\n".to_string()],
            caches: Vec::new(),
        };
        // a type's own associations come first, the way xdg-mime and GLib look
        let opens = |kind: &str| {
            found
                .opener(kind, &all, &types, Some(ui::APP_ID))
                .map(|app| app.id)
        };
        assert_eq!(opens("text/x-python").as_deref(), Some("hx"));
        assert_eq!(opens("text/plain").as_deref(), Some("zed"));
        let rest: Vec<String> = others(&all, &types, "text/x-python", Some("hx"))
            .into_iter()
            .map(|app| app.id)
            .collect();
        assert_eq!(rest, ["zed"]);
        // Files opens folders and never a file
        assert!(opens("inode/directory").is_none());
    }
}
