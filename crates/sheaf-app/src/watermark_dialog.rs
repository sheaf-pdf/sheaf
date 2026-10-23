//! Document > Watermark… and Background…: text, an image or a PDF's page
//! across pages, with opacity, rotation and position, in front of the page
//! content (a watermark) or behind it (a background): the document's
//! watermark at that z (M10.5 slice 16), set by one op and taken off by
//! another. Tab moves between fields, Enter applies, Escape cancels.
//!
//! The dialog starts from the watermark the document has at its z, as the
//! model has it, one a saved file had included: an image or a page it
//! draws is kept unless another file is chosen. It previews the page the
//! active pane showed as it opened, as applying would draw it (M10.5 §8.6,
//! `dialog_preview`): the settings' op lowered on the document as it is
//! now ([`draw_preview`]), off the UI thread, again at each change of the
//! settings and each change the document goes through
//! ([`WatermarkDialog::follow`]), and the page realized from the state that
//! leaves, as the screen draws it. It says what others drew of its kind on
//! the selected pages, or all of them, which applying replaces and Remove
//! takes off, counted once off the UI thread as it opens
//! (`generator_dialog::Count`; proposal 31, D61).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use futures::channel::oneshot;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton,
    Subscription, Window, div, prelude::*, px,
};
use sheaf_doc::model::{ImageRef, PageForm, WatermarkContent as Drawn, WatermarkGen};
use sheaf_doc::store::BlobKind;
use sheaf_doc::tx::LowerCx;
use sheaf_doc::{Blob, DocState, Document, Snapshot, Src, Store, ViewInputs};
use sheaf_edit::{
    BlobSel, Color, FontChoice, GridPos, Op, PageSel, Rotation, ScopeSel, WatermarkContent,
    WatermarkSpec, Z,
};
use sheaf_image::raster::PixelFormat;
use sheaf_ops::NoProgress;
use sheaf_render::{Budget, Job, Rendered};
use sheaf_ui::button::{Button, Chip};
use sheaf_ui::text_input::{TextInput, TextInputEvent};
use sheaf_ui::{dialog, theme};

use crate::dialog_preview::{Preview, drawn_width, why};
use crate::generator_dialog::{Count, FAMILIES, FONTS, STYLES, counted_pages, font_at, scope_text};
use crate::jobs::Previewer;
use crate::{DialogCancel, DialogNext, DialogPrev, DialogRun};

pub const KEY_CONTEXT: &str = "WatermarkDialog";

const COLORS: [(Color, &str); 5] = [
    ([200, 30, 30], "Red"),
    ([128, 128, 128], "Gray"),
    ([20, 60, 170], "Blue"),
    ([0, 0, 0], "Black"),
    ([30, 130, 60], "Green"),
];
const ROTATIONS: [f32; 4] = [0.0, 45.0, 90.0, -45.0];
const OPACITIES: [f32; 5] = [0.1, 0.25, 0.5, 0.75, 1.0];

/// Each names pages of the version the dialog opened on
/// ([`WatermarkDialog::opened`]).
pub enum WatermarkEvent {
    /// The op that sets the settings on the pages they name, and the file
    /// it draws, which nothing else holds yet: the transaction holds it
    /// until it's committed.
    Apply(Box<Op>, Option<Blob>),
    /// Take the watermark off every page: others' too (§6.5, D61), and the
    /// author's own with `include_authored`.
    Remove {
        include_authored: bool,
    },
    Dismiss,
}

pub struct WatermarkContext {
    /// A watermark (over the content) or a background (under it).
    pub z: Z,
    /// The document as the dialog opened on it: its pages are counted and
    /// named by id there, its watermark at `z` is what the dialog starts
    /// from, and the change is made on that version.
    pub opened: Snapshot,
    /// The user's lowering context on it, which the preview lowers on,
    /// and what the preview is drawn with.
    pub lower: Arc<LowerCx>,
    pub previewer: Previewer,
    /// The selected pages (0-based), which the dialog counts others' on:
    /// none for all of them.
    pub selected: Vec<usize>,
}

