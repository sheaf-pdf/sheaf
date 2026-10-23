//! This process's place among sheaf processes, through instance-handoff:
//! the one owner per user and state folder opens every window and alone
//! writes sheaf's state; a later launch hands its files over and exits.
//! What's sheaf's here is when a launch joins (its flags), what it hands
//! over (its files) and what the owner does with them. The lock, the
//! socket, the folders and the checks on both ends are the crate's.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{App, Global};
use instance_handoff::{AppId, Args, Builder, Claim, End, Exclusive, Owner, Request, Started};

/// sheaf's app id, as its Wayland app id and desktop file have it. It names
/// the runtime folder (`/run/user/<uid>/sheaf` on Linux), so keep it: two
/// builds with two ids would each become an owner.
const APP: AppId = AppId::windowed("sheaf");

/// How long a launch keeps trying an owner that holds the lock but doesn't
/// take its files (still starting up, or quitting) before running alone.
const WAIT: Duration = Duration::from_secs(3);

/// How a launch joins the running sheaf: the app's choice, from its flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Become the owner, or hand our files to it.
    Join,
    /// Hand nothing over: hold sheaf's state if nobody does, taking no other
    /// launch's files, and otherwise run alone, writing no state. For
    /// `--new-instance`, the dev flags and debug builds.
    Separate,
}

static OWNS_STATE: AtomicBool = AtomicBool::new(true);

/// Whether this process may write sheaf's state (`state.json`, recent
/// files). Only the owner does; a process running alone beside it writes
/// nothing, and an owner that lost its lock or its state folder stops.
pub fn owns_state() -> bool {
    OWNS_STATE.load(Ordering::Relaxed)
}

/// What a launch holds, decided before GPUI starts.
enum Held {
    /// The lock, to serve once the app can open what comes.
    Claim(Claim<Args>),
    /// The state, serving nobody ([`Mode::Separate`]).
    Exclusive(Exclusive),
}

/// What this process holds for its life: the owner it serves as, or the
/// state it holds alone. An [`Exclusive`] is released when dropped, so it
/// lives here until the process exits.
struct Instance {
    owner: Option<Owner>,
    _state: Option<Exclusive>,
}
impl Global for Instance {}

/// Decided before GPUI starts.
pub struct Launch {
    held: Option<Held>,
    /// Files from later launches (and, on macOS, from Finder), absolute.
    requests: UnboundedReceiver<Vec<PathBuf>>,
    pub sink: UnboundedSender<Vec<PathBuf>>,
}

/// The builder every launch uses. The state is the folder sheaf writes, so
/// a scratch config (`SHEAF_CONFIG_DIR`, as tests and scripts use) moves it
/// and gets an owner of its own.
fn builder() -> Builder {
    Builder::new(APP, sheaf_ops::dirs::state_dir()).timeout(WAIT)
}

/// What a launch hands over: its files, made absolute against its own
/// working directory, since the owner's isn't ours.
fn request(paths: &[PathBuf]) -> Args {
    paths
        .iter()
        .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()))
        .collect()
}

/// The files a launch handed over, as paths the owner can open: any still
/// relative is joined to the launch's working directory, not the owner's.
fn files(request: &Request<Args>) -> Vec<PathBuf> {
    request
        .payload()
        .iter()
        .filter_map(|arg| request.resolve(arg))
        .collect()
}

/// The owner's handler, on the crate's thread: queue the files for the app
/// and answer at once. A closed queue means the app is on its way out, so
/// the launch is told the owner is busy and waits for the lock instead.
fn handler(
    sink: UnboundedSender<Vec<PathBuf>>,
) -> impl FnMut(Request<Args>) -> bool + Send + 'static {
    move |request| sink.unbounded_send(files(&request)).is_ok()
}

/// Whether an owner that ended this way still owns sheaf's state. One that
/// lost its lock or its state folder to another process must stop writing.
fn still_owns(end: End) -> bool {
    !matches!(end, End::LostLock | End::LostState)
}

/// Run alone, writing no state, and say why.
fn alone(mode: Mode, why: impl std::fmt::Display) {
    OWNS_STATE.store(false, Ordering::Relaxed);
    log::warn!("running alone, keeping no settings: {why}");
    if mode == Mode::Join {
        eprintln!("sheaf: running alone, keeping no settings: {why}");
    }
}

/// Join the running sheaf, or become it. Exits the process when another
/// sheaf took `paths`.
pub fn start(mode: Mode, paths: &[PathBuf]) -> Launch {
    let (sink, requests) = unbounded();
    let b = builder();
    let held = match mode {
        Mode::Join => match b.start(&request(paths)) {
            Started::Owner(claim) => {
                if let Some(e) = claim.listen_error() {
                    // Still the owner (the only state writer); launches just
                    // can't reach us, and run alone.
                    log::warn!("sheaf instance socket in {}: {e}", claim.dir().display());
                }
                Some(Held::Claim(claim))
            }
            Started::HandedOff(owner) => {
                if !owner.confirmed() {
                    log::warn!("the running sheaf ({owner}) didn't confirm it took the files");
                }
                std::process::exit(0)
            }
            Started::Alone(why) => {
                alone(mode, why);
                None
            }
        },
        Mode::Separate => match b.exclusive() {
            Ok(state) => Some(Held::Exclusive(state)),
            Err(why) => {
                alone(mode, why);
                None
            }
        },
    };
    Launch {
        held,
        requests,
        sink,
    }
}

