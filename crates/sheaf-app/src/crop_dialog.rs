//! Document > Crop Pages…: take margins off pages, or trim them to their
//! content. Margins are as the page is shown. Tab moves between rows,
//! Left/Right change, Enter crops, Escape cancels.
//!
//! The dialog previews the page the active pane showed as it opened, as
//! cropping would draw it (M10.5 §8.6, `dialog_preview`): what the dialog
//! asks lowered on the document as it is now ([`draw_preview`]), the
//! margins' op, or the crop Trim White measures, off the UI thread, again
//! at each change of the settings and each change the document goes
//! through ([`CropDialog::follow`]); then the page realized from the state
//! that leaves, as the screen draws it, and its size there in the hint.

use std::sync::Arc;

use anyhow::Context as _;
use futures::channel::oneshot;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton,
    Subscription, Window, div, prelude::*, px,
};
use sheaf_doc::tx::LowerCx;
use sheaf_doc::{Document, PageId, Snapshot, ViewInputs};
use sheaf_edit::{Margins, PageSize, Rotation};
use sheaf_image::raster::PixelFormat;
use sheaf_ops::{NoProgress, crop};
use sheaf_render::{Budget, Job, Rendered};
use sheaf_ui::button::{Button, Chip};
use sheaf_ui::text_input::{TextInput, TextInputEvent};
use sheaf_ui::{dialog, theme};

use crate::dialog_preview::{Preview, drawn_width, why};
use crate::jobs::Previewer;
use crate::{DialogCancel, DialogLeft, DialogNext, DialogPrev, DialogRight, DialogRun};

pub const KEY_CONTEXT: &str = "CropDialog";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Margins,
    Trim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    Inch,
    Mm,
    Pt,
}

