//! Document > Header & Footer… and Bates Numbering…: text in six places
//! (left, center, right of the header and of the footer) with tokens for
//! the page number, page count, date, file name and Bates number. Shown
//! over the pages until the document is saved. Tab moves between fields,
//! Enter applies, Escape cancels.
//!
//! The dialog previews the page the active pane showed as it opened, as
//! applying would draw it (M10.5 §8.6): the settings' op lowered on the
//! document as it is now ([`draw_preview`]), off the UI thread, again at
//! each change of the settings and each change the document goes through
//! ([`HeaderFooterDialog::follow`]), and the page realized from the state
//! that leaves, as the screen draws it. A lowering's picture shows only
//! while no later one has begun ([`Preview`]).

use std::sync::Arc;

use anyhow::Context as _;
use futures::channel::oneshot;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton, ObjectFit,
    RenderImage, Subscription, Task, Window, div, img, prelude::*, px,
};
use sheaf_doc::stamps::{Bates, HeaderFooter, TOKENS};
use sheaf_doc::tx::LowerCx;
use sheaf_doc::{Derive, Document, PageId, Snapshot, ViewInputs};
use sheaf_edit::{Op, Rotation};
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_ops::NoProgress;
use sheaf_render::{Budget, Job, Rendered};
use sheaf_ui::button::{Button, Chip};
use sheaf_ui::text_input::{TextInput, TextInputEvent};
use sheaf_ui::{dialog, theme};

use crate::jobs::Previewer;
use crate::{DialogCancel, DialogNext, DialogPrev, DialogRun};

pub const KEY_CONTEXT: &str = "HeaderFooterDialog";

const COLORS: [([u8; 3], &str); 4] = [
    ([0, 0, 0], "Black"),
    ([110, 110, 110], "Gray"),
    ([20, 60, 170], "Blue"),
    ([190, 20, 20], "Red"),
];

pub enum HeaderFooterEvent {
    /// The settings, and the pages they're on (none: all) of the version
    /// the dialog opened on ([`HeaderFooterDialog::opened`]).
    Apply(Box<HeaderFooter>, Option<Vec<PageId>>),
    Remove,
    Dismiss,
}

pub struct HeaderFooterContext {
    /// What's there now, to update.
    pub current: Option<HeaderFooter>,
    /// The document as the dialog opened on it: its pages are counted and
    /// named by id there, and the change is made on that version.
    pub opened: Snapshot,
    /// The user's lowering context on it, which the preview lowers on,
    /// and what the preview is drawn with.
    pub lower: Arc<LowerCx>,
    pub previewer: Previewer,
    pub file: String,
    /// The current scope as a page range ("" for all pages).
    pub scope: String,
    /// Start with Bates numbering on.
    pub bates: bool,
}

/// The fields, in Tab order.
const F_SIZE: usize = 6;
const F_TOP: usize = 7;
const F_BOTTOM: usize = 8;
const F_SIDES: usize = 9;
const F_FIRST: usize = 10;
const F_PAGES: usize = 11;
const F_PREFIX: usize = 12;
const F_SUFFIX: usize = 13;
const F_DIGITS: usize = 14;
const F_START: usize = 15;
const FIELDS: usize = 16;

/// The previewed page's height on screen, logical px, and its width at
/// most.
const PREVIEW_H: f32 = 220.0;
const PREVIEW_W: f32 = 340.0;

/// The dialog's preview: lowered and drawn off the UI thread, each
/// lowering numbered, so the picture of one that a later one has begun
/// after never shows.
#[derive(Default)]
struct Preview {
    /// The newest lowering's number, and the op it lowered: settings that
    /// make the same op aren't lowered again until the document changes.
    generation: u64,
    op: Option<Op>,
    /// Its lowering and drawing: replacing it drops an older one's render
    /// before the worker gets to it.
    task: Option<Task<()>>,
    /// The newest picture shown, and its size on screen; why the newest
    /// lowering drew none.
    picture: Option<(Arc<RenderImage>, (f32, f32))>,
    failed: Option<String>,
    /// Pictures let go, whose textures go at the next frame.
    retired: Vec<Arc<RenderImage>>,
}

impl Preview {
    /// A lowering of `op` begun: its number, or None when the newest
    /// lowered it already and `again` isn't asked.
    fn begin(&mut self, op: &Op, again: bool) -> Option<u64> {
        if !again && self.op.as_ref() == Some(op) {
            return None;
        }
        self.op = Some(op.clone());
        self.generation += 1;
        Some(self.generation)
    }