impl WatermarkContext {
    fn page_count(&self) -> usize {
        self.opened.state.page_count()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Image,
}

/// What an image watermark draws.
#[derive(Clone)]
enum Art {
    /// A file chosen in the dialog, read into the document's store: an
    /// image, or a PDF, whose first page is drawn, kept parsed while the
    /// dialog's open.
    Image {
        path: PathBuf,
        blob: Blob,
    },
    Pdf {
        path: PathBuf,
        src: Src,
    },
    /// What the watermark at the dialog's z draws now, kept: a page or an
    /// image, the file's own when `placed`.
    Kept {
        page: bool,
        placed: bool,
    },
}

impl Art {
    /// What the dialog says it is.
    fn name(&self) -> String {
        let file = |p: &Path| (p.file_name()).map(|n| n.to_string_lossy().to_string());
        match self {
            Art::Image { path, .. } => file(path).unwrap_or_else(|| "the image chosen".into()),
            Art::Pdf { path, .. } => match file(path) {
                Some(name) => format!("{name}, page 1"),
                None => "the PDF chosen, page 1".into(),
            },
            Art::Kept { page, placed } => match (page, placed) {
                (false, true) => "the image in the file",
                (true, true) => "the page in the file",
                (false, false) => "the image it draws now",
                (true, false) => "the page it draws now",
            }
            .into(),
        }
    }

    /// The file it reads, which a transaction setting it holds.
    fn holds(&self) -> Option<Blob> {
        match self {
            Art::Image { blob, .. } => Some(blob.clone()),
            Art::Pdf { src, .. } => Some(src.blob().clone()),
            Art::Kept { .. } => None,
        }
    }

    /// What a watermark of it draws, `scale` of the page wide.
    fn content(&self, scale: f32, opacity: f32) -> WatermarkContent {
        let id = |blob: &Blob| Some(BlobSel::Id(blob.id()));
        match self {
            Art::Image { blob, .. } => WatermarkContent::Image {
                image: id(blob),
                scale,
                opacity,
            },
            Art::Pdf { src, .. } => WatermarkContent::Page {
                pdf: id(src.blob()),
                page: 1,
                scale,
                opacity,
            },
            Art::Kept { page: false, .. } => WatermarkContent::Image {
                image: None,
                scale,
                opacity,
            },
            Art::Kept { page: true, .. } => WatermarkContent::Page {
                pdf: None,
                page: 1,
                scale,
                opacity,
            },
        }
    }
}

/// The file at `path` read into `store` for a watermark to draw: an image
/// file, or a PDF, parsed (off the UI thread).
fn read_art(store: &Store, path: &Path) -> anyhow::Result<Art> {
    let reading = || format!("reading {}", path.display());
    let bytes: Arc<[u8]> = std::fs::read(path).with_context(reading)?.into();
    let path = path.to_path_buf();
    if let Some(kind) = BlobKind::image(&bytes) {
        let blob = store.intern(bytes, kind)?;
        return Ok(Art::Image { path, blob });
    }
    let src = (store.open(bytes, BlobKind::file(&path, None)))
        .with_context(|| format!("{} is no image or PDF sheaf reads", path.display()))?;
    Ok(Art::Pdf { path, src })
}

/// The preview of `set`, the op applying the dialog's settings makes,
/// holding `holds`, the file it draws: lowered on `now` with `lower` as
/// applying lowers it; then the page the previewer shows (the first, when
/// `now` hasn't it) of the state that leaves, drawn `width` device px wide
/// by the previewer's views under the screen's inputs, in the screen's
/// budget. An error when it can't be set so, and why.
fn draw_preview(
    now: &Snapshot,
    lower: Arc<LowerCx>,
    previewer: &Previewer,
    set: Op,
    holds: Option<Blob>,
    width: u32,
) -> anyhow::Result<oneshot::Receiver<anyhow::Result<Rendered>>> {
    let page = previewer.page_in(now)?;
    let mut tx = Document::tx_at(now, lower);
    if let Some(blob) = holds {
        tx.pin(blob);
    }
    tx.apply(set, &NoProgress).map_err(why)?;
    let job = Job::Page {
        width,
        rotation: Rotation::R0,
    };
    let (st, screen, format) = (tx.state(), ViewInputs::screen(), PixelFormat::Bgra);
    Ok((previewer.views).render(st, page, &screen, job, format, 0, Budget::Screen))
}

const F_TEXT: usize = 0;
const F_SIZE: usize = 1;
const F_SCALE: usize = 2;
const F_PAGES: usize = 3;

pub struct WatermarkDialog {
    focus: FocusHandle,
    ctx: WatermarkContext,
    /// The document as it is now, which the preview is lowered on.
    now: Snapshot,
    /// Whether the document had a watermark at the dialog's z as it
    /// opened, which applying updates.
    current: bool,
    kind: Kind,
    fields: Vec<Entity<TextInput>>,
    font: FontChoice,
    color: Color,
    opacity: f32,
    angle: f32,
    position: GridPos,
    art: Option<Art>,
    /// Remove takes the author's own off too.
    include_authored: bool,
    /// What others drew of its kind on the pages `counted_on` names.
    count: Count,
    counted_on: String,
    /// The field with focus, for Tab.
    at: usize,
    error: Option<String>,
    preview: Preview<Op>,
    /// Device px a logical px, as the dialog opened: what the preview is
    /// drawn at.
    scale: f32,
    _subs: Vec<Subscription>,
}

impl EventEmitter<WatermarkEvent> for WatermarkDialog {}

impl Focusable for WatermarkDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// What the dialog starts from: the watermark the document has at its z,
/// as the model has it, or the dialog's defaults there.
struct Start {
    /// Whether there's one, which applying updates.
    current: bool,
    kind: Kind,
    text: String,
    font: FontChoice,
    size: f32,
    color: Color,
    opacity: f32,
    angle: f32,
    scale: f32,
    art: Option<Art>,
    position: GridPos,
    /// The pages it's on, as the Pages field says them: empty for all.
    pages: String,
}

impl Start {
    /// The defaults at `z`: a red DRAFT over the content, a gray
    /// CONFIDENTIAL under it, on every page.
    fn new(z: Z) -> Start {
        let (text, size, color, opacity) = match z {
            Z::Over => ("DRAFT", 72.0, [200, 30, 30], 0.3),
            Z::Under => ("CONFIDENTIAL", 60.0, [128, 128, 128], 0.25),
        };
        Start {
            current: false,
            kind: Kind::Text,
            text: text.into(),
            font: FontChoice::HelveticaBold,
            size,
            color,
            opacity,
            angle: 45.0,
            scale: 0.5,
            art: None,
            position: GridPos::Center,
            pages: String::new(),
        }
    }

