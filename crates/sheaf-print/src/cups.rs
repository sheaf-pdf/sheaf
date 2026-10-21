//! CUPS through its command-line tools, which every CUPS install has:
//! `lpstat` for the printers, `lpoptions` for their paper, `lp` to print,
//! the job on its stdin. `SHEAF_CUPS_BIN` names a directory to run them
//! from instead of `PATH` (for tests).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;

use anyhow::{Context, Result, bail};

use crate::{Duplex, JobSettings, Orientation, Paper, Printer, Sizing};

fn tool(name: &str) -> Command {
    match std::env::var_os("SHEAF_CUPS_BIN") {
        Some(dir) => Command::new(Path::new(&dir).join(name)),
        None => Command::new(name),
    }
}

fn run(mut cmd: Command) -> Result<String> {
    let what = format!("{cmd:?}");
    let out = cmd
        .env("LC_ALL", "C")
        .output()
        .with_context(|| format!("running {what} (is CUPS installed?)"))?;
    said(&what, out)
}

/// What the tool `what` printed, or what it said went wrong if it failed.
fn said(what: &str, out: Output) -> Result<String> {
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("{what} failed: {}", err.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn printers() -> Result<Vec<Printer>> {
    // `-e` lists destinations (printers and classes) by name; older CUPS
    // lacks it, so fall back to the accepting-requests list.
    let names = match run({
        let mut c = tool("lpstat");
        c.arg("-e");
        c
    }) {
        Ok(out) => parse_names(&out),
        Err(_) => {
            let mut c = tool("lpstat");
            c.arg("-a");
            parse_accepting(&run(c).unwrap_or_default())
        }
    };
    let default = std::env::var("LPDEST")
        .or_else(|_| std::env::var("PRINTER"))
        .ok()
        .or_else(|| {
            let mut c = tool("lpstat");
            c.arg("-d");
            run(c).ok().and_then(|out| parse_default(&out))
        });
    Ok(names
        .into_iter()
        .map(|name| {
            let (papers, default_paper) = {
                let mut c = tool("lpoptions");
                c.args(["-p", &name, "-l"]);
                match run(c) {
                    Ok(out) => parse_papers(&out),
                    Err(e) => {
                        log::warn!("paper sizes for {name}: {e:#}");
                        (Vec::new(), 0)
                    }
                }
            };
            let (papers, default_paper) = if papers.is_empty() {
                (vec![Paper::letter()], 0)
            } else {
                (papers, default_paper)
            };
            Printer {
                is_default: default.as_deref() == Some(name.as_str()),
                name,
                papers,
                default_paper,
            }
        })
        .collect())
}

pub fn submit(job: &JobSettings, pdf: Arc<[u8]>) -> Result<String> {
    send(tool("lp"), job, pdf)
}

/// Prints `pdf` with `lp`, run as `lp <options> -`: the file `-` is its
/// stdin, which gets the job from memory. A thread of its own writes it
/// while `lp`'s output is read, so neither waits on a full pipe.
fn send(mut lp: Command, job: &JobSettings, pdf: Arc<[u8]>) -> Result<String> {
    lp.args(lp_args(job)).arg("-");
    let what = format!("{lp:?}");
    let mut child = lp
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {what} (is CUPS installed?)"))?;
    let mut stdin = child.stdin.take().expect("piped");
    let writer = std::thread::spawn(move || stdin.write_all(&pdf));
    let out = child
        .wait_with_output()
        .with_context(|| format!("running {what}"))?;
    let wrote = writer
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e));
    // A failed lp says why; the write may have failed only for that.
    let out = said(&what, out)?;
    wrote.with_context(|| format!("sending the job to {what}"))?;
    Ok(out
        .lines()
        .find_map(|l| l.strip_prefix("request id is "))
        .and_then(|rest| rest.split_whitespace().next())
        .map(|id| format!("request {id}"))
        .unwrap_or_else(|| out.trim().to_string()))
}

/// `lp`'s options for a job (before `-`, the job on its stdin).
pub(crate) fn lp_args(job: &JobSettings) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-d".into(),
        job.printer.clone(),
        "-n".into(),
        job.copies.max(1).to_string(),
        "-t".into(),
        job.title.clone(),
        "-o".into(),
        format!("media={}", job.paper.id),
        "-o".into(),
        format!(
            "print-scaling={}",
            match job.sizing {
                Sizing::Fit => "fit",
                Sizing::ActualSize => "none",
                Sizing::ShrinkOversized => "auto",
            }
        ),
    ];
    if job.sizing == Sizing::Fit {
        // Older pdftopdf filters only know this one.
        a.extend(["-o".into(), "fit-to-page".into()]);
    }
    match job.orientation {
        // Left out, CUPS turns each page to suit, as the preview does.
        Orientation::Auto => {}
        Orientation::Portrait => a.extend(["-o".into(), "orientation-requested=3".into()]),
        Orientation::Landscape => a.extend(["-o".into(), "orientation-requested=4".into()]),
    }
    a.extend([
        "-o".into(),
        format!(
            "sides={}",
            match job.duplex {
                Duplex::Off => "one-sided",
                Duplex::LongEdge => "two-sided-long-edge",
                Duplex::ShortEdge => "two-sided-short-edge",
            }
        ),
    ]);
    if job.copies > 1 {
        a.extend([
            "-o".into(),
            format!("collate={}", if job.collate { "true" } else { "false" }),
        ]);
    }
    a
}

