//! Builds the hardware page of the website out of the reports under `hw/`.
//!
//! One report per machine Rift has booted on, written by `rift doctor --report` on that machine.
//! This reads the title, the table of facts and two of the sections out of each one and writes a
//! single page: a table with a row per machine, then a section per machine. The whole of a report
//! stays in the repository, and the page links to it there.
//!
//! The stylesheet is the guide's, built in, so the page is already a Rift page and the site of
//! P3.5 can drop it in. The page is one file so that nothing has to be copied beside it.
//!
//! It runs in CI over a directory that holds the template, the readme and no machine at all, so
//! nothing here needs a machine to exist.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "Usage: hw-site <directory> <page>";

const HELP: &str = "Reads the hardware reports in the directory, one markdown file per machine, \
and writes the page the website carries. The directory's README.md and TEMPLATE.md are not \
machines and are left out.";

/// The guide's one stylesheet, which this page is held to as well.
const STYLE: &str = include_str!("../../../nix/guide/style.css");

/// What the page adds to it: a third heading level for the sections of a report, and a first
/// column of a table that does not wrap.
const EXTRA: &str = "\nh3 {\n  font-size: 1rem;\n  margin: 1.2rem 0 0.3rem;\n}\n\n\
                     th {\n  white-space: nowrap;\n  padding-right: 1rem;\n}\n";

/// Where the reports live, for the link under each machine.
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// The files in the directory that are not a machine.
const NOT_MACHINES: [&str; 2] = ["README.md", "TEMPLATE.md"];

/// The fact every row of the table at the top of the page comes from.
const DATE: &str = "Date";
const VERSION: &str = "Rift version";
const MACHINE: &str = "Machine";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{USAGE}\n\n{HELP}");
            ExitCode::SUCCESS
        }
        [directory, page] => match build(Path::new(directory), Path::new(page)) {
            Ok(count) => {
                let machines = if count == 1 { "machine" } else { "machines" };
                println!("Wrote {page} from {count} {machines} in {directory}.");
                ExitCode::SUCCESS
            }
            Err(why) => {
                eprintln!("{why}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Reads every report in `directory` and writes the page. The count is how many machines it found.
fn build(directory: &Path, page: &Path) -> Result<usize, String> {
    let reports = read(directory)?;
    fs::write(page, html(&reports))
        .map_err(|e| format!("Could not write {}: {e}.", page.display()))?;
    Ok(reports.len())
}

/// Every report in the directory, in the order of their file names.
fn read(directory: &Path) -> Result<Vec<Report>, String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .map_err(|e| format!("Could not read {}: {e}.", directory.display()))?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_str()?.to_string();
            let markdown = Path::new(&name)
                .extension()
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case("md"));
            let machine = markdown && !NOT_MACHINES.contains(&name.as_str());
            machine.then_some(name)
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let path = directory.join(&name);
            let text = fs::read_to_string(&path)
                .map_err(|e| format!("Could not read {}: {e}.", path.display()))?;
            Report::read(&name, &text)
                .ok_or_else(|| format!("{} has no title line and no table of facts. A report is written by `rift doctor --report`; hw/TEMPLATE.md is its shape.", path.display()))
        })
        .collect()
}

/// One machine's report, as much of it as the page carries.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Report {
    /// The file name, which is also the link to it.
    file: String,
    /// The file name without the suffix, which is the anchor of its section.
    slug: String,
    /// The first heading: the maker and the model.
    title: String,
    /// The table at the top, in the order the file has it.
    facts: Vec<(String, String)>,
    /// The sentences under the Verdict heading.
    verdict: Vec<String>,
    /// The sentences under the heading that says what did not work.
    didnt: Vec<String>,
}

impl Report {
    /// Reads one. `None` when the file has no title or no facts, which is a file that is not a
    /// report at all.
    fn read(file: &str, text: &str) -> Option<Self> {
        let title = text
            .lines()
            .find_map(|line| line.strip_prefix("# "))?
            .trim()
            .to_string();
        let facts = facts(text);
        if title.is_empty() || facts.is_empty() {
            return None;
        }
        Some(Self {
            file: file.to_string(),
            slug: file.strip_suffix(".md").unwrap_or(file).to_string(),
            title,
            facts,
            verdict: section(text, "Verdict"),
            didnt: section(text, "What didn't"),
        })
    }