    /// Lowering `generation`'s picture, drawn at `scale` device px a
    /// logical px, or why it drew none: taken, unless a later lowering has
    /// begun, which makes it stale. Whether it was taken.
    fn take(&mut self, generation: u64, drawn: anyhow::Result<Raster>, scale: f32) -> bool {
        if generation != self.generation {
            return false;
        }
        match drawn {
            Ok(raster) => {
                let size = (raster.width as f32 / scale, raster.height as f32 / scale);
                let image = Arc::new(crate::docview::to_render_image(raster));
                let old = self.picture.replace((image, size));
                self.retired.extend(old.map(|(image, _)| image));
                self.failed = None;
            }
            Err(e) => self.failed = Some(format!("{e:#}")),
        }
        true
    }
}

/// The preview of `set`, the op applying the dialog's settings makes:
/// lowered on `now` with `lower` as applying lowers it, after the
/// previewed page's own drawn header, if its file has one, is stripped
/// onto a copy in the document's store (as `DocView::set_header_footer`
/// strips every such page); then that page (the first, when `now` hasn't
/// it) of the state that leaves, drawn `width` device px wide by the
/// previewer's views under the screen's inputs, in the screen's budget.
fn draw_preview(
    now: &Snapshot,
    lower: Arc<LowerCx>,
    previewer: &Previewer,
    set: Op,
    width: u32,
) -> anyhow::Result<oneshot::Receiver<anyhow::Result<Rendered>>> {
    let page = match now.state.def(previewer.page) {
        Some(_) => previewer.page,
        None => now.state.page_at(0).context("the document has pages")?,
    };
    let copies = sheaf_ops::derive::strip_stamps_of(&previewer.store, now, &[page], &NoProgress)?;
    let mut tx = Document::tx_at(now, lower);
    let bases: Vec<_> = copies.iter().flat_map(|c| c.bases()).collect();
    if !bases.is_empty() {
        tx.set_page_base(bases, Derive::StripHeaderFooter)?;
    }
    tx.apply(set, &NoProgress)?;
    let job = Job::Page {
        width,
        rotation: Rotation::R0,
    };
    let (st, screen, format) = (tx.state(), ViewInputs::screen(), PixelFormat::Bgra);
    Ok((previewer.views).render(st, page, &screen, job, format, 0, Budget::Screen))
}

pub struct HeaderFooterDialog {
    focus: FocusHandle,
    ctx: HeaderFooterContext,
    /// The document as it is now, which the preview is lowered on.
    now: Snapshot,
    fields: Vec<Entity<TextInput>>,
    color: [u8; 3],
    bates: bool,
    /// The field with focus, for Tab.
    at: usize,
    error: Option<String>,
    preview: Preview,
    /// Device px a logical px, as the dialog opened: what the preview is
    /// drawn at.
    scale: f32,
    _subs: Vec<Subscription>,
}

impl EventEmitter<HeaderFooterEvent> for HeaderFooterDialog {}

impl HeaderFooterContext {
    fn page_count(&self) -> usize {
        self.opened.state.page_count()
    }
}

impl Focusable for HeaderFooterDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn inches(pt: f32) -> String {
    format!("{}", (pt / 72.0 * 100.0).round() / 100.0)
}

/// Today as MM/DD/YYYY.
fn today() -> String {
    let d = sheaf_doc::pdf_date_now();
    // D:YYYYMMDDhhmmss
    let (y, m, day) = (&d[2..6], &d[6..8], &d[8..10]);
    format!("{m}/{day}/{y}")
}

