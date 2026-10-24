//! The wasmtime side: one shared engine, one worker thread and `Store` per
//! plugin, and the host half of the WIT interfaces.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow};
use arc_swap::ArcSwap;
use futures::channel::oneshot;
use sheaf_doc::{PageId, Published, Snapshot, TextService, Tx, ViewInputs};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap, UpdateDeadline};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use crate::api;
use crate::change::ChangeInfo;
use crate::landing::Committer;
use crate::manifest::{Grants, Manifest, WASM_NAME};
use crate::storage::Storage;
use crate::ui::Ui;

/// How often the epoch advances; timeouts are measured in these.
const TICK: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Longest a single call (activate or a command) may run.
    pub call_timeout: Duration,
    /// Most linear memory a plugin may grow to.
    pub memory_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            call_timeout: Duration::from_secs(30),
            memory_bytes: 256 << 20,
        }
    }
}

/// Runs OCR for plugins. The app implements it over `sheaf_ocr`.
pub trait OcrSource: Send + Sync {
    /// Recognize page `page` of `snap`, as the call reads the document, as
    /// it shows (`Snapshot::geom`), from its content alone: its words in
    /// PageSpace.
    fn recognize(
        &self,
        snap: &Snapshot,
        page: PageId,
        language: Option<String>,
        dpi: Option<u32>,
    ) -> Result<sheaf_doc::TextLayer>;
}

/// A document event, for plugins that subscribe to it.
#[derive(Debug, Clone, PartialEq)]
pub enum DocEvent {
    Opened,
    /// What changed, and why (M10.5 §14.3).
    Changed(ChangeInfo),
    /// The path saved to.
    Saved(String),
    SelectionChanged,
    Closed,
    /// A region on a page, in PageSpace.
    Region {
        page: PageId,
        rect: sheaf_doc::Box2,
    },
    RegionCleared,
    /// A call's edits didn't land, and why ([`follow_ups`]).
    ///
    /// [`follow_ups`]: crate::landing::follow_ups
    EditRefused(String),
}

impl DocEvent {
    pub fn kind(&self) -> crate::manifest::EventKind {
        use crate::manifest::EventKind;
        match self {
            DocEvent::Opened => EventKind::Opened,
            DocEvent::Changed(_) => EventKind::Changed,
            DocEvent::Saved(_) => EventKind::Saved,
            DocEvent::SelectionChanged => EventKind::SelectionChanged,
            DocEvent::Closed => EventKind::Closed,
            DocEvent::Region { .. } | DocEvent::RegionCleared => EventKind::Region,
            DocEvent::EditRefused(_) => EventKind::EditRefused,
        }
    }
}

/// How to start a plugin beyond its manifest.
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    /// Where its `storage` lives; None keeps it in memory.
    pub data_dir: Option<PathBuf>,
    /// The permissions the user granted (a subset of the manifest's).
    pub grants: Grants,
}

/// A pattern-search preset, for `patterns.presets`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub name: String,
    pub regex: String,
}

/// What a call can see and reach.
#[derive(Clone, Default)]
pub struct CallContext {
    /// None when no document is open.
    pub doc: Option<DocContext>,
    /// Where `ui` calls go. None: forms are cancelled, notices logged,
    /// panel updates dropped.
    pub ui: Option<Arc<dyn Ui>>,
    /// The user's pattern presets, then plugin-shipped ones.
    pub presets: Vec<Preset>,
    /// Where the plugin's `progress` tasks report, and how the call is
    /// cancelled. None: tasks report nowhere and are never cancelled.
    pub progress: Option<in_progress::Tracker>,
    /// The plugin's section of the user's `settings.toml`, as JSON
    /// (`storage.preferences`), taken when the call starts so it stays the
    /// same throughout. Empty for none.
    pub preferences: String,
    /// The installed plugins' ids: a mark one of them made is told as
    /// theirs, by its role (`plugin <id>`).
    pub plugins: Arc<[String]>,
}

/// One entry point into a plugin.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Command {
        id: String,
        args: Option<String>,
    },
    PanelAction {
        panel: String,
        row: String,
        action: String,
    },
    Event(DocEvent),
}

impl std::fmt::Display for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Call::Command { id, .. } => write!(f, "command {id:?}"),
            Call::PanelAction { panel, action, .. } => {
                write!(f, "action {action:?} in panel {panel:?}")
            }
            Call::Event(e) => write!(f, "event {e:?}"),
        }
    }
}