    /// What `st`'s watermark at `z` sets, from the defaults there.
    fn at(st: &DocState, z: Z) -> Start {
        let start = Start::new(z);
        let Some((g, wm)) = st.watermark(z).and_then(|g| Some((g, g.spec.watermark()?))) else {
            return start;
        };
        let start = Start {
            current: true,
            position: wm.position,
            pages: scope_text(st, &g.scope),
            ..start
        };
        let kept = |page, placed| Some(Art::Kept { page, placed });
        match &wm.content {
            Drawn::Text {
                text,
                font,
                size,
                color,
                opacity,
                angle,
            } => Start {
                text: text.to_string(),
                font: *font,
                size: *size,
                color: *color,
                opacity: *opacity,
                angle: *angle,
                ..start
            },
            Drawn::Image {
                image,
                scale,
                opacity,
            } => Start {
                kind: Kind::Image,
                art: kept(false, matches!(image, ImageRef::Placed(_))),
                scale: *scale,
                opacity: *opacity,
                ..start
            },
            Drawn::Page {
                page,
                scale,
                opacity,
            } => Start {
                kind: Kind::Image,
                art: kept(true, matches!(page, PageForm::Placed(_))),
                scale: *scale,
                opacity: *opacity,
                ..start
            },
        }
    }
}

impl WatermarkDialog {
    pub fn new(ctx: WatermarkContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let start = Start::at(&ctx.opened.state, ctx.z);
        let values = [
            start.text,
            format!("{}", start.size),
            format!("{}", (start.scale * 100.0).round()),
            start.pages,
        ];
        let placeholders = ["e.g. DRAFT", "72", "50", "All pages"];
        let fields: Vec<Entity<TextInput>> = (0..4)
            .map(|i| {
                let v = values[i].clone();
                cx.new(|cx| {
                    let mut t = TextInput::new(placeholders[i], cx);
                    t.set_text(v, cx);
                    t
                })
            })
            .collect();
        let mut subs: Vec<Subscription> = fields
            .iter()
            .map(|f| {
                cx.subscribe(f, |d, _, ev: &TextInputEvent, cx| match ev {
                    TextInputEvent::Confirm => d.go(cx),
                    TextInputEvent::Cancel => cx.emit(WatermarkEvent::Dismiss),
                })
            })
            .collect();
        for f in &fields {
            subs.push(cx.observe(f, |d, _, cx| {
                d.relower(false, cx);
                cx.notify();
            }));
        }
        let at = match start.kind {
            Kind::Text => F_TEXT,
            Kind::Image => F_SCALE,
        };
        window.focus(&fields[at].read(cx).focus_handle(cx), cx);
        let (pages, counted_on) = counted_pages(&ctx.opened, &ctx.selected);
        let kinds = BTreeSet::from([WatermarkGen::kind_at(ctx.z)]);
        let count = Count::begin(&ctx.opened, kinds, pages, |d: &mut Self| &mut d.count, cx);
        let mut d = Self {
            focus: cx.focus_handle(),
            now: ctx.opened.clone(),
            current: start.current,
            kind: start.kind,
            fields,
            font: start.font,
            color: start.color,
            opacity: start.opacity,
            angle: start.angle,
            position: start.position,
            art: start.art,
            include_authored: false,
            count,
            counted_on,
            ctx,
            at,
            error: None,
            preview: Preview::default(),
            scale: window.scale_factor(),
            _subs: subs,
        };
        d.relower(false, cx);
        d
    }