/// `lpstat -e`: one destination per line.
fn parse_names(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// `lpstat -a`: "name accepting requests since …".
fn parse_accepting(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(String::from)
        .collect()
}

/// `lpstat -d`: "system default destination: name".
fn parse_default(out: &str) -> Option<String> {
    out.lines()
        .find_map(|l| l.split_once("destination:"))
        .map(|(_, name)| name.trim().to_string())
        .filter(|n| !n.is_empty())
}

/// `lpoptions -p name -l`: the `PageSize` (or IPP `media`) line, e.g.
/// `PageSize/Media Size: *Letter Legal A4 Env10`; `*` marks the default.
fn parse_papers(out: &str) -> (Vec<Paper>, usize) {
    let line = out
        .lines()
        .find(|l| l.starts_with("PageSize/") || l.starts_with("PageSize:"))
        .or_else(|| {
            out.lines()
                .find(|l| l.starts_with("media/") || l.starts_with("media:"))
        });
    let Some((_, choices)) = line.and_then(|l| l.split_once(':')) else {
        return (Vec::new(), 0);
    };
    let mut papers = Vec::new();
    let mut default = 0;
    for choice in choices.split_whitespace() {
        let (is_default, id) = match choice.strip_prefix('*') {
            Some(id) => (true, id),
            None => (false, choice),
        };
        let Some((w, h)) = crate::known_paper(id).or_else(|| crate::ipp_media_size(id)) else {
            continue;
        };
        if is_default {
            default = papers.len();
        }
        papers.push(Paper {
            id: id.to_string(),
            label: crate::paper_label(id),
            width: w.min(h),
            height: w.max(h),
        });
    }
    (papers, default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lpstat() {
        assert_eq!(
            parse_names("Office\nHP_LaserJet\n\n"),
            ["Office", "HP_LaserJet"]
        );
        assert_eq!(
            parse_accepting("Office accepting requests since Mon 01 Jan\n"),
            ["Office"]
        );
        assert_eq!(
            parse_default("system default destination: HP_LaserJet\n").as_deref(),
            Some("HP_LaserJet")
        );
        assert_eq!(parse_default("no system default destination\n"), None);
    }

    #[test]
    fn parses_page_sizes_with_the_default() {
        let out = "PageSize/Media Size: Letter Legal *A4 Env10 Weird\nDuplex/2-Sided: *None DuplexNoTumble\n";
        let (papers, default) = parse_papers(out);
        let ids: Vec<&str> = papers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["Letter", "Legal", "A4", "Env10"]);
        assert_eq!(default, 2);
        // Driverless (IPP Everywhere) queues list media names with sizes.
        let out = "media/Media Size: *na_letter_8.5x11in iso_a4_210x297mm\n";
        let (papers, default) = parse_papers(out);
        assert_eq!(papers[0].label, "Letter");
        assert_eq!(papers[1].label, "A4");
        assert_eq!(default, 0);
    }

    #[test]
    fn lp_options_follow_the_settings() {
        let job = JobSettings {
            printer: "Office".into(),
            title: "Return.pdf".into(),
            copies: 2,
            collate: true,
            paper: Paper::letter(),
            sizing: Sizing::Fit,
            orientation: Orientation::Landscape,
            duplex: Duplex::LongEdge,
        };
        let a = lp_args(&job).join(" ");
        assert_eq!(
            a,
            "-d Office -n 2 -t Return.pdf -o media=Letter -o print-scaling=fit -o fit-to-page \
             -o orientation-requested=4 -o sides=two-sided-long-edge -o collate=true"
        );
        let one = JobSettings {
            copies: 1,
            sizing: Sizing::ShrinkOversized,
            orientation: Orientation::Auto,
            duplex: Duplex::Off,
            ..job
        };
        let a = lp_args(&one).join(" ");
        assert!(a.contains("print-scaling=auto"));
        assert!(!a.contains("orientation") && !a.contains("collate"));
    }

    /// `lp` reads the job on its stdin, `-` its last argument, so nothing
    /// goes to disk (M10.5 §10.6): a fake `lp` keeps its arguments and what
    /// it reads beside it and answers as CUPS does. The job is more than a
    /// pipe holds, so it's written while `lp` reads.
    #[test]
    fn lp_reads_the_job_from_stdin() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let lp = dir.path().join("lp");
        let script = "#!/bin/sh\ncd \"$(dirname \"$0\")\"\nprintf '%s\\n' \"$@\" > args\n\
                      cat > job\necho 'request id is fake-1 (1 file(s))'\n";
        std::fs::write(&lp, script).unwrap();
        std::fs::set_permissions(&lp, std::fs::Permissions::from_mode(0o755)).unwrap();
        let pdf: Vec<u8> = (b"%PDF-1.7\n".iter().copied())
            .chain((0..1 << 20).map(|i: u32| i as u8))
            .collect();
        let job = JobSettings {
            printer: "Office".into(),
            title: "Return.pdf".into(),
            copies: 1,
            collate: false,
            paper: Paper::letter(),
            sizing: Sizing::ShrinkOversized,
            orientation: Orientation::Auto,
            duplex: Duplex::Off,
        };
        let said = send(Command::new(&lp), &job, pdf.clone().into()).unwrap();
        assert_eq!(said, "request fake-1");
        assert!(std::fs::read(dir.path().join("job")).unwrap() == pdf);
        let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
        let mut want = lp_args(&job);
        want.push("-".into());
        assert_eq!(args.lines().collect::<Vec<_>>(), want);
    }
}