#[derive(Clone)]
pub struct DocContext {
    /// The document's file; None when it has no file yet.
    pub path: Option<PathBuf>,
    /// The document as it is now, which the app publishes at every change:
    /// a call makes its transaction on it at its first read or submit
    /// (M10.5 §14.3), so one that waited in the queue, or on a form, reads
    /// the document as it is by then.
    pub cell: Arc<ArcSwap<Published>>,
    /// In document order.
    pub selection: Vec<PageId>,
    pub current_page: Option<PageId>,
    /// Pages' text, each read from what it draws (M10.5 §10.5).
    pub text: Arc<dyn TextService>,
    /// None: `ocr.recognize` reports that OCR isn't available.
    pub ocr: Option<Arc<dyn OcrSource>>,
    /// None: `render` reports that rendering isn't available.
    pub render: Option<Arc<dyn crate::render::RenderSource>>,
    /// Where a call's `edits.commit` lands its edits before it returns.
    /// None: it can't, and says so.
    pub committer: Option<Arc<dyn Committer>>,
}

/// What a call left to land ([`Landing::finish`]).
///
/// [`Landing::finish`]: crate::landing::Landing::finish
#[derive(Default)]
pub struct CallOutput {
    /// Its edits since its last commit, in the plugin's transaction, made
    /// on the cell's document at its first read or submit, or on the one
    /// its last commit made; what it read there recorded in it as decision
    /// reads (M10.5 §14.3). None when it submitted none.
    pub tx: Option<Tx>,
    /// What its edits are called: its first submit's label.
    pub label: Option<String>,
    /// Why its edits were refused, each told once: it answered an undo, or
    /// another plugin's answer to a change. Never why an answer to
    /// `edit-refused` was, which its submits told it.
    pub refused: Vec<String>,
    /// How far down a chain of answers its edits are: one deeper than the
    /// change a `changed` event told of, else 0.
    pub depth: u8,
}

impl std::fmt::Debug for CallOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallOutput")
            .field("tx", &self.tx.as_ref().map(|tx| tx.base()))
            .field("label", &self.label)
            .field("refused", &self.refused)
            .field("depth", &self.depth)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginError {
    /// The plugin returned an error; it stays loaded.
    Failed(String),
    /// The plugin trapped (crashed, ran out of time or memory) and has been
    /// unloaded.
    Trapped(String),
    /// The plugin isn't running: it failed to load or trapped earlier.
    Unavailable(String),
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginError::Failed(m) => write!(f, "{m}"),
            PluginError::Trapped(m) => write!(f, "crashed and was unloaded: {m}"),
            PluginError::Unavailable(m) => write!(f, "not running: {m}"),
        }
    }
}

impl std::error::Error for PluginError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Loading,
    Ready,
    /// Failed to load, or trapped; the reason.
    Failed(String),
}

/// The shared engine. Create one per process.
pub struct Runtime {
    engine: Engine,
    linker: Linker<HostState>,
    limits: Limits,
    /// The epoch ticker runs while this is alive.
    _ticker: Arc<()>,
}

impl Runtime {
    pub fn new(limits: Limits) -> Result<Arc<Self>> {
        let mut config = Config::new();
        config.wasm_component_model(true).epoch_interruption(true);
        let engine = Engine::new(&config).map_err(anyhow::Error::from)?;
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(anyhow::Error::from)?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_sync(&mut linker)
            .map_err(anyhow::Error::from)?;
        api::World::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)
            .map_err(anyhow::Error::from)?;
        let token = Arc::new(());
        let alive = Arc::downgrade(&token);
        let ticking = engine.clone();
        thread::Builder::new()
            .name("plugin-epoch".into())
            .spawn(move || {
                while alive.upgrade().is_some() {
                    thread::sleep(TICK);
                    ticking.increment_epoch();
                }
            })?;
        Ok(Arc::new(Self {
            engine,
            linker,
            limits,
            _ticker: token,
        }))
    }

    /// Start a plugin: its thread compiles and instantiates `plugin.wasm`
    /// from `dir` and calls `activate`. Returns at once; calls made before
    /// loading finishes wait for it.
    pub fn load(self: &Arc<Self>, dir: PathBuf, manifest: Manifest, opts: LoadOptions) -> Plugin {
        let (tx, rx) = mpsc::channel::<Request>();
        let status = Arc::new(Mutex::new(Status::Loading));
        let (rt, st, who) = (self.clone(), status.clone(), manifest.clone());
        let spawned = thread::Builder::new()
            .name(format!("plugin-{}", manifest.id))
            .spawn(move || worker(rt, dir, who, opts, st, rx));
        if let Err(e) = spawned {
            *status.lock().unwrap() = Status::Failed(format!("starting its thread: {e}"));
        }
        Plugin {
            manifest,
            tx,
            status,
        }
    }
}