    /// The document went through a change: it's `now` as it is, which
    /// `lower` lowers on, and the preview is lowered again on it.
    pub fn follow(&mut self, now: Snapshot, lower: Arc<LowerCx>, cx: &mut Context<Self>) {
        self.now = now;
        self.ctx.lower = lower;
        self.relower(true, cx);
        cx.notify();
    }

    /// Lower the preview of the settings again, off the UI thread, when
    /// they make another op than the last lowered, or `again`; settings
    /// with something wrong leave the last picture.
    fn relower(&mut self, again: bool, cx: &mut Context<Self>) {
        let Ok(set) = self.asked(cx) else {
            return;
        };
        let Some(generation) = self.preview.begin(&set, again) else {
            return;
        };
        let shown = self.now.geom(self.ctx.previewer.page).shown();
        let width = drawn_width(shown, self.scale);
        let (now, lower) = (self.now.clone(), self.ctx.lower.clone());
        let previewer = self.ctx.previewer.clone();
        let holds = self.art.as_ref().and_then(Art::holds);
        self.preview.task = Some(cx.spawn(async move |this, cx| {
            let drawn = cx.background_executor().spawn(async move {
                let rx = draw_preview(&now, lower, &previewer, set, holds, width)?;
                Ok(rx.await.context("the render thread is gone")??.raster)
            });
            let drawn = drawn.await;
            this.update(cx, |d, cx| {
                if d.preview.take(generation, drawn, d.scale) {
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Let go of the preview's GPU textures (on close).
    pub fn release(&mut self, window: &mut Window) {
        self.preview.release(window);
    }

    fn order(&self) -> Vec<usize> {
        match self.kind {
            Kind::Text => vec![F_TEXT, F_SIZE, F_PAGES],
            Kind::Image => vec![F_SCALE, F_PAGES],
        }
    }

    fn step(&mut self, by: i64, window: &mut Window, cx: &mut Context<Self>) {
        let order = self.order();
        let at = order.iter().position(|&i| i == self.at).unwrap_or(0) as i64;
        self.at = order[(at + by).rem_euclid(order.len() as i64) as usize];
        let h = self.fields[self.at].read(cx).focus_handle(cx);
        window.focus(&h, cx);
        cx.notify();
    }

    /// Choose an image or a PDF, read into the document's store off the
    /// UI thread.
    fn pick(&mut self, cx: &mut Context<Self>) {
        let rx = crate::file_dialog::open_filtered(
            cx,
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Choose Image or PDF".into()),
            },
            crate::file_dialog::Filter::PdfAndImages,
        );
        let store = self.ctx.previewer.store.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.pop() else { return };
            let read = cx
                .background_executor()
                .spawn(async move { read_art(&store, &path) });
            let read = read.await;
            this.update(cx, |d, cx| {
                match read {
                    Ok(art) => {
                        d.art = Some(art);
                        d.kind = Kind::Image;
                        d.error = None;
                        d.relower(false, cx);
                    }
                    Err(e) => d.error = Some(format!("{e:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The version it opened on.
    pub fn opened(&self) -> &Snapshot {
        &self.ctx.opened
    }

    /// The z it sets: over for the watermark, under for the background.
    pub fn z(&self) -> Z {
        self.ctx.z
    }

    /// The op the settings make, setting the watermark on the pages they
    /// name of the version the dialog opened on, or what's wrong with them.
    fn asked(&self, cx: &App) -> Result<Op, String> {
        let text = |i: usize| self.fields[i].read(cx).text().trim().to_string();
        let content = match self.kind {
            Kind::Text => {
                let t = self.fields[F_TEXT].read(cx).text().to_string();
                if t.trim().is_empty() {
                    return Err("Type the watermark's text".into());
                }
                let size: f32 = (text(F_SIZE).parse().ok())
                    .filter(|s| (6.0..=400.0).contains(s))
                    .ok_or("Size: 6 to 400 pt")?;
                WatermarkContent::Text {
                    text: t,
                    font: self.font,
                    size,
                    color: self.color,
                    opacity: self.opacity,
                    angle: self.angle,
                }
            }
            Kind::Image => {
                let art = self.art.as_ref().ok_or("Choose an image or a PDF")?;
                let pct: f32 = (text(F_SCALE).parse().ok())
                    .filter(|s| (1.0..=200.0).contains(s))
                    .ok_or("Width: 1 to 200% of the page")?;
                art.content(pct / 100.0, self.opacity)
            }
        };
        let scope = match text(F_PAGES) {
            t if t.is_empty() => ScopeSel::All,
            t => {
                let indexes = sheaf_ops::pages::parse(&t, self.ctx.page_count())
                    .map_err(|e| format!("Pages: {e}"))?;
                let ids = crate::page_insert::ids_at(&self.ctx.opened, &indexes);
                ScopeSel::Pages(PageSel::ids(ids))
            }
        };
        let spec = WatermarkSpec {
            content,
            position: self.position,
        };
        let z = self.ctx.z;
        Ok(Op::SetWatermark { spec, z, scope })
    }

    fn go(&mut self, cx: &mut Context<Self>) {
        match self.asked(cx) {
            Ok(set) => {
                let holds = self.art.as_ref().and_then(Art::holds);
                cx.emit(WatermarkEvent::Apply(Box::new(set), holds))
            }
            Err(e) => {
                self.error = Some(e);
                cx.notify();
            }
        }
    }

    fn field(&self, i: usize, w: f32) -> AnyElement {
        div()
            .w(px(w))
            .child(self.fields[i].clone())
            .into_any_element()
    }

    /// A row of chips named `name`, one each of `opts` (its value and
    /// label), `current` selected; clicking one gives it to `set`.
    fn chips<T: Copy + PartialEq + 'static>(
        &self,
        name: &'static str,
        opts: Vec<(T, String)>,
        current: T,
        set: fn(&mut Self, T),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(opts.into_iter().enumerate().map(|(i, (v, label))| {
                Chip::new((name, i), label)
                    .selected(v == current)
                    .on_click(cx.listener(move |d, _, _, cx| {
                        set(d, v);
                        d.error = None;
                        d.relower(false, cx);
                        cx.notify();
                    }))
            }))
            .into_any_element()
    }
}

impl Render for WatermarkDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.preview.drop_retired(window);
        let picture = (self.preview).element(self.now.geom(self.ctx.previewer.page).shown());
        let row = |label: &str, el: AnyElement| {
            dialog::field_row(label.to_string(), px(100.0), false, el).into_any_element()
        };
        let kinds = self.chips(
            "wm-kind",
            vec![
                (Kind::Text, "Text".into()),
                (Kind::Image, "Image or page".into()),
            ],
            self.kind,
            |d, k| d.kind = k,
            cx,
        );
        let (family, style) = font_at(self.font);
        let families = self.chips(
            "wm-family",
            (FAMILIES.iter().enumerate())
                .map(|(i, f)| (FONTS[i][style], f.to_string()))
                .collect(),
            self.font,
            |d, f| d.font = f,
            cx,
        );
        let styles = self.chips(
            "wm-style",
            (STYLES.iter().enumerate())
                .map(|(i, s)| (FONTS[family][i], s.to_string()))
                .collect(),
            self.font,
            |d, f| d.font = f,
            cx,
        );
        let colors = self.chips(
            "wm-color",
            (COLORS.iter()).map(|(c, l)| (*c, l.to_string())).collect(),
            self.color,
            |d, c| d.color = c,
            cx,
        );
        let rotations = self.chips(
            "wm-rot",
            (ROTATIONS.iter()).map(|r| (*r, format!("{r}°"))).collect(),
            self.angle,
            |d, r| d.angle = r,
            cx,
        );
        let opacities = self.chips(
            "wm-op",
            (OPACITIES.iter())
                .map(|o| (*o, format!("{}%", (o * 100.0).round())))
                .collect(),
            self.opacity,
            |d, o| d.opacity = o,
            cx,
        );
        // A 3×3 grid of positions, as the page shows.
        const GRID: [[GridPos; 3]; 3] = [
            [GridPos::TopLeft, GridPos::Top, GridPos::TopRight],
            [GridPos::Left, GridPos::Center, GridPos::Right],
            [GridPos::BottomLeft, GridPos::Bottom, GridPos::BottomRight],
        ];
        let grid = div()
            .flex()
            .flex_col()
            .gap_1()
            .children(GRID.iter().enumerate().map(|(v, cells)| {
                div()
                    .flex()
                    .gap_1()
                    .children(cells.iter().enumerate().map(|(h, &at)| {
                        div()
                            .id(("wm-pos", v * 3 + h))
                            .w(px(26.0))
                            .h(px(20.0))
                            .rounded_sm()
                            .border_1()
                            .border_color(theme::BORDER)
                            .bg(if at == self.position {
                                theme::ACCENT
                            } else {
                                theme::CONTROL
                            })
                            .cursor_pointer()
                            .on_click(cx.listener(move |d, _, _, cx| {
                                d.position = at;
                                d.relower(false, cx);
                                cx.notify();
                            }))
                    }))
            }))
            .into_any_element();
        let file_row = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                Button::new("wm-pick", "Choose…").on_click(cx.listener(|d, _, _, cx| d.pick(cx))),
            )
            .child(dialog::subtitle(
                (self.art.as_ref())
                    .map(Art::name)
                    .unwrap_or_else(|| "no image or PDF yet".into()),
            ))
            .into_any_element();
        let pair = |a: AnyElement, note: String| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(a)
                .child(dialog::subtitle(note))
                .into_any_element()
        };
        let authored = Chip::new(
            ("wm-authored", 0usize),
            format!(
                "{} the author's own too",
                if self.include_authored { "☑" } else { "☐" }
            ),
        )
        .on_click(cx.listener(|d, _, _, cx| {
            d.include_authored = !d.include_authored;
            cx.notify();
        }))
        .into_any_element();
        let (title, note, what) = match self.ctx.z {
            Z::Over => ("Watermark", "in front of the page's content", "watermark"),
            Z::Under => ("Background", "behind the page's content", "background"),
        };
        let found = dialog::subtitle(self.count.said(&self.counted_on, what)).into_any_element();
        let message = match (&self.error, &self.preview.failed) {
            (Some(e), _) | (None, Some(e)) => Some(dialog::message(e.clone(), true)),
            (None, None) => None,
        };
        dialog::backdrop("wm-backdrop")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|d, _: &DialogNext, window, cx| d.step(1, window, cx)))
            .on_action(cx.listener(|d, _: &DialogPrev, window, cx| d.step(-1, window, cx)))
            .on_action(cx.listener(|d, _: &DialogRun, _, cx| d.go(cx)))
            .on_action(cx.listener(|_, _: &DialogCancel, _, cx| cx.emit(WatermarkEvent::Dismiss)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(WatermarkEvent::Dismiss)),
            )
            .child(
                dialog::card("wm-dialog", px(640.0))
                    .child(dialog::header(title).child(dialog::subtitle(note)))
                    .child(row("Use", kinds))
                    .when(self.kind == Kind::Text, |d| {
                        d.child(row("Text", self.field(F_TEXT, 300.0)))
                            .child(row("Font", families))
                            .child(row("", styles))
                            .child(row("Size", pair(self.field(F_SIZE, 70.0), "pt".into())))
                            .child(row("Color", colors))
                            .child(row("Rotation", rotations))
                    })
                    .when(self.kind == Kind::Image, |d| {
                        d.child(row("File", file_row)).child(row(
                            "Width",
                            pair(self.field(F_SCALE, 70.0), "% of the page".into()),
                        ))
                    })
                    .child(row("Opacity", opacities))
                    .child(row("Position", grid))
                    .child(row(
                        "Pages",
                        pair(
                            self.field(F_PAGES, 160.0),
                            format!("of {} (empty: all)", self.ctx.page_count()),
                        ),
                    ))
                    .child(row("Found", found))
                    .child(row("Remove", authored))
                    .children(message)
                    .child(row("Preview", picture))
                    .child(
                        dialog::footer()
                            .child(dialog::hint("Tab moves, Enter applies"))
                            .child(Button::new("wm-remove", "Remove").on_click(cx.listener(
                                |d, _, _, cx| {
                                    let include_authored = d.include_authored;
                                    cx.emit(WatermarkEvent::Remove { include_authored })
                                },
                            )))
                            .child(Button::new("wm-cancel", "Cancel").on_click(
                                cx.listener(|_, _, _, cx| cx.emit(WatermarkEvent::Dismiss)),
                            ))
                            .child(
                                Button::new("wm-go", if self.current { "Update" } else { "Add" })
                                    .primary()
                                    .on_click(cx.listener(|d, _, _, cx| d.go(cx))),
                            ),
                    ),
            )
            .text_color(theme::TEXT_SOFT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use sheaf_doc::Actor;
    use sheaf_doc::tx::fixture::{set_watermark, watermark};
    use sheaf_image::raster::Raster;
    use sheaf_ops::views::Views;
    use sheaf_render::EnginePool;

    /// `bytes` opened as the file `name` in `dir`.
    fn opened(dir: &Path, name: &str, bytes: &[u8]) -> Document {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        sheaf_doc::open::file(&path, None).unwrap().doc
    }

    /// A 4×2 red PNG, written as `name` in `dir`.
    fn png(dir: &Path, name: &str) -> PathBuf {
        let rgb = image::RgbImage::from_pixel(4, 2, image::Rgb([200, 30, 30]));
        let path = dir.join(name);
        std::fs::write(&path, sheaf_image::encode::png(&rgb, 72).unwrap()).unwrap();
        path
    }

    /// The op that sets `art` under the content of every page, half the
    /// page wide.
    fn under(art: &Art) -> Op {
        let spec = WatermarkSpec {
            content: art.content(0.5, 0.5),
            position: GridPos::Center,
        };
        let (z, scope) = (Z::Under, ScopeSel::All);
        Op::SetWatermark { spec, z, scope }
    }

    /// The Watermark dialog's preview is its page as applying the settings
    /// draws it (M10.5 §8.6): text, an image file and a PDF's page, each
    /// read as the dialog reads it and held by the preview's transaction
    /// alone, pixel for pixel the page the apply leaves, not the page as it
    /// shows now; an image the document doesn't draw, kept, the reason. A
    /// lowering's picture shows only while no later lowering has begun:
    /// settings that make the op lowered last aren't lowered again, an
    /// older lowering's picture is dropped, a change of the document lowers
    /// the same op again, and a failure keeps the last picture.
    #[test]
    fn the_watermark_dialog_previews_the_watermark_and_drops_a_stale_result() {
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let blank = sheaf_doc::memory::blank_pages(612.0, 792.0, 2).unwrap();
        let doc = opened(dir.path(), "blank.pdf", &blank);
        let (now, page) = (doc.snapshot(), doc.page(1).unwrap());
        let pool = EnginePool::new().unwrap();
        let previewer = Previewer {
            store: doc.store().clone(),
            views: Views::new(pool.clone()),
            page,
        };
        let lower = || doc.lower_cx(Actor::User);
        let drawn = |st: &DocState| {
            let job = Job::Page {
                width: 300,
                rotation: Rotation::R0,
            };
            let (screen, format) = (ViewInputs::screen(), PixelFormat::Bgra);
            let views = Views::new(pool.clone());
            let rx = views.render(st, page, &screen, job, format, 0, Budget::Screen);
            block_on(rx).unwrap().unwrap().raster.data
        };
        let preview = |set: &Op, holds: Option<Blob>| {
            let rx = draw_preview(&now, lower(), &previewer, set.clone(), holds, 300).unwrap();
            block_on(rx).unwrap().unwrap().raster.data
        };
        let applied = |set: &Op, holds: Option<Blob>| {
            let mut tx = Document::tx_at(&now, lower());
            tx.pin(holds.unwrap());
            tx.apply(set.clone(), &NoProgress).unwrap();
            drawn(tx.state())
        };

        let text = set_watermark(watermark("DRAFT"), Z::Over, None);
        let shown = preview(&text, None);
        let mut tx = Document::tx_at(&now, lower());
        tx.apply(text.clone(), &NoProgress).unwrap();
        assert!(shown == drawn(tx.state()), "as applying draws it");
        assert!(shown != drawn(&now.state), "not as it shows now");

        let image = read_art(doc.store(), &png(dir.path(), "logo.png")).unwrap();
        assert!(matches!(image, Art::Image { .. }));
        assert_eq!(image.name(), "logo.png");
        let set = under(&image);
        let shown = preview(&set, image.holds());
        assert!(
            shown == applied(&set, image.holds()),
            "the image as applied"
        );
        assert!(shown != drawn(&now.state));
        let pdf = dir.path().join("cover.pdf");
        std::fs::write(
            &pdf,
            sheaf_doc::memory::blank_pages(300.0, 200.0, 1).unwrap(),
        )
        .unwrap();
        let cover = read_art(doc.store(), &pdf).unwrap();
        assert!(matches!(cover, Art::Pdf { .. }));
        assert_eq!(cover.name(), "cover.pdf, page 1");
        let set = under(&cover);
        assert!(preview(&set, cover.holds()) == applied(&set, cover.holds()));
        let what = dir.path().join("notes.txt");
        std::fs::write(&what, "not a picture").unwrap();
        assert!(read_art(doc.store(), &what).is_err());

        let kept = Art::Kept {
            page: false,
            placed: false,
        };
        let none = draw_preview(&now, lower(), &previewer, under(&kept), None, 300);
        let why = none.expect_err("no image to keep");
        assert_eq!(format!("{why:#}"), "no image is drawn at its z to keep");

        let picture = |width| Raster {
            width,
            height: 4,
            format: PixelFormat::Bgra,
            data: vec![255; width as usize * 16],
        };
        let mut shown = Preview::default();
        let first = shown.begin(&text, false).unwrap();
        assert_eq!(
            shown.begin(&text, false),
            None,
            "the same op isn't lowered again"
        );
        let other = set_watermark(watermark("OTHER"), Z::Over, None);
        let second = shown.begin(&other, false).unwrap();
        assert!(
            !shown.take(first, Ok(picture(2)), 2.0),
            "a stale picture is dropped"
        );
        assert!(shown.picture.is_none());
        assert!(shown.take(second, Ok(picture(8)), 2.0));
        let third = (shown.begin(&other, true)).expect("lowered again on a change");
        let late = anyhow::anyhow!("late");
        assert!(!shown.take(second, Err(late), 2.0));
        assert!(shown.take(third, Err(why), 2.0));
        assert_eq!(
            shown.failed.as_deref(),
            Some("no image is drawn at its z to keep")
        );
        assert!(shown.picture.is_some(), "the last picture stays");
    }

    /// The dialog starts from the document's watermark at its z as the
    /// model has it, and from what a file sheaf saved has after it's opened
    /// again: its text, face, size, colour, opacity, angle, place and
    /// pages, or the image it draws, kept, which the op then names by its z
    /// alone; with none there, from its defaults.
    #[test]
    fn the_dialog_starts_from_the_model_after_a_save_and_reopen() {
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let blank = sheaf_doc::memory::blank_pages(612.0, 792.0, 3).unwrap();
        let mut doc = opened(dir.path(), "blank.pdf", &blank);
        let fresh = Start::at(doc.state(), Z::Over);
        assert!(!fresh.current);
        assert_eq!((fresh.text.as_str(), fresh.size), ("DRAFT", 72.0));
        assert!(fresh.kind == Kind::Text && fresh.pages.is_empty());
        let pages = [doc.page(1).unwrap(), doc.page(2).unwrap()];
        let spec = WatermarkSpec {
            content: WatermarkContent::Text {
                text: "Café".into(),
                font: FontChoice::TimesBold,
                size: 50.0,
                color: [20, 60, 170],
                opacity: 0.5,
                angle: 90.0,
            },
            position: GridPos::TopRight,
        };
        let image = read_art(doc.store(), &png(dir.path(), "logo.png")).unwrap();
        let ops = vec![set_watermark(spec, Z::Over, Some(&pages)), under(&image)];
        doc.commit_ops(ops, "Watermarks").unwrap();
        let unsaved = Start::at(doc.state(), Z::Under);
        let kept = unsaved.art.expect("the image kept");
        assert_eq!(kept.name(), "the image it draws now");
        let saved = sheaf_doc::save::rewrite(&doc).unwrap();
        let again = opened(dir.path(), "saved.pdf", &saved);

        let over = Start::at(again.state(), Z::Over);
        assert!(over.current && over.kind == Kind::Text);
        assert_eq!(
            (over.text.as_str(), over.font, over.size, over.color),
            ("Café", FontChoice::TimesBold, 50.0, [20, 60, 170])
        );
        assert_eq!((over.opacity, over.angle), (0.5, 90.0));
        assert_eq!(
            (over.position, over.pages.as_str()),
            (GridPos::TopRight, "2-3")
        );
        assert_eq!(font_at(over.font), (1, 1));

        let under_it = Start::at(again.state(), Z::Under);
        assert!(under_it.current && under_it.kind == Kind::Image);
        assert_eq!((under_it.scale, under_it.opacity), (0.5, 0.5));
        assert!(under_it.pages.is_empty(), "every page");
        let art = under_it.art.expect("the image in the file");
        assert_eq!(art.name(), "the image in the file");
        assert!(art.holds().is_none());
        let moved = WatermarkSpec {
            content: art.content(0.25, 0.5),
            position: GridPos::Bottom,
        };
        let WatermarkContent::Image { image: None, .. } = &moved.content else {
            panic!("named by its z: {moved:?}");
        };
        let mut tx = again.tx(Actor::User);
        tx.apply(set_watermark(moved, Z::Under, None), &NoProgress)
            .unwrap();
    }
}