    /// One fact by its name, empty when the report has no such row.
    fn fact(&self, name: &str) -> &str {
        self.facts
            .iter()
            .find(|(key, _)| key == name)
            .map_or("", |(_, value)| value.as_str())
    }

    /// Whether the machine boots, which is the word after `Boots:` in the verdict.
    fn boots(&self) -> &str {
        self.verdict
            .iter()
            .find_map(|line| {
                let rest = line.split_once("Boots:")?.1.trim_start();
                Some(rest.split([' ', '.', ',']).next().unwrap_or(rest))
            })
            .unwrap_or("unknown")
    }
}

/// The rows of the markdown table at the top of a report. The line of dashes under the header is
/// not one, and neither is a row with nothing in it.
fn facts(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map_while(|line| {
            // the table ends where the first section begins
            (!line.starts_with("## ")).then_some(line.trim())
        })
        .filter(|line| line.starts_with('|'))
        .filter_map(|line| {
            // a value may hold a pipe of its own, so only the first one is a boundary
            let (key, value) = line.trim_matches('|').split_once('|')?;
            let (key, value) = (key.trim(), value.trim());
            (!key.is_empty() && !value.is_empty() && !key.starts_with("---"))
                .then(|| (key.to_string(), value.to_string()))
        })
        .collect()
}

/// The lines under one `##` heading, down to the next one. A blank line ends nothing: the lines
/// are the sentences, one per line, the way a report writes them.
fn section(text: &str, heading: &str) -> Vec<String> {
    text.lines()
        .skip_while(|line| line.trim_end() != format!("## {heading}"))
        .skip(1)
        .take_while(|line| !line.starts_with("## "))
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("```"))
        .map(ToString::to_string)
        .collect()
}

/// The page.
fn html(reports: &[Report]) -> String {
    let mut out = String::new();
    out.push_str(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>Rift on real hardware</title>\n<style>\n",
    );
    out.push_str(STYLE);
    out.push_str(EXTRA);
    out.push_str("</style>\n</head>\n<body>\n<main>\n<h1>Rift on real hardware</h1>\n");
    if reports.is_empty() {
        out.push_str(
            "<p>No machine has a report yet. A report is written by <code>rift doctor \
             --report</code> on the machine itself.</p>\n",
        );
    } else {
        let machines = if reports.len() == 1 {
            "One machine has".to_string()
        } else {
            format!("{} machines have", reports.len())
        };
        let _ = writeln!(
            out,
            "<p>{machines} a report so far. Each one was written by <code>rift doctor \
             --report</code> on the machine itself, and says what that machine is, how long it \
             took to start, and what did not work.</p>"
        );
        out.push_str(
            "<table>\n<tr><th>Machine</th><th>Date</th><th>Rift version</th><th>Boots</th></tr>\n",
        );
        for report in reports {
            let _ = writeln!(
                out,
                "<tr><td><a href=\"#{}\">{}</a></td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(&report.slug),
                escape(&report.title),
                escape(report.fact(DATE)),
                escape(report.fact(VERSION)),
                escape(report.boots())
            );
        }
        out.push_str("</table>\n");
        for report in reports {
            out.push_str(&machine(report));
        }
    }
    let _ = write!(
        out,
        "<footer><p>The reports are in <a href=\"{REPOSITORY}/tree/main/hw\">hw/</a> in Rift's \
         repository. To add one, run <code>rift doctor --report &gt; \
         hw/&lt;vendor&gt;-&lt;model&gt;.md</code> on the machine and open a pull \
         request.</p></footer>\n</main>\n</body>\n</html>"
    );
    out.push('\n');
    out
}