struct Request {
    call: Call,
    ctx: CallContext,
    reply: oneshot::Sender<Result<CallOutput, PluginError>>,
}

/// A loaded (or loading) plugin. Dropping it stops its thread once queued
/// calls finish.
pub struct Plugin {
    manifest: Manifest,
    tx: mpsc::Sender<Request>,
    status: Arc<Mutex<Status>>,
}

impl Plugin {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    /// Queue a command. The reply arrives when it finishes.
    pub fn run_command(
        &self,
        command: &str,
        args: Option<String>,
        ctx: CallContext,
    ) -> oneshot::Receiver<Result<CallOutput, PluginError>> {
        self.call(
            Call::Command {
                id: command.to_string(),
                args,
            },
            ctx,
        )
    }

    /// Queue a call. The reply arrives when it finishes.
    pub fn call(
        &self,
        call: Call,
        ctx: CallContext,
    ) -> oneshot::Receiver<Result<CallOutput, PluginError>> {
        let (reply, rx) = oneshot::channel();
        let req = Request { call, ctx, reply };
        if let Err(mpsc::SendError(req)) = self.tx.send(req) {
            let why = match self.status() {
                Status::Failed(m) => m,
                _ => "its thread has stopped".into(),
            };
            let _ = req.reply.send(Err(PluginError::Unavailable(why)));
        }
        rx
    }
}

fn worker(
    rt: Arc<Runtime>,
    dir: PathBuf,
    manifest: Manifest,
    opts: LoadOptions,
    status: Arc<Mutex<Status>>,
    rx: mpsc::Receiver<Request>,
) {
    let set = |s: Status| *status.lock().unwrap() = s;
    let id = manifest.id.clone();
    let mut inst = match Instance::start(&rt, &dir, manifest, opts) {
        Ok(i) => {
            set(Status::Ready);
            i
        }
        Err(e) => {
            let msg = format!("{e:#}");
            log::error!("plugin {id}: {msg}");
            set(Status::Failed(msg.clone()));
            for req in rx {
                let _ = req.reply.send(Err(PluginError::Unavailable(msg.clone())));
            }
            return;
        }
    };
    let mut dead: Option<String> = None;
    for req in rx {
        if let Some(why) = &dead {
            let _ = req.reply.send(Err(PluginError::Unavailable(why.clone())));
            continue;
        }
        let result = inst.run(&req.call, req.ctx);
        if let Err(PluginError::Trapped(m)) = &result {
            log::error!("plugin {id} trapped in {}: {m}", req.call);
            set(Status::Failed(m.clone()));
            dead = Some(m.clone());
        }
        let _ = req.reply.send(result);
        if dead.is_some() {
            // Free the instance's memory now rather than at shutdown.
            inst.unload();
        }
    }
}

struct Instance {
    store: Option<Store<HostState>>,
    world: Option<api::World>,
    timeout: Duration,
}