impl HeaderFooterDialog {
    pub fn new(ctx: HeaderFooterContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut hf = ctx.current.clone().unwrap_or_default();
        if ctx.current.is_none() && ctx.bates {
            hf.texts[5] = "<<bates>>".into();
        }
        let bates = hf.bates.clone().unwrap_or_default();
        let range = ctx.scope.clone();
        let texts: Vec<String> = hf
            .texts
            .iter()
            .cloned()
            .chain([
                format!("{}", hf.font_size),
                inches(hf.top),
                inches(hf.bottom),
                inches(hf.left.min(hf.right)),
                format!("{}", hf.first_page_number),
                range,
                bates.prefix.clone(),
                bates.suffix.clone(),
                format!("{}", bates.digits),
                format!("{}", bates.start),
            ])
            .collect();
        let placeholders = [
            "Header left",
            "Header center",
            "Header right",
            "Footer left",
            "Footer center",
            "Footer right",
            "10",
            "0.5",
            "0.5",
            "1",
            "1",
            "All pages",
            "e.g. ABC",
            "",
            "6",
            "1",
        ];
        let fields: Vec<Entity<TextInput>> = (0..FIELDS)
            .map(|i| {
                let text = texts[i].clone();
                cx.new(|cx| {
                    let mut t = TextInput::new(placeholders[i], cx);
                    t.set_text(text, cx);
                    t
                })
            })
            .collect();
        let mut subs: Vec<Subscription> = fields
            .iter()
            .map(|f| {
                cx.subscribe(f, |d, _, ev: &TextInputEvent, cx| match ev {
                    TextInputEvent::Confirm => d.go(cx),
                    TextInputEvent::Cancel => cx.emit(HeaderFooterEvent::Dismiss),
                })
            })
            .collect();
        for f in &fields {
            subs.push(cx.observe(f, |d, _, cx| {
                d.relower(false, cx);
                cx.notify();
            }));
        }
        let focus = cx.focus_handle();
        let first = if ctx.bates && ctx.current.is_none() {
            F_PREFIX
        } else {
            4
        };
        window.focus(&fields[first].read(cx).focus_handle(cx), cx);
        let has_bates = ctx.bates || hf.bates.is_some();
        let mut d = Self {
            focus,
            color: hf.color,
            bates: has_bates,
            now: ctx.opened.clone(),
            ctx,
            fields,
            at: first,
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
        let Ok((hf, pages)) = self.settings(cx) else {
            return;
        };
        let hf = HeaderFooter { pages, ..hf };
        let set = crate::page_insert::header_footer_op(Some(hf), &self.ctx.opened);
        let Some(generation) = self.preview.begin(&set, again) else {
            return;
        };
        let shown = self.now.geom(self.ctx.previewer.page).shown();
        let wide = (PREVIEW_H * shown.width / shown.height.max(1.0)).min(PREVIEW_W);
        let width = (wide * self.scale).round().max(1.0) as u32;
        let (now, lower) = (self.now.clone(), self.ctx.lower.clone());
        let previewer = self.ctx.previewer.clone();
        self.preview.task = Some(cx.spawn(async move |this, cx| {
            let drawn = cx.background_executor().spawn(async move {
                let rx = draw_preview(&now, lower, &previewer, set, width)?;
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
        let shown = self.preview.picture.take().map(|(image, _)| image);
        for image in self.preview.retired.drain(..).chain(shown) {
            let _ = window.drop_image(image);
        }
    }

    fn order(&self) -> Vec<usize> {
        (0..FIELDS)
            .filter(|&i| self.bates || !(F_PREFIX..=F_START).contains(&i))
            .collect()
    }

    fn step(&mut self, by: i64, window: &mut Window, cx: &mut Context<Self>) {
        let order = self.order();
        let at = order.iter().position(|&i| i == self.at).unwrap_or(0) as i64;
        self.at = order[(at + by).rem_euclid(order.len() as i64) as usize];
        let h = self.fields[self.at].read(cx).focus_handle(cx);
        window.focus(&h, cx);
        cx.notify();
    }

    fn text(&self, i: usize, cx: &App) -> String {
        self.fields[i].read(cx).text().trim().to_string()
    }

    /// The header/footer the fields describe, or what's wrong.
    /// The version it opened on.
    pub fn opened(&self) -> &Snapshot {
        &self.ctx.opened
    }

    fn settings(&self, cx: &App) -> Result<(HeaderFooter, Option<Vec<PageId>>), String> {
        let num = |i: usize, name: &str, lo: f32, hi: f32| -> Result<f32, String> {
            let t = self.text(i, cx);
            let v: f32 = t
                .parse()
                .map_err(|_| format!("{name}: '{t}' isn't a number"))?;
            if !(lo..=hi).contains(&v) {
                return Err(format!("{name}: from {lo} to {hi}"));
            }
            Ok(v)
        };
        let mut texts: [String; 6] = Default::default();
        for (i, t) in texts.iter_mut().enumerate() {
            *t = self.fields[i].read(cx).text().to_string();
        }
        if texts.iter().all(|t| t.trim().is_empty()) {
            return Err("Type something in at least one place".into());
        }
        let font_size = num(F_SIZE, "Size", 4.0, 72.0)?;
        let top = num(F_TOP, "Top", 0.0, 5.0)? * 72.0;
        let bottom = num(F_BOTTOM, "Bottom", 0.0, 5.0)? * 72.0;
        let sides = num(F_SIDES, "Sides", 0.0, 5.0)? * 72.0;
        let first = num(F_FIRST, "Start numbers at", 1.0, 1e6)? as u32;
        let pages = match self.text(F_PAGES, cx) {
            t if t.is_empty() => None,
            t => {
                let indexes = sheaf_ops::pages::parse(&t, self.ctx.page_count())
                    .map_err(|e| format!("Pages: {e}"))?;
                Some(crate::page_insert::ids_at(&self.ctx.opened, &indexes))
            }
        };
        let bates = if self.bates {
            let digits = num(F_DIGITS, "Digits", 1.0, 12.0)? as u8;
            let start = self
                .text(F_START, cx)
                .parse::<u64>()
                .map_err(|_| "Bates start must be a whole number".to_string())?;
            Some(Bates {
                prefix: self.fields[F_PREFIX].read(cx).text().to_string(),
                suffix: self.fields[F_SUFFIX].read(cx).text().to_string(),
                digits,
                start,
            })
        } else {
            None
        };
        if bates.is_some() && !texts.iter().any(|t| t.contains("<<bates>>")) {
            return Err("Put <<bates>> where the Bates number goes".into());
        }
        let hf = HeaderFooter {
            texts,
            font_size,
            color: self.color,
            top,
            bottom,
            left: sides,
            right: sides,
            first_page_number: first,
            pages: None,
            date: self
                .ctx
                .current
                .as_ref()
                .map(|c| c.date.clone())
                .filter(|d| !d.is_empty())
                .unwrap_or_else(today),
            file: self.ctx.file.clone(),
            bates,
        };
        Ok((hf, pages))
    }

    fn go(&mut self, cx: &mut Context<Self>) {
        match self.settings(cx) {
            Ok((hf, pages)) => cx.emit(HeaderFooterEvent::Apply(Box::new(hf), pages)),
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
}

impl HeaderFooterDialog {
    /// The preview's picture, at its size, or a box of the page's size
    /// saying why there's none yet.
    fn picture(&self) -> AnyElement {
        if let Some((image, (w, h))) = &self.preview.picture {
            let picture = img(image.clone()).size_full().object_fit(ObjectFit::Fill);
            return (div().w(px(*w)).h(px(*h)))
                .border_1()
                .border_color(theme::BORDER)
                .child(picture)
                .into_any_element();
        }
        let shown = self.now.geom(self.ctx.previewer.page).shown();
        let w = (PREVIEW_H * shown.width / shown.height.max(1.0)).min(PREVIEW_W);
        let said = (self.preview.failed.clone()).unwrap_or_else(|| "Drawing…".into());
        (div().w(px(w)).h(px(PREVIEW_H)).p_2())
            .border_1()
            .border_color(theme::BORDER)
            .child(dialog::subtitle(said))
            .into_any_element()
    }
}

impl Render for HeaderFooterDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for image in self.preview.retired.drain(..) {
            let _ = window.drop_image(image);
        }
        let slots = |base: usize| {
            div()
                .flex()
                .gap_2()
                .children((0..3).map(|k| div().flex_1().child(self.fields[base + k].clone())))
                .into_any_element()
        };
        let colors = div()
            .flex()
            .gap_1()
            .children(COLORS.iter().enumerate().map(|(i, (c, l))| {
                let c = *c;
                Chip::new(("hf-color", i), *l)
                    .selected(c == self.color)
                    .on_click(cx.listener(move |d, _, _, cx| {
                        d.color = c;
                        d.relower(false, cx);
                        cx.notify();
                    }))
            }));
        let row = |label: &str, el: AnyElement| {
            dialog::field_row(label.to_string(), px(110.0), false, el).into_any_element()
        };
        let pair = |a: AnyElement, note: &str| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(a)
                .child(dialog::subtitle(note.to_string()))
                .into_any_element()
        };
        let tokens = TOKENS
            .iter()
            .filter(|(t, _)| self.bates || *t != "<<bates>>")
            .map(|(t, what)| format!("{t} {what}"))
            .collect::<Vec<_>>()
            .join(" · ");
        let bates_toggle = Chip::new(
            ("hf-bates", 0usize),
            format!("{} Bates numbering", if self.bates { "☑" } else { "☐" }),
        )
        .on_click(cx.listener(|d, _, _, cx| {
            d.bates = !d.bates;
            d.error = None;
            d.relower(false, cx);
            cx.notify();
        }))
        .into_any_element();
        let preview = match self.settings(cx) {
            Ok((hf, _)) => {
                let f = sheaf_doc::stamps::PageFacts {
                    index: 0,
                    count: self.ctx.page_count(),
                    bates: hf.bates.as_ref().map(|b| b.start),
                };
                let size = sheaf_edit::PageSize {
                    width: 612.0,
                    height: 792.0,
                };
                let shown: Vec<String> = hf.layout(size, &f).into_iter().map(|p| p.text).collect();
                Ok(format!("Page 1: {}", shown.join("  ·  ")))
            }
            Err(e) => Err(e),
        };
        dialog::backdrop("hf-backdrop")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|d, _: &DialogNext, window, cx| d.step(1, window, cx)))
            .on_action(cx.listener(|d, _: &DialogPrev, window, cx| d.step(-1, window, cx)))
            .on_action(cx.listener(|d, _: &DialogRun, _, cx| d.go(cx)))
            .on_action(
                cx.listener(|_, _: &DialogCancel, _, cx| cx.emit(HeaderFooterEvent::Dismiss)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(HeaderFooterEvent::Dismiss)),
            )
            .child(
                dialog::card("hf-dialog", px(760.0))
                    .child(dialog::header("Header & Footer").child(dialog::subtitle(
                        "shown on the pages now, written into them when you save",
                    )))
                    .child(row("Header", slots(0)))
                    .child(row("Footer", slots(3)))
                    .child(div().px_2().child(dialog::subtitle(tokens)))
                    .child(row("Text", pair(self.field(F_SIZE, 60.0), "pt")))
                    .child(row("Color", colors.into_any_element()))
                    .child(row(
                        "Margins",
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(dialog::subtitle("top"))
                            .child(self.field(F_TOP, 60.0))
                            .child(dialog::subtitle("bottom"))
                            .child(self.field(F_BOTTOM, 60.0))
                            .child(dialog::subtitle("sides"))
                            .child(self.field(F_SIDES, 60.0))
                            .child(dialog::subtitle("in"))
                            .into_any_element(),
                    ))
                    .child(row(
                        "Page numbers",
                        pair(self.field(F_FIRST, 60.0), "the first page's <<1>>"),
                    ))
                    .child(row(
                        "Pages",
                        pair(
                            self.field(F_PAGES, 160.0),
                            &format!("of {} (empty: all)", self.ctx.page_count()),
                        ),
                    ))
                    .child(row("Bates", bates_toggle))
                    .when(self.bates, |d| {
                        d.child(row(
                            "",
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(dialog::subtitle("prefix"))
                                .child(self.field(F_PREFIX, 100.0))
                                .child(dialog::subtitle("suffix"))
                                .child(self.field(F_SUFFIX, 80.0))
                                .child(dialog::subtitle("digits"))
                                .child(self.field(F_DIGITS, 44.0))
                                .child(dialog::subtitle("start"))
                                .child(self.field(F_START, 90.0))
                                .into_any_element(),
                        ))
                    })
                    .child(match (&self.error, preview) {
                        (Some(e), _) => dialog::message(e.clone(), true),
                        (None, Ok(p)) => dialog::message(p, false),
                        (None, Err(e)) => dialog::message(e, true),
                    })
                    .child(row("Preview", self.picture()))
                    .child(
                        dialog::footer()
                            .child(dialog::hint("Tab moves, Enter applies"))
                            .when(self.ctx.current.is_some(), |d| {
                                d.child(Button::new("hf-remove", "Remove").on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(HeaderFooterEvent::Remove)),
                                ))
                            })
                            .child(Button::new("hf-cancel", "Cancel").on_click(
                                cx.listener(|_, _, _, cx| cx.emit(HeaderFooterEvent::Dismiss)),
                            ))
                            .child(
                                Button::new(
                                    "hf-go",
                                    if self.ctx.current.is_some() {
                                        "Update"
                                    } else {
                                        "Add"
                                    },
                                )
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
    use sheaf_ops::views::Views;
    use sheaf_render::EnginePool;

    #[test]
    fn today_is_a_date() {
        let t = super::today();
        assert_eq!(t.len(), 10);
        assert_eq!(&t[2..3], "/");
    }

    /// `bytes` opened as the file `name` in `dir`.
    fn opened(dir: &std::path::Path, name: &str, bytes: &[u8]) -> Document {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        sheaf_doc::open::file(&path, None).unwrap().doc
    }

    /// The Header & Footer dialog's preview is its page as applying the
    /// settings draws it (M10.5 §8.6): on a file sheaf saved with a header
    /// drawn into its pages, an update's preview of the first page is that
    /// page with the old header stripped and the new one drawn, pixel for
    /// pixel the page the apply leaves, not the page as it shows now, nor
    /// the new header drawn over the old; and once the page is gone, the
    /// first page's preview is shown. A lowering's picture shows only
    /// while no later lowering has begun: settings that make the op
    /// lowered last aren't lowered again, an older lowering's picture is
    /// dropped, and a change of the document lowers the same op again.
    #[test]
    fn the_header_dialog_previews_the_header_and_drops_a_stale_result() {
        use sheaf_doc::tx::fixture::{header, set_header};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let blank = sheaf_doc::generated::blank_pages(612.0, 792.0, 2).unwrap();
        let mut saved = opened(dir.path(), "blank.pdf", &blank);
        let old = set_header(header("OLD OLD OLD"), None);
        saved.commit_ops(vec![old], "Header").unwrap();
        let headed = sheaf_doc::save::rewrite(&saved.snapshot()).unwrap();
        let doc = opened(dir.path(), "headed.pdf", &headed);
        let (now, page) = (doc.snapshot(), doc.page(0).unwrap());
        let pool = EnginePool::new().unwrap();
        let previewer = Previewer {
            store: doc.store().clone(),
            views: Views::new(pool.clone()),
            page,
        };
        let mut hf = (**now.state.header_footer().expect("the file's header")).clone();
        hf.texts[0] = "NEW".into();
        let set = crate::page_insert::header_footer_op(Some(hf), &now);
        let lower = || doc.lower_cx(Actor::User);
        let preview = draw_preview(&now, lower(), &previewer, set.clone(), 300).unwrap();
        let preview = block_on(preview).unwrap().unwrap().raster;

        let drawn = |st: &sheaf_doc::DocState| {
            let job = Job::Page {
                width: 300,
                rotation: Rotation::R0,
            };
            let (screen, format) = (ViewInputs::screen(), PixelFormat::Bgra);
            let views = Views::new(pool.clone());
            let rx = views.render(st, page, &screen, job, format, 0, Budget::Screen);
            block_on(rx).unwrap().unwrap().raster.data
        };
        let store = doc.store();
        let copies = sheaf_ops::derive::strip_stamps(store, &now, &NoProgress).unwrap();
        let bases: Vec<_> = copies.iter().flat_map(|c| c.bases()).collect();
        let mut applied = Document::tx_at(&now, lower());
        applied
            .set_page_base(bases, Derive::StripHeaderFooter)
            .unwrap();
        applied.apply(set.clone(), &NoProgress).unwrap();
        assert!(
            preview.data == drawn(applied.state()),
            "as applying draws it"
        );
        assert!(preview.data != drawn(&now.state), "not as it shows now");
        let mut over = Document::tx_at(&now, lower());
        over.apply(set.clone(), &NoProgress).unwrap();
        assert!(
            preview.data != drawn(over.state()),
            "the old header stripped"
        );
        let gone = Previewer {
            page: PageId(u64::MAX),
            ..previewer.clone()
        };
        let first = draw_preview(&now, lower(), &gone, set.clone(), 300).unwrap();
        assert!(block_on(first).unwrap().unwrap().raster.data == preview.data);

        let picture = |width| Raster {
            width,
            height: 4,
            format: PixelFormat::Bgra,
            data: vec![255; width as usize * 16],
        };
        let mut shown = Preview::default();
        let first = shown.begin(&set, false).unwrap();
        assert_eq!(
            shown.begin(&set, false),
            None,
            "the same op isn't lowered again"
        );
        let other = set_header(header("OTHER"), None);
        let second = shown.begin(&other, false).unwrap();
        assert!(
            !shown.take(first, Ok(picture(2)), 2.0),
            "a stale picture is dropped"
        );
        assert!(shown.picture.is_none());
        assert!(shown.take(second, Ok(picture(8)), 2.0));
        assert_eq!(
            shown.picture.as_ref().map(|(_, size)| *size),
            Some((4.0, 2.0))
        );
        let third = shown
            .begin(&other, true)
            .expect("lowered again on a change");
        let late = anyhow::anyhow!("late");
        assert!(!shown.take(second, Err(late), 2.0));
        assert!(shown.take(third, Err(anyhow::anyhow!("no pages")), 2.0));
        assert_eq!(shown.failed.as_deref(), Some("no pages"));
        assert!(shown.picture.is_some(), "the last picture stays");
    }
}