/// One machine's section: its facts, its verdict, what did not work, and where the whole report is.
fn machine(report: &Report) -> String {
    let mut out = format!(
        "<h2 id=\"{}\">{}</h2>\n<table>\n",
        escape(&report.slug),
        escape(&report.title)
    );
    for (key, value) in &report.facts {
        // the machine's own name heads the section, and the date is in the table at the top
        if key == MACHINE || key == DATE {
            continue;
        }
        let _ = writeln!(
            out,
            "<tr><th>{}</th><td>{}</td></tr>",
            escape(key),
            escape(value)
        );
    }
    out.push_str("</table>\n");
    for line in &report.verdict {
        let _ = writeln!(out, "<p>{}</p>", escape(line));
    }
    if !report.didnt.is_empty() {
        out.push_str("<h3>What didn't work</h3>\n");
        for line in &report.didnt {
            let _ = writeln!(out, "<p>{}</p>", escape(line));
        }
    }
    let _ = writeln!(
        out,
        "<p>The whole report is <a href=\"{REPOSITORY}/blob/main/hw/{0}\">hw/{0}</a>.</p>",
        escape(&report.file)
    );
    out
}

/// Text as it goes into the page. A report is written on a machine and names its devices, so
/// nothing in it is trusted to be html.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One report, as `rift doctor --report` writes it, cut to the parts the page reads.
    const REPORT: &str = "\
# QEMU Standard PC (Q35 + ICH9, 2009)

| | |
|---|---|
| Date | 2026-10-06 |
| Rift version | 0.1.0 |
| Machine | QEMU Standard PC (Q35 + ICH9, 2009), pc-q35-10.0, desktop |
| Firmware | SeaBIOS rel-1.17.0 | 02/06/2015, secure boot off |
| Display(s) | Virtual-1, 1280x800, 32x20 cm, 102 dpi, scale 1 |

## Verdict

Boots: yes. Power on to the login took 14.5 s: 3.1 s of kernel and 11.4 s of userspace.

## What worked

Every check of `rift doctor` on this machine:

```
Orbit   Passed   On the bus, host 5297c0f65d6a, class borrowed, AI tier small
| not a fact |

2 checks, 2 passed, 0 warnings, 0 failed.
```

## What didn't

One PCI device has no driver: bridge 8086:29c0.
Quasar warned: No chat model is on the drive.

## PCI