impl Instance {
    fn start(rt: &Runtime, dir: &std::path::Path, m: Manifest, opts: LoadOptions) -> Result<Self> {
        let wasm = dir.join(WASM_NAME);
        let component = Component::from_file(&rt.engine, &wasm)
            .map_err(anyhow::Error::from)
            .with_context(|| format!("loading {}", wasm.display()))?;
        let wasi = wasi_ctx(&opts.grants)?;
        let credit = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let state = HostState {
            wasi,
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(rt.limits.memory_bytes)
                .build(),
            plugin: m.id,
            version: m.version.to_string().into(),
            edit_schema: m.edit_schema,
            identity: opts.grants.identity,
            panels: m.panels.into_iter().map(|p| p.id).collect(),
            storage: Storage::new(opts.data_dir),
            http: wasmtime_wasi_http::WasiHttpCtx::new(),
            net: NetHooks {
                grants: opts.grants,
                credit: credit.clone(),
            },
            credit,
            render_handles: Default::default(),
            task_handles: Vec::new(),
            call: None,
            call_tx: None,
            base: None,
            label: None,
            uploads: Vec::new(),
            refusal: None,
            refused: Vec::new(),
            depth: 0,
            deadline: Instant::now() + rt.limits.call_timeout,
        };
        let mut store = Store::new(&rt.engine, state);
        store.limiter(|s| &mut s.limits);
        // Checked every tick against a wall-clock deadline, which host calls
        // that wait for the user push back.
        store.epoch_deadline_callback(|cx| {
            let d = cx.data();
            let waited = Duration::from_nanos(d.credit.load(std::sync::atomic::Ordering::Relaxed));
            Ok(if Instant::now() < d.deadline + waited {
                UpdateDeadline::Continue(1)
            } else {
                UpdateDeadline::Interrupt
            })
        });
        store.set_epoch_deadline(1);
        let world = api::World::instantiate(&mut store, &component, &rt.linker)
            .map_err(anyhow::Error::from)
            .context("instantiating")?;
        store.data_mut().deadline = Instant::now() + rt.limits.call_timeout;
        store.set_epoch_deadline(1);
        world
            .call_activate(&mut store)
            .map_err(|e| anyhow!(describe_trap(&e, rt.limits.call_timeout)))
            .context("activate")?
            .map_err(|e| anyhow!("activate: {e}"))?;
        Ok(Self {
            store: Some(store),
            world: Some(world),
            timeout: rt.limits.call_timeout,
        })
    }

    fn unload(&mut self) {
        self.world = None;
        self.store = None;
    }

    fn run(&mut self, call: &Call, ctx: CallContext) -> Result<CallOutput, PluginError> {
        let (Some(store), Some(world)) = (&mut self.store, &self.world) else {
            return Err(PluginError::Unavailable("unloaded".into()));
        };
        let data = store.data_mut();
        data.call = Some(ctx);
        (data.depth, data.refusal) = answering(call);
        data.deadline = Instant::now() + self.timeout;
        data.credit.store(0, std::sync::atomic::Ordering::Relaxed);
        store.set_epoch_deadline(1);
        let result = match call {
            Call::Command { id, args } => world.call_run_command(&mut *store, id, args.as_deref()),
            Call::PanelAction { panel, row, action } => {
                world.call_panel_action(&mut *store, panel, row, action)
            }
            Call::Event(e) => world.call_on_event(&mut *store, &api::event_to_wit(e)),
        };
        let data = store.data_mut();
        data.drop_render_handles();
        data.drop_task_handles();
        data.call = None;
        data.base = None;
        data.uploads.clear();
        data.refusal = None;
        let refused = std::mem::take(&mut data.refused);
        let out = CallOutput {
            tx: data.call_tx.take().filter(|tx| !tx.is_empty()),
            label: data.label.take(),
            // An answer to a refusal is told why it's refused as it
            // submits: told again by an event, it would answer again.
            refused: match call {
                Call::Event(DocEvent::EditRefused(_)) => Vec::new(),
                _ => refused,
            },
            depth: data.depth,
        };
        match result {
            Ok(Ok(())) => Ok(out),
            Ok(Err(msg)) => Err(PluginError::Failed(msg)),
            Err(e) => Err(PluginError::Trapped(describe_trap(&e, self.timeout))),
        }
    }
}

/// How far down a chain of answers the edits `call` makes are, and why
/// they're refused, if they are (M10.5 §14.3). A change's answer is one
/// deeper than the change, and refused when the change was the user
/// moving through history, or a failed call's edits taken back, which it
/// would fight, or was itself an answer to a change: answers to answers
/// stop. An answer to `edit-refused` would make again what was just
/// refused, so it's refused too.
fn answering(call: &Call) -> (u8, Option<String>) {
    let why = |why: &str| Some(why.to_string());
    match call {
        Call::Event(DocEvent::Changed(change)) => {
            let refused = if change.undone {
                why("the user undid this")
            } else if change.depth >= 1 {
                why("it answers a plugin's answer to a change, and those stop there")
            } else {
                None
            };
            (change.depth.saturating_add(1), refused)
        }
        Call::Event(DocEvent::EditRefused(_)) => (0, why("it answers a refusal of its edits")),
        _ => (0, None),
    }
}

