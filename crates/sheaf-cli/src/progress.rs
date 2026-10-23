//! Drawing a `Tracker` as a bar on stderr while an op runs, and Ctrl-C.
//! A stderr that isn't a terminal gets no bar, but plain lines: one as
//! each op starts, and one per message.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use in_progress::{Level, Snapshot, Tracker};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

/// The op Ctrl-C cancels.
static CURRENT: Mutex<Option<Tracker>> = Mutex::new(None);

/// Run `work` with a fresh tracker, drawing it until `work` returns.
/// The first Ctrl-C cancels the tracker; a second one exits at once.
pub fn run<T: Send>(
    label: &str,
    quiet: bool,
    work: impl FnOnce(&Tracker) -> anyhow::Result<T> + Send,
) -> anyhow::Result<T> {
    install_ctrlc();
    let tracker = Tracker::new();
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(tracker.clone());
    let bar = if quiet {
        ProgressBar::hidden()
    } else {
        ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr())
    };
    bar.set_style(
        ProgressStyle::with_template("{msg:>12} [{bar:30}] {pos}/{len} {prefix}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=> "),
    );
    bar.set_message(label.to_string());
    let plain = !quiet && bar.is_hidden();
    if plain {
        eprintln!("{label}…");
    }
    let done = AtomicBool::new(false);
    let result = std::thread::scope(|s| {
        s.spawn(|| {
            let mut last_message = None;
            while !done.load(Ordering::Relaxed) {
                draw(&bar, plain, &tracker.snapshot(), &mut last_message);
                std::thread::sleep(Duration::from_millis(80));
            }
            draw(&bar, plain, &tracker.snapshot(), &mut last_message);
        });
        let result = work(&tracker);
        done.store(true, Ordering::Relaxed);
        result
    });
    bar.finish_and_clear();
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = None;
    result
}

/// `snap` on `bar`, its message as a line of its own: above the bar, or
/// when `plain`, alone.
fn draw(
    bar: &ProgressBar,
    plain: bool,
    snap: &Snapshot,
    last_message: &mut Option<(Level, String)>,
) {
    if let Some(total) = snap.total {
        bar.set_length(total);
        bar.set_position(snap.done.min(total));
        bar.set_prefix(snap.unit.noun());
    }
    if snap.message != *last_message {
        if let Some((level, msg)) = &snap.message {
            let tag = match level {
                Level::Info => "",
                Level::Warn => "warning: ",
                Level::Error => "error: ",
            };
            match plain {
                true => eprintln!("{tag}{msg}"),
                false => bar.println(format!("{tag}{msg}")),
            }
        }
        last_message.clone_from(&snap.message);
    }
}

fn install_ctrlc() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let result = ctrlc::set_handler(|| {
            let current = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
            match current.as_ref() {
                // Cancelled, then said: what reads the line knows it is.
                Some(t) if !t.snapshot().cancelled => {
                    t.cancel();
                    eprintln!("\nstopping… (Ctrl-C again to quit now)");
                }
                _ => std::process::exit(130),
            }
        });
        if let Err(e) = result {
            log::warn!("can't catch Ctrl-C: {e}");
        }
    });
}