```
00:00.0 060000 8086:29c0 no driver
```
";

    fn work(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("hw-site-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_report_is_its_title_its_facts_and_two_of_its_sections() {
        let report = Report::read("qemu-standard-pc.md", REPORT).unwrap();
        assert_eq!(report.title, "QEMU Standard PC (Q35 + ICH9, 2009)");
        assert_eq!(report.slug, "qemu-standard-pc");
        assert_eq!(report.fact(DATE), "2026-10-06");
        assert_eq!(report.fact(VERSION), "0.1.0");
        assert_eq!(
            report.fact("Display(s)"),
            "Virtual-1, 1280x800, 32x20 cm, 102 dpi, scale 1"
        );
        assert_eq!(report.fact("Drive"), "");
        // the table stops at the first section, so a line of a listing is never a fact
        assert_eq!(report.facts.len(), 5);
        // and a value may hold a pipe of its own
        assert_eq!(
            report.fact("Firmware"),
            "SeaBIOS rel-1.17.0 | 02/06/2015, secure boot off"
        );
        assert_eq!(report.boots(), "yes");
        assert_eq!(report.verdict.len(), 1);
        assert_eq!(
            report.didnt,
            [
                "One PCI device has no driver: bridge 8086:29c0.",
                "Quasar warned: No chat model is on the drive.",
            ]
        );
    }

    #[test]
    fn a_file_that_is_not_a_report_is_not_one() {
        assert_eq!(Report::read("notes.md", "nothing here\n"), None);
        assert_eq!(
            Report::read("notes.md", "# A machine\n\nand no table.\n"),
            None
        );
        let no_verdict = "# A machine\n\n| | |\n|---|---|\n| Date | 2026-10-06 |\n";
        let report = Report::read("a-machine.md", no_verdict).unwrap();
        assert_eq!(report.boots(), "unknown");
        assert!(report.verdict.is_empty());
        assert!(report.didnt.is_empty());
    }

    #[test]
    fn the_template_and_the_readme_are_not_machines() {
        let dir = work("directory");
        fs::write(dir.join("README.md"), "# hw/\n\nOne file per machine.\n").unwrap();
        fs::write(
            dir.join("TEMPLATE.md"),
            "# <Vendor> <Model>\n\n| | |\n|---|---|\n| Date | |\n",
        )
        .unwrap();
        fs::write(dir.join("qemu-standard-pc.md"), REPORT).unwrap();
        fs::write(dir.join("notes.txt"), "not markdown\n").unwrap();
        fs::create_dir(dir.join("private")).unwrap();
        let reports = read(&dir).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].slug, "qemu-standard-pc");

        // a machine whose file is not a report says which file and what writes one
        fs::write(dir.join("broken.md"), "nothing here\n").unwrap();
        let why = read(&dir).unwrap_err();
        assert!(why.contains("broken.md"), "{why}");
        assert!(why.contains("rift doctor --report"), "{why}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_directory_with_no_machine_in_it_still_makes_a_page() {
        let dir = work("empty");
        let page = dir.join("hardware.html");
        assert_eq!(build(&dir, &page).unwrap(), 0);
        let said = fs::read_to_string(&page).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert!(said.contains("<h1>Rift on real hardware</h1>"), "{said}");
        assert!(said.contains("No machine has a report yet."), "{said}");
        assert!(!said.contains("<table>"), "{said}");
        // the stylesheet is the guide's, built in
        assert!(
            said.contains("font-family: \"Noto Sans\", sans-serif;"),
            "{said}"
        );
    }

    #[test]
    fn a_directory_that_is_not_there_says_so() {
        let why = read(Path::new("/nonexistent/hw")).unwrap_err();
        assert!(why.starts_with("Could not read /nonexistent/hw:"), "{why}");
    }

    #[test]
    fn the_page_is_a_row_and_a_section_per_machine() {
        let said = html(&[Report::read("qemu-standard-pc.md", REPORT).unwrap()]);
        assert!(said.contains("One machine has a report so far."), "{said}");
        for part in [
            "<tr><th>Machine</th><th>Date</th><th>Rift version</th><th>Boots</th></tr>",
            "<tr><td><a href=\"#qemu-standard-pc\">QEMU Standard PC (Q35 + ICH9, 2009)</a></td>\
             <td>2026-10-06</td><td>0.1.0</td><td>yes</td></tr>",
            "<h2 id=\"qemu-standard-pc\">QEMU Standard PC (Q35 + ICH9, 2009)</h2>",
            "<tr><th>Rift version</th><td>0.1.0</td></tr>",
            "<p>Boots: yes. Power on to the login took 14.5 s: 3.1 s of kernel and 11.4 s of \
             userspace.</p>",
            "<h3>What didn't work</h3>",
            "<p>One PCI device has no driver: bridge 8086:29c0.</p>",
            "blob/main/hw/qemu-standard-pc.md\">hw/qemu-standard-pc.md</a>",
        ] {
            assert!(said.contains(part), "{part} is not in\n{said}");
        }
        // the machine's own row heads its section, and the date is in the table at the top
        assert!(!said.contains("<tr><th>Machine</th><td>"), "{said}");
        assert!(!said.contains("<tr><th>Date</th><td>"), "{said}");
    }

    #[test]
    fn nothing_a_machine_said_about_itself_is_html() {
        let text = REPORT.replace(
            "| Display(s) | Virtual-1",
            "| Display(s) | <script>alert(\"x\")</script> & Virtual-1",
        );
        let said = html(&[Report::read("a-machine.md", &text).unwrap()]);
        assert!(
            said.contains(
                "<td>&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; Virtual-1, 1280x800"
            ),
            "{said}"
        );
        assert!(!said.contains("<script>"), "{said}");
    }
}