impl UnitKind {
    fn points(self) -> f32 {
        match self {
            UnitKind::Inch => 72.0,
            UnitKind::Mm => 72.0 / 25.4,
            UnitKind::Pt => 1.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            UnitKind::Inch => "in",
            UnitKind::Mm => "mm",
            UnitKind::Pt => "pt",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pages {
    Current,
    Selected,
    All,
    Range,
}

pub struct CropContext {
    /// The document as the dialog opened on it: its pages are counted and
    /// named by id there, and the crop is made on that version.
    pub opened: Snapshot,
    /// The user's lowering context on it, which the preview lowers on,
    /// and what the preview is drawn with.
    pub lower: Arc<LowerCx>,
    pub previewer: Previewer,
    pub current: usize,
    /// 0-based, in order.
    pub selected: Vec<usize>,
}

impl CropContext {
    fn page_count(&self) -> usize {
        self.opened.state.page_count()
    }
}

/// What the dialog asks of its pages.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Request {
    /// Margins off each side as the page shows.
    Margins(Margins),
    /// Each page down to its content, [`crop::PAD`] to spare.
    Trim,
}

/// The preview of `what`, as cropping makes it of the page the previewer
/// shows (the first, when `now` hasn't it): lowered on `now` with `lower`
/// as applying lowers it, margins as their op, Trim White as the crop it
/// measures through the previewer's views ([`crop::trim_white`]), first in
/// the screen's budget as a preview's renders are; then that page's size
/// as the state that leaves shows it, and the page drawn from that state
/// by the previewer's views at `scale` device px a logical px, under the
/// screen's inputs, the same way. An error when it can't be cropped so,
/// and why.
fn draw_preview(
    now: &Snapshot,
    lower: Arc<LowerCx>,
    previewer: &Previewer,
    what: Request,
    scale: f32,
) -> anyhow::Result<(PageSize, oneshot::Receiver<anyhow::Result<Rendered>>)> {
    let page = previewer.page_in(now)?;
    let mut tx = Document::tx_at(now, lower.clone());
    match what {
        Request::Margins(m) => {
            let crop = crate::page_insert::crop_op(vec![page], m);
            tx.apply(crop, &NoProgress).map_err(why)?;
        }
        Request::Trim => {
            let (views, urgency) = (&previewer.views, crop::Urgency::Preview);
            let found =
                crop::trim_white(views, now, lower, &[page], crop::PAD, urgency, &NoProgress)?;
            found.lower(&mut tx).map_err(why)?;
        }
    }
    let shown = (tx.state().geom(page)).context("the page is gone")?.shown();
    let job = Job::Page {
        width: drawn_width(shown, scale),
        rotation: Rotation::R0,
    };
    let (st, screen, format) = (tx.state(), ViewInputs::screen(), PixelFormat::Bgra);
    let drawn = (previewer.views).render(st, page, &screen, job, format, 0, Budget::Screen);
    Ok((shown, drawn))
}

/// The size of a page that shows `shown`, in `unit`, as the hint says it.
fn size_in(shown: PageSize, unit: UnitKind) -> String {
    let u = unit.points();
    let (w, h) = (shown.width / u, shown.height / u);
    format!("{w:.2} × {h:.2} {}", unit.label())
}

/// Each names pages of the version the dialog opened on
/// ([`CropDialog::opened`]).
pub enum CropEvent {
    /// Crop them by `margins`, off each side as it shows.
    Margins {
        pages: Vec<PageId>,
        margins: Margins,
    },
    /// Trim them to their content.
    Trim {
        pages: Vec<PageId>,
    },
    Dismiss,
}

const ROW_METHOD: usize = 0;
const ROW_UNIT: usize = 1;
const ROW_TOP: usize = 2;
const ROW_BOTTOM: usize = 3;
const ROW_LEFT: usize = 4;
const ROW_RIGHT: usize = 5;
const ROW_PAGES: usize = 6;
const ROW_RANGE: usize = 7;

pub struct CropDialog {
    focus: FocusHandle,
    ctx: CropContext,
    /// The document as it is now, which the preview is lowered on.
    now: Snapshot,
    method: Method,
    unit: UnitKind,
    /// Top, bottom, left, right.
    sides: [Entity<TextInput>; 4],
    pages: Pages,
    range: Entity<TextInput>,
    row: usize,
    error: Option<String>,
    preview: Preview<Request>,
    /// The previewed page's size as the newest picture shows it.
    shown: Option<PageSize>,
    /// Device px a logical px, as the dialog opened: what the preview is
    /// drawn at.
    scale: f32,
    _subs: Vec<Subscription>,
}

impl EventEmitter<CropEvent> for CropDialog {}

impl Focusable for CropDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl CropDialog {
    pub fn new(ctx: CropContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let side = |cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut t = TextInput::new("0", cx);
                t.set_text("0", cx);
                t
            })
        };
        let sides = [side(cx), side(cx), side(cx), side(cx)];
        let range = cx.new(|cx| TextInput::new("e.g. 1-3, 7, 10-", cx));
        let mut subs: Vec<Subscription> = sides
            .iter()
            .map(|i| {
                cx.subscribe(i, |d, _, ev: &TextInputEvent, cx| match ev {
                    TextInputEvent::Confirm => d.go(cx),
                    TextInputEvent::Cancel => cx.emit(CropEvent::Dismiss),
                })
            })
            .collect();
        subs.push(
            cx.subscribe(&range, |d, _, ev: &TextInputEvent, cx| match ev {
                TextInputEvent::Confirm => {
                    d.pages = Pages::Range;
                    d.go(cx)
                }
                TextInputEvent::Cancel => cx.emit(CropEvent::Dismiss),
            }),
        );
        for i in &sides {
            subs.push(cx.observe(i, |d, _, cx| {
                d.relower(false, cx);
                cx.notify();
            }));
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let pages = if ctx.selected.len() > 1 {
            Pages::Selected
        } else {
            Pages::Current
        };
        let mut d = Self {
            focus,
            now: ctx.opened.clone(),
            ctx,
            method: Method::Margins,
            unit: UnitKind::Inch,
            sides,
            pages,
            range,
            row: ROW_METHOD,
            error: None,
            preview: Preview::default(),
            shown: None,
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

    /// Lower the preview of what the dialog asks again, off the UI thread,
    /// when it asks other than the last lowered, or `again`; fields with
    /// something wrong leave the last picture.
    fn relower(&mut self, again: bool, cx: &mut Context<Self>) {
        let Ok(what) = self.asked(cx) else {
            return;
        };
        let Some(generation) = self.preview.begin(&what, again) else {
            return;
        };
        let (now, lower) = (self.now.clone(), self.ctx.lower.clone());
        let (previewer, scale) = (self.ctx.previewer.clone(), self.scale);
        self.preview.task = Some(cx.spawn(async move |this, cx| {
            let drawn = cx.background_executor().spawn(async move {
                let (shown, rx) = draw_preview(&now, lower, &previewer, what, scale)?;
                Ok((
                    shown,
                    rx.await.context("the render thread is gone")??.raster,
                ))
            });
            let (shown, drawn) = match drawn.await {
                Ok((shown, raster)) => (Some(shown), Ok(raster)),
                Err(e) => (None, Err(e)),
            };
            this.update(cx, |d, cx| {
                if d.preview.take(generation, drawn, d.scale) {
                    d.shown = shown;
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

    fn rows(&self) -> Vec<usize> {
        let mut rows = vec![ROW_METHOD];
        if self.method == Method::Margins {
            rows.extend([ROW_UNIT, ROW_TOP, ROW_BOTTOM, ROW_LEFT, ROW_RIGHT]);
        }
        rows.extend([ROW_PAGES, ROW_RANGE]);
        rows
    }

    fn input_for(&self, row: usize) -> Option<&Entity<TextInput>> {
        match row {
            ROW_TOP => Some(&self.sides[0]),
            ROW_BOTTOM => Some(&self.sides[1]),
            ROW_LEFT => Some(&self.sides[2]),
            ROW_RIGHT => Some(&self.sides[3]),
            ROW_RANGE => Some(&self.range),
            _ => None,
        }
    }

    fn set_row(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.row = row;
        let target = match self.input_for(row) {
            Some(i) => i.read(cx).focus_handle(cx),
            None => self.focus.clone(),
        };
        window.focus(&target, cx);
        cx.notify();
    }

    fn step_row(&mut self, by: i64, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.rows();
        let at = rows.iter().position(|&r| r == self.row).unwrap_or(0) as i64;
        let next = rows[(at + by).rem_euclid(rows.len() as i64) as usize];
        self.set_row(next, window, cx);
    }

    fn adjust(&mut self, by: i64, cx: &mut Context<Self>) {
        fn cycle<T: Copy + PartialEq>(items: &[T], current: T, by: i64) -> T {
            let i = items.iter().position(|x| *x == current).unwrap_or(0) as i64;
            items[(i + by).rem_euclid(items.len() as i64) as usize]
        }
        match self.row {
            ROW_METHOD => self.method = cycle(&[Method::Margins, Method::Trim], self.method, by),
            ROW_UNIT => {
                let old = self.unit;
                self.unit = cycle(&[UnitKind::Inch, UnitKind::Mm, UnitKind::Pt], old, by);
                self.convert(old, cx);
            }
            ROW_PAGES => {
                let opts: Vec<Pages> = [Pages::Current, Pages::Selected, Pages::All, Pages::Range]
                    .into_iter()
                    .filter(|p| *p != Pages::Selected || !self.ctx.selected.is_empty())
                    .collect();
                self.pages = cycle(&opts, self.pages, by);
            }
            _ => {}
        }
        self.error = None;
        self.relower(false, cx);
        cx.notify();
    }

    /// Rewrite the margin fields from `old` units into the current ones.
    fn convert(&mut self, old: UnitKind, cx: &mut Context<Self>) {
        let unit = self.unit;
        for i in &self.sides {
            let v: Option<f32> = i.read(cx).text().trim().parse().ok();
            if let Some(v) = v {
                let pt = v * old.points();
                let shown = pt / unit.points();
                let text = format!("{}", (shown * 100.0).round() / 100.0);
                i.update(cx, |t, cx| t.set_text(text, cx));
            }
        }
    }

    /// The version it opened on.
    pub fn opened(&self) -> &Snapshot {
        &self.ctx.opened
    }

    fn margins(&self, cx: &App) -> Result<Margins, String> {
        let names = ["Top", "Bottom", "Left", "Right"];
        let mut v = [0f32; 4];
        for (k, i) in self.sides.iter().enumerate() {
            let t = i.read(cx).text().trim().to_string();
            let n: f32 = if t.is_empty() {
                0.0
            } else {
                t.parse()
                    .map_err(|_| format!("{}: '{t}' isn't a number", names[k]))?
            };
            if !(0.0..=10_000.0).contains(&n) {
                return Err(format!("{}: margins can't be negative", names[k]));
            }
            v[k] = n * self.unit.points();
        }
        Ok(Margins {
            top: v[0],
            bottom: v[1],
            left: v[2],
            right: v[3],
        })
    }

    fn page_list(&self, cx: &App) -> Result<Vec<usize>, String> {
        let n = self.ctx.page_count();
        match self.pages {
            Pages::Current => Ok(vec![self.ctx.current.min(n.saturating_sub(1))]),
            Pages::Selected => Ok(self.ctx.selected.clone()),
            Pages::All => Ok((0..n).collect()),
            Pages::Range => sheaf_ops::pages::parse(self.range.read(cx).text(), n),
        }
    }

    /// What the fields ask, margins of zero too, or what's wrong with
    /// them: what the preview shows.
    fn asked(&self, cx: &App) -> Result<Request, String> {
        match self.method {
            Method::Trim => Ok(Request::Trim),
            Method::Margins => self.margins(cx).map(Request::Margins),
        }
    }

    /// What the dialog asks, or why it can't ask it.
    fn request(&self, cx: &App) -> Result<Request, String> {
        match self.asked(cx)? {
            Request::Margins(m) if [m.top, m.bottom, m.left, m.right] == [0.0; 4] => {
                Err("Every margin is zero: nothing to crop".into())
            }
            asked => Ok(asked),
        }
    }

    /// The hint line: what the fields ask makes of the previewed page, its
    /// size as the newest picture shows it, or what's wrong with them or
    /// with the crop they make of it (as an error).
    fn hint(&self, cx: &App) -> Result<String, String> {
        let asked = self.asked(cx)?;
        if let Some(why) = &self.preview.failed {
            return Err(why.clone());
        }
        let trim = "Each page is cropped to its content, with an eighth of an inch to spare";
        Ok(match (asked, self.shown.map(|s| size_in(s, self.unit))) {
            (Request::Trim, Some(size)) => format!("{trim}: the page below becomes {size}"),
            (Request::Trim, None) => trim.into(),
            (Request::Margins(_), Some(size)) => format!("The page below becomes {size}"),
            (Request::Margins(_), None) => "Margins come off each side as the page shows".into(),
        })
    }

    fn go(&mut self, cx: &mut Context<Self>) {
        match (self.request(cx), self.page_list(cx)) {
            (Ok(request), Ok(pages)) if !pages.is_empty() => {
                let pages = crate::page_insert::ids_at(&self.ctx.opened, &pages);
                cx.emit(match request {
                    Request::Margins(margins) => CropEvent::Margins { pages, margins },
                    Request::Trim => CropEvent::Trim { pages },
                })
            }
            (Err(e), _) | (_, Err(e)) => {
                self.error = Some(e);
                cx.notify();
            }
            _ => {
                self.error = Some("No pages to crop".into());
                cx.notify();
            }
        }
    }

    fn row_el(&self, index: usize, label: &str, content: AnyElement) -> AnyElement {
        dialog::field_row(label.to_string(), px(90.0), self.row == index, content)
            .into_any_element()
    }

    fn chips<T: Copy + PartialEq + 'static>(
        &self,
        name: &'static str,
        row: usize,
        opts: Vec<(T, String, bool)>,
        current: T,
        set: fn(&mut Self, T, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(
                opts.into_iter()
                    .enumerate()
                    .map(|(i, (v, label, enabled))| {
                        Chip::new((name, i), label)
                            .selected(v == current)
                            .disabled(!enabled)
                            .on_click(cx.listener(move |d, _, window, cx| {
                                if enabled {
                                    set(d, v, cx);
                                    d.error = None;
                                    d.relower(false, cx);
                                    d.set_row(row, window, cx);
                                }
                            }))
                    }),
            )
            .into_any_element()
    }
}

impl Render for CropDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.preview.drop_retired(window);
        let picture = (self.preview).element(self.now.geom(self.ctx.previewer.page).shown());
        let method = self.chips(
            "crop-method",
            ROW_METHOD,
            vec![
                (Method::Margins, "Margins".into(), true),
                (Method::Trim, "Trim white margins".into(), true),
            ],
            self.method,
            |d, v, _| d.method = v,
            cx,
        );
        let unit = self.chips(
            "crop-unit",
            ROW_UNIT,
            [UnitKind::Inch, UnitKind::Mm, UnitKind::Pt]
                .into_iter()
                .map(|u| (u, u.label().to_string(), true))
                .collect(),
            self.unit,
            |d, v, cx| {
                let old = d.unit;
                d.unit = v;
                d.convert(old, cx);
            },
            cx,
        );
        let side = |i: usize| -> AnyElement {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().w(px(100.0)).child(self.sides[i].clone()))
                .child(dialog::subtitle(self.unit.label()))
                .into_any_element()
        };
        let n = self.ctx.page_count();
        let pages = self.chips(
            "crop-pages",
            ROW_PAGES,
            vec![
                (
                    Pages::Current,
                    format!("Current page ({})", self.ctx.current + 1),
                    true,
                ),
                (
                    Pages::Selected,
                    format!("Selected ({})", self.ctx.selected.len()),
                    !self.ctx.selected.is_empty(),
                ),
                (Pages::All, format!("All ({n})"), true),
                (Pages::Range, "Pages…".into(), true),
            ],
            self.pages,
            |d, v, _| d.pages = v,
            cx,
        );
        let range = div()
            .w(px(200.0))
            .child(self.range.clone())
            .into_any_element();
        let hint = match &self.error {
            Some(e) => Err(e.clone()),
            None => self.hint(cx),
        };
        dialog::backdrop("crop-backdrop")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|d, _: &DialogNext, window, cx| d.step_row(1, window, cx)))
            .on_action(cx.listener(|d, _: &DialogPrev, window, cx| d.step_row(-1, window, cx)))
            .on_action(cx.listener(|d, _: &DialogLeft, _, cx| d.adjust(-1, cx)))
            .on_action(cx.listener(|d, _: &DialogRight, _, cx| d.adjust(1, cx)))
            .on_action(cx.listener(|d, _: &DialogRun, _, cx| d.go(cx)))
            .on_action(cx.listener(|_, _: &DialogCancel, _, cx| cx.emit(CropEvent::Dismiss)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(CropEvent::Dismiss)),
            )
            .child(
                dialog::card("crop-dialog", px(600.0))
                    .child(dialog::header("Crop Pages").child(dialog::subtitle(
                        "what's cropped away is hidden, not deleted",
                    )))
                    .child(self.row_el(ROW_METHOD, "Crop to", method))
                    .when(self.method == Method::Margins, |d| {
                        d.child(self.row_el(ROW_UNIT, "Units", unit))
                            .child(self.row_el(ROW_TOP, "Top", side(0)))
                            .child(self.row_el(ROW_BOTTOM, "Bottom", side(1)))
                            .child(self.row_el(ROW_LEFT, "Left", side(2)))
                            .child(self.row_el(ROW_RIGHT, "Right", side(3)))
                    })
                    .child(self.row_el(ROW_PAGES, "Pages", pages))
                    .child(self.row_el(ROW_RANGE, "", range))
                    .child(match hint {
                        Ok(hint) => dialog::message(hint, false),
                        Err(e) => dialog::message(e, true),
                    })
                    .child(dialog::field_row(
                        "Preview".to_string(),
                        px(90.0),
                        false,
                        picture,
                    ))
                    .child(
                        dialog::footer()
                            .child(dialog::hint("Tab moves, ←/→ change, Enter crops"))
                            .child(
                                Button::new("crop-cancel", "Cancel").on_click(
                                    cx.listener(|_, _, _, cx| cx.emit(CropEvent::Dismiss)),
                                ),
                            )
                            .child(
                                Button::new("crop-go", "Crop")
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
    use sheaf_doc::tx::fixture as fx;
    use sheaf_doc::{Actor, DocState};
    use sheaf_image::raster::Raster;
    use sheaf_ops::views::Views;
    use sheaf_render::EnginePool;

    /// Two Letter pages, a black square at user (300..400, 300..400) on
    /// the first and a block at (100..250, 500..600) on the second, opened
    /// from `dir` as the app opens them.
    fn blocks(dir: &std::path::Path) -> Document {
        use lopdf::{Document as Pdf, Object, Stream, dictionary};
        let mut pdf = Pdf::with_version("1.7");
        let root = pdf.new_object_id();
        let drawn: [&[u8]; 2] = [
            b"0 0 0 rg 300 300 100 100 re f",
            b"0 0 0 rg 100 500 150 100 re f",
        ];
        let kids: Vec<Object> = (drawn.into_iter())
            .map(|content| {
                let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
                let page =
                    dictionary! { "Type" => "Page", "Parent" => root, "Contents" => content };
                pdf.add_object(page).into()
            })
            .collect();
        let pages = dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => 2,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        };
        pdf.objects.insert(root, Object::Dictionary(pages));
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        pdf.trailer.set("Root", catalog);
        let path = dir.join("blocks.pdf");
        pdf.save(&path).unwrap();
        sheaf_doc::open::file(&path, None).unwrap().doc
    }

    /// The Crop Pages dialog's preview is its page as cropping draws it
    /// (M10.5 §8.6): by margins, pixel for pixel the page the op leaves,
    /// not the page as it shows now, drawn at the cropped page's own
    /// aspect, its size there what the hint says; by Trim White, the page
    /// the crop it measures leaves; margins that leave too little of it,
    /// the reason; and once the page is gone, the first page's preview. A
    /// lowering's picture shows only while no later lowering has begun:
    /// a request lowered last isn't lowered again, an older lowering's
    /// picture is dropped, and a change of the document lowers the same
    /// request again.
    #[test]
    fn the_crop_dialog_previews_the_crop_and_drops_a_stale_result() {
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let doc = blocks(dir.path());
        let (now, page) = (doc.snapshot(), doc.page(1).unwrap());
        let pool = EnginePool::new().unwrap();
        let previewer = Previewer {
            store: doc.store().clone(),
            views: Views::new(pool.clone()),
            page,
        };
        let lower = || doc.lower_cx(Actor::User);
        let preview = |previewer: &Previewer, what| {
            let (shown, rx) = draw_preview(&now, lower(), previewer, what, 2.0).unwrap();
            (shown, block_on(rx).unwrap().unwrap().raster)
        };
        let drawn = |st: &DocState, width| {
            let job = Job::Page {
                width,
                rotation: Rotation::R0,
            };
            let (screen, format) = (ViewInputs::screen(), PixelFormat::Bgra);
            let views = Views::new(pool.clone());
            let rx = views.render(st, page, &screen, job, format, 0, Budget::Screen);
            block_on(rx).unwrap().unwrap().raster.data
        };

        let m = Margins {
            top: 72.0,
            bottom: 144.0,
            left: 36.0,
            right: 108.0,
        };
        let (shown, cropped) = preview(&previewer, Request::Margins(m));
        let mut applied = Document::tx_at(&now, lower());
        applied.apply(fx::crop(&[page], m), &NoProgress).unwrap();
        let g = applied.state().geom(page).unwrap().shown();
        assert_eq!(shown, g);
        let (w, h) = (468.0, 576.0);
        assert_eq!((g.width, g.height), (w, h));
        assert_eq!(size_in(shown, UnitKind::Inch), "6.50 × 8.00 in");
        assert_eq!(cropped.width, drawn_width(shown, 2.0));
        let aspect = cropped.height as f32 / cropped.width as f32;
        assert!((aspect - h / w).abs() < 0.01, "the cropped page's aspect");
        let width = cropped.width;
        assert!(
            cropped.data == drawn(applied.state(), width),
            "as applying draws it"
        );
        assert!(
            cropped.data != drawn(&now.state, width),
            "not as it shows now"
        );

        let (trim_shown, trimmed) = preview(&previewer, Request::Trim);
        let views = Views::new(pool.clone());
        let urgency = crop::Urgency::Background;
        let found = crop::trim_white(
            &views,
            &now,
            lower(),
            &[page],
            crop::PAD,
            urgency,
            &NoProgress,
        );
        let found = found.unwrap();
        assert_eq!(found.crops.len(), 1, "{found:?}");
        let mut trim = Document::tx_at(&now, lower());
        found.lower(&mut trim).unwrap();
        let g = trim.state().geom(page).unwrap().shown();
        assert_eq!(trim_shown, g);
        let block = (150.0 + 2.0 * crop::PAD, 100.0 + 2.0 * crop::PAD);
        assert!((g.width - block.0).abs() <= 1.0, "{g:?}");
        assert!((g.height - block.1).abs() <= 1.0, "{g:?}");
        assert!(
            trimmed.data == drawn(trim.state(), trimmed.width),
            "as Trim White draws it"
        );

        let all = Margins {
            top: 400.0,
            bottom: 400.0,
            left: 0.0,
            right: 0.0,
        };
        let too_much = draw_preview(&now, lower(), &previewer, Request::Margins(all), 2.0);
        let why = too_much.expect_err("nothing left of the page");
        assert_eq!(format!("{why:#}"), "those margins leave nothing of page 2");

        let first = Previewer {
            page: doc.page(0).unwrap(),
            ..previewer.clone()
        };
        let gone = Previewer {
            page: PageId(u64::MAX),
            ..previewer.clone()
        };
        let (_, of_first) = preview(&first, Request::Margins(m));
        let (_, of_gone) = preview(&gone, Request::Margins(m));
        assert!(of_gone.data == of_first.data, "the first page's");
        assert!(of_first.data != cropped.data);

        let picture = |width| Raster {
            width,
            height: 4,
            format: PixelFormat::Bgra,
            data: vec![255; width as usize * 16],
        };
        let mut shown = Preview::default();
        let first = shown.begin(&Request::Margins(m), false).unwrap();
        assert_eq!(
            shown.begin(&Request::Margins(m), false),
            None,
            "the same request isn't lowered again"
        );
        let second = shown.begin(&Request::Trim, false).unwrap();
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
        let third = (shown.begin(&Request::Trim, true)).expect("lowered again on a change");
        let late = anyhow::anyhow!("late");
        assert!(!shown.take(second, Err(late), 2.0));
        let failed = anyhow::anyhow!("those margins leave nothing of page 2");
        assert!(shown.take(third, Err(failed), 2.0));
        assert_eq!(
            shown.failed.as_deref(),
            Some("those margins leave nothing of page 2")
        );
        assert!(shown.picture.is_some(), "the last picture stays");
    }
}