/// WASI with only what the user granted: the listed directories (seen by
/// the plugin at their own paths, with `/` separators). No sockets: the
/// network is HTTP only, through `wasi:http` and [`NetHooks`]. Stderr goes
/// to ours, for panic messages.
fn wasi_ctx(grants: &Grants) -> Result<WasiCtx> {
    use wasmtime_wasi::{FsPerms, WasiCtxBuilder};
    let mut b = WasiCtxBuilder::new();
    b.inherit_stderr();
    for (dirs, perms) in [
        (&grants.read, FsPerms::ReadOnly),
        (&grants.write, FsPerms::ReadWrite),
    ] {
        for d in dirs {
            if !d.is_dir() {
                log::warn!("plugin permission: {} isn't a directory", d.display());
                continue;
            }
            let guest = d.to_string_lossy().replace('\\', "/");
            b.preopened_dir(d, guest, perms)
                .map_err(anyhow::Error::from)
                .with_context(|| format!("opening {}", d.display()))?;
        }
    }
    Ok(b.build())
}

/// `wasi:http` for a plugin: requests only to the hosts it was granted,
/// sent by the host (which does TLS). Time waiting on a response is added
/// to `credit`, which the call's time limit doesn't count.
pub(crate) struct NetHooks {
    grants: Grants,
    credit: Arc<std::sync::atomic::AtomicU64>,
}

impl wasmtime_wasi_http::WasiHttpHooks for NetHooks {
    fn send_request(
        &mut self,
        request: http::Request<wasmtime_wasi_http::WasiBody>,
        options: Option<wasmtime_wasi_http::RequestOptions>,
        fut: Box<dyn std::future::Future<Output = Result<(), wasmtime_wasi_http::Error>> + Send>,
    ) -> Box<
        dyn std::future::Future<
                Output = Result<
                    (
                        http::Response<wasmtime_wasi_http::WasiBody>,
                        Box<
                            dyn std::future::Future<Output = Result<(), wasmtime_wasi_http::Error>>
                                + Send,
                        >,
                    ),
                    wasmtime_wasi_http::Error,
                >,
            > + Send,
    > {
        _ = fut;
        let uri = request.uri();
        let host = uri.host().unwrap_or("").to_string();
        let allowed = self.grants.allows_host(&host, uri.port_u16());
        if !allowed {
            log::warn!(target: "plugin", "HTTP request to {host} denied");
        }
        let credit = self.credit.clone();
        Box::new(async move {
            use http_body_util::BodyExt;
            if !allowed {
                return Err(wasmtime_wasi_http::Error::HttpRequestDenied);
            }
            let started = Instant::now();
            let sent = wasmtime_wasi_http::default_send_request(request, options).await;
            credit.fetch_add(
                started.elapsed().as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
            let (res, io) = sent?;
            Ok((
                res.map(BodyExt::boxed_unsync),
                Box::new(io) as Box<dyn std::future::Future<Output = _> + Send>,
            ))
        })
    }
}

fn describe_trap(e: &wasmtime::Error, timeout: Duration) -> String {
    match e.downcast_ref::<Trap>() {
        Some(Trap::Interrupt) => format!("took longer than {timeout:?}"),
        Some(t) => t.to_string(),
        None => format!("{e:#}"),
    }
}

/// Per-plugin data the host functions reach.
pub(crate) struct HostState {
    wasi: WasiCtx,
    pub(crate) table: ResourceTable,
    limits: StoreLimits,
    pub(crate) plugin: String,
    /// Its version, as its transactions' actor says.
    version: Arc<str>,
    /// The Op JSON version its manifest says its edits are written in.
    pub(crate) edit_schema: Option<String>,
    /// The user let it know who they are ([`Grants::identity`]).
    pub(crate) identity: bool,
    /// Panel ids the manifest declares.
    pub(crate) panels: Vec<String>,
    pub(crate) storage: Storage,
    http: wasmtime_wasi_http::WasiHttpCtx,
    net: NetHooks,
    /// Nanoseconds this call spent waiting on the network (not charged).
    credit: Arc<std::sync::atomic::AtomicU64>,
    /// Render handles this call made, freed when it returns.
    pub(crate) render_handles: crate::render::CallHandles,
    /// `progress.task` handles this call made, freed when it returns.
    pub(crate) task_handles: Vec<u32>,
    pub(crate) call: Option<CallContext>,
    /// The running call's transaction ([`HostState::tx`]).
    pub(crate) call_tx: Option<Tx>,
    /// What it's made on, or the next one will be: the cell's document at
    /// the call's first read or submit, then what each commit published.
    pub(crate) base: Option<Arc<Published>>,
    /// What the call's edits are called: its first submit's label.
    pub(crate) label: Option<String>,
    /// What the call uploaded, granted in order in each of its
    /// transactions.
    pub(crate) uploads: Vec<api::Upload>,
    /// Why the call's edits are refused ([`answering`]), if they are.
    pub(crate) refusal: Option<String>,
    /// The refusals it was answered with, each once ([`CallOutput::refused`]).
    pub(crate) refused: Vec<String>,
    /// How far down a chain of answers its edits are ([`CallOutput::depth`]).
    pub(crate) depth: u8,
    /// When the running call times out.
    pub(crate) deadline: Instant,
}

impl HostState {
    pub(crate) fn doc(&self) -> Option<&DocContext> {
        self.call.as_ref()?.doc.as_ref()
    }