impl Launch {
    /// Hold the lock for the app's life and open what later launches send.
    pub fn serve(self, cx: &mut App) {
        let (owner, state) = match self.held {
            Some(Held::Claim(claim)) => {
                let owner = claim
                    .on_end(|end| {
                        if !still_owns(end) {
                            OWNS_STATE.store(false, Ordering::Relaxed);
                            log::warn!("no longer sheaf's owner ({end}): keeping no settings");
                        }
                    })
                    .serve(handler(self.sink.clone()));
                (Some(owner), None)
            }
            Some(Held::Exclusive(state)) => (None, Some(state)),
            None => (None, None),
        };
        cx.set_global(Instance {
            owner,
            _state: state,
        });
        let mut requests = self.requests;
        cx.spawn(async move |cx| {
            while let Some(paths) = requests.next().await {
                cx.update(|cx| crate::workspace::open_from_launch(paths, cx));
            }
        })
        .detach();
    }
}

/// Quit: tell later launches to wait for the lock rather than hand files
/// to a process on its way out, and write the state's last changes.
pub fn quit(cx: &mut App) {
    if let Some(Instance {
        owner: Some(owner), ..
    }) = cx.try_global::<Instance>()
    {
        owner.stop();
    }
    crate::state::flush(cx);
    cx.quit();
}

/// A `file://` URL as a path (macOS hands opened files over as these).
pub fn file_url_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    // An empty host or "localhost"; anything else is another machine.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1), bytes.get(i + 2)) {
            (b'%', Some(&h), Some(&l)) if hex(h).is_some() && hex(l).is_some() => {
                out.push((hex(h)? * 16 + hex(l)?) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_decode() {
        assert_eq!(
            file_url_path("file:///Users/g/My%20Binder.pdf"),
            Some(PathBuf::from("/Users/g/My Binder.pdf"))
        );
        assert_eq!(
            file_url_path("file://localhost/tmp/a.pdf"),
            Some(PathBuf::from("/tmp/a.pdf"))
        );
        assert_eq!(file_url_path("file://server/share/a.pdf"), None);
        assert_eq!(file_url_path("https://example.com/a.pdf"), None);
        // A stray percent is kept as it is.
        assert_eq!(
            file_url_path("file:///tmp/100%.pdf"),
            Some(PathBuf::from("/tmp/100%.pdf"))
        );
    }

    /// An absolute path, on every platform.
    fn abs(name: &str) -> PathBuf {
        std::env::current_dir().unwrap().join(name)
    }

    #[test]
    fn a_launch_hands_over_its_files_made_absolute() {
        let sent = request(&[PathBuf::from("two.pdf"), abs("three.pdf")]);
        let sent: Vec<PathBuf> = sent.iter().map(PathBuf::from).collect();
        assert_eq!(sent, vec![abs("two.pdf"), abs("three.pdf")]);
    }

    #[test]
    fn the_owner_opens_relative_files_against_the_launchs_folder() {
        let launched_in = abs("launched-in");
        let req = Request::local(Args::new([PathBuf::from("a.pdf"), abs("b.pdf")]))
            .with_cwd(Some(launched_in.clone()));
        assert_eq!(files(&req), vec![launched_in.join("a.pdf"), abs("b.pdf")]);
        // A relative file from a launch with no folder it could name isn't
        // opened against ours.
        let req = Request::local(Args::new([PathBuf::from("a.pdf")])).with_cwd(None);
        assert!(files(&req).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_file_name_that_isnt_utf8_survives() {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::OsStr::from_bytes(b"caf\xe9.pdf");
        let got = files(&Request::local(request(&[PathBuf::from(name)])));
        assert_eq!(got, vec![std::env::current_dir().unwrap().join(name)]);
    }

    #[test]
    fn the_owner_queues_what_launches_send() {
        let (tx, mut rx) = unbounded();
        let mut take = handler(tx);
        assert!(take(Request::local(request(&[PathBuf::from("one.pdf")]))));
        assert_eq!(rx.try_recv().unwrap(), vec![abs("one.pdf")]);
        // With no files, the frontmost window comes forward.
        assert!(take(Request::local(Args::default())));
        assert_eq!(rx.try_recv().unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn an_owner_on_its_way_out_has_launches_wait() {
        let (tx, rx) = unbounded();
        let mut take = handler(tx);
        // The app has gone (its receiver dropped): busy, so the launch
        // keeps trying the lock rather than handing its files to nobody.
        drop(rx);
        assert!(!take(Request::local(request(&[PathBuf::from("x.pdf")]))));
    }

    #[test]
    fn an_owner_that_lost_its_lock_or_state_stops_writing() {
        assert!(still_owns(End::Stopped));
        assert!(still_owns(End::Panicked));
        assert!(!still_owns(End::LostLock));
        assert!(!still_owns(End::LostState));
    }
}
