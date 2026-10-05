//! The installs and the removes. Pressing Install or Remove puts the app in a queue and one worker
//! takes them one after another, since flatpak holds a lock on the installation and two at once
//! would only wait on each other. The worker runs on a thread of its own and says how far each one
//! has got. The window can close while it works: the app keeps running until the queue is empty.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;

use iced::Task;
use iced::futures::channel::mpsc;
use librift::flatpak;

use crate::ui::Message;

/// One app to install or to take off, and where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// The app's id.
    pub id: String,
    /// Where it comes from. Empty for a remove.
    pub remote: String,
    /// Whether it is to be taken off rather than installed.
    pub remove: bool,
}

/// How one install or remove is going.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Doing {
    /// In the queue behind another.
    Waiting,
    /// Running, this far out of a hundred.
    Running(u32),
    /// Done.
    Done,
    /// Stopped, and what flatpak said.
    Failed(String),
}

impl Doing {
    /// Whether it is still to finish.
    #[must_use]
    pub const fn pending(&self) -> bool {
        matches!(self, Self::Waiting | Self::Running(_))
    }

    /// What `--state` prints for it.
    #[must_use]
    pub fn word(&self) -> String {
        match self {
            Self::Waiting => "waiting".to_string(),
            Self::Running(percent) => format!("running {percent}"),
            Self::Done => "done".to_string(),
            Self::Failed(why) => format!("failed {why}"),
        }
    }
}

/// One install or remove the pages show, in the order they were asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    /// The app's id.
    pub id: String,
    /// Its name, as the page that asked knows it.
    pub name: String,
    /// Whether it is a remove.
    pub remove: bool,
    /// How it is going.
    pub doing: Doing,
}

impl Work {
    /// The verb for the row while it runs: what is being done, not what was asked.
    #[must_use]
    pub const fn verb(&self) -> &'static str {
        if self.remove {
            "Removing"
        } else {
            "Installing"
        }
    }
}

/// What the worker says about one job.
#[derive(Debug, Clone)]
pub enum Step {
    /// It has taken the app out of the queue and started flatpak.
    Started,
    /// It has got this far, out of a hundred.
    Moved(u32),
    /// It has finished, or why not.
    Finished(Result<(), String>),
}

/// The apps waiting, and whether a worker is taking them.
#[derive(Debug, Default)]
pub struct Queue {
    jobs: VecDeque<Job>,
    working: bool,
}

/// The queue, shared with the worker.
pub type Shared = Arc<Mutex<Queue>>;

/// Put a job in the queue, and start a worker when none is working. The task carries everything the
/// worker says; with a worker already running, its task does.
pub fn start(queue: &Shared, job: Job) -> Task<Message> {
    let Ok(mut held) = queue.lock() else {
        return Task::none();
    };
    held.jobs.push_back(job);
    if held.working {
        return Task::none();
    }
    held.working = true;
    drop(held);
    let (sender, receiver) = mpsc::unbounded();
    let queue = Arc::clone(queue);
    thread::spawn(move || work(&queue, &sender));
    Task::stream(receiver)
}

/// Do what is in the queue, one app at a time, until it is empty.
fn work(queue: &Shared, sender: &mpsc::UnboundedSender<Message>) {
    let say = |id: &str, step: Step| {
        let _ = sender.unbounded_send(Message::Doing(id.to_string(), step));
    };
    loop {
        let job = {
            let Ok(mut held) = queue.lock() else {
                return;
            };
            if let Some(job) = held.jobs.pop_front() {
                job
            } else {
                held.working = false;
                return;
            }
        };
        say(&job.id, Step::Started);
        let mut last = None;
        let moved = |progress: &flatpak::Progress| {
            let percent = progress.percent();
            if last != Some(percent) {
                last = Some(percent);
                say(&job.id, Step::Moved(percent));
            }
        };
        let done = if job.remove {
            flatpak::remove(&job.id, moved)
        } else {
            flatpak::install(&job.remote, &job.id, moved)
        };
        say(&job.id, Step::Finished(done));
    }
}

/// Tell the owner an app has finished installing while the window is closed, the way the shell
/// shows any notification.
pub fn tell(work: &Work, done: &Result<(), String>) {
    let name = &work.name;
    let (title, body) = match (done, work.remove) {
        (Ok(()), false) => (
            format!("{name} is installed"),
            "It is in the Applications menu.".to_string(),
        ),
        (Ok(()), true) => (
            format!("{name} is gone"),
            "It has been taken off.".to_string(),
        ),
        (Err(why), false) => (format!("{name} could not be installed"), why.clone()),
        (Err(why), true) => (format!("{name} could not be taken off"), why.clone()),
    };
    let _ = std::process::Command::new("notify-send")
        .args([
            "--app-name=Store",
            "--icon=system-software-install-symbolic",
            &title,
            &body,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_word_for_each_way_a_job_goes() {
        assert_eq!(Doing::Waiting.word(), "waiting");
        assert_eq!(Doing::Running(45).word(), "running 45");
        assert_eq!(Doing::Done.word(), "done");
        assert_eq!(
            Doing::Failed("No network.".to_string()).word(),
            "failed No network."
        );
        assert!(Doing::Waiting.pending() && Doing::Running(0).pending());
        assert!(!Doing::Done.pending() && !Doing::Failed(String::new()).pending());
    }

    #[test]
    fn a_row_says_what_is_being_done_to_the_app() {
        let row = |remove: bool| Work {
            id: "org.videolan.VLC".to_string(),
            name: "VLC".to_string(),
            remove,
            doing: Doing::Waiting,
        };
        assert_eq!(row(false).verb(), "Installing");
        assert_eq!(row(true).verb(), "Removing");
    }
}