    pub(crate) fn progress(&self) -> Option<&in_progress::Tracker> {
        self.call.as_ref()?.progress.as_ref()
    }

    pub(crate) fn ui(&self) -> Option<Arc<dyn Ui>> {
        self.call.as_ref()?.ui.clone()
    }

    /// The call's transaction, and what it's made on: made at the call's
    /// first read or submit on the document as the cell has it then (M10.5
    /// §14.3), or after a commit on what the commit published, with what
    /// the call uploaded granted in it. Reads record their decision reads
    /// in it and see its own edits. None when no document is open.
    pub(crate) fn tx(&mut self) -> Option<(&mut Tx, &Published)> {
        if self.call_tx.is_none() {
            let base = match &self.base {
                Some(base) => base.clone(),
                None => self.doc()?.cell.load_full(),
            };
            let id = self.plugin.as_str().into();
            let mut tx = base.plugin_tx(id, self.version.clone(), self.identity);
            for upload in &self.uploads {
                tx.pin(upload.blob.clone());
                tx.grant(upload.grant.clone());
            }
            self.base = Some(base);
            self.call_tx = Some(tx);
        }
        Some((self.call_tx.as_mut()?, self.base.as_deref()?))
    }

    /// Page `page` as the call's transaction draws it under `inputs`: text
    /// as the file will draw it ([`ViewInputs::plugin_text`]), a render
    /// with or without markup, OCR its content alone
    /// ([`ViewInputs::content_only`]). Read: what it draws so. The call's
    /// document, the snapshot it reads, the call's own edits in it, and
    /// the page.
    pub(crate) fn drawn(
        &mut self,
        page: u64,
        inputs: &ViewInputs,
    ) -> Result<(&DocContext, Snapshot, PageId), String> {
        let id = PageId(page);
        let (tx, base) = self.tx().ok_or("no document is open")?;
        tx.read_render(id, inputs);
        (tx.state().def(id)).ok_or_else(|| format!("no page {page}"))?;
        let snap = Snapshot {
            state: tx.state().clone(),
            ..base.snap.clone()
        };
        Ok((self.doc().expect("a document's call"), snap, id))
    }

    /// How the plugin is told who made what (M10.5 §6.8).
    pub(crate) fn roles(&self) -> Roles {
        Roles {
            plugin: self.plugin.clone(),
            installed: self
                .call
                .as_ref()
                .map(|c| c.plugins.clone())
                .unwrap_or_default(),
            identity: self.identity,
        }
    }
}

/// How a plugin is told who made a mark or a comment: an installed
/// plugin by its role, and, granted `identity`, anyone else by name;
/// without it, the user as "you" and everyone else as "other".
pub(crate) struct Roles {
    plugin: String,
    installed: Arc<[String]>,
    identity: bool,
}

impl Roles {
    /// `author`'s, in the document `base` is of.
    pub(crate) fn of(&self, base: &Published, author: &str) -> String {
        if author == self.plugin || self.installed.iter().any(|id| id == author) {
            format!("plugin {author}")
        } else if self.identity {
            author.to_string()
        } else if base.is_user(author) {
            "you".into()
        } else {
            "other".into()
        }
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl wasmtime_wasi_http::WasiHttpView for HostState {
    fn http(&mut self) -> wasmtime_wasi_http::WasiHttpCtxView<'_> {
        wasmtime_wasi_http::WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.net,
        }
    }
}
