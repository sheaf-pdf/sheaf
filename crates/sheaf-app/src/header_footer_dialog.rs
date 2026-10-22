//! Document > Header & Footer… and Bates Numbering…: text in six places
//! (left, center, right of the header and of the footer) with tokens for
//! the page number, page count, page label, date, file name and Bates
//! number: the document's header and footer (M10.5 slice 17), set by one op
//! and taken off by another. Shown over the pages until the document is
//! saved. Tab moves between fields, Enter applies, Escape cancels.
//!
//! The dialog starts from the header and footer the document has, as the
//! model has it: a file sheaf saved has its own found again as it opens. It
//! previews the page the active pane showed as it opened, as applying
//! would draw it (M10.5 §8.6, `dialog_preview`): the settings' op lowered
//! on the document as it is now ([`draw_preview`]), off the UI thread,
//! again at each change of the settings and each change the document goes
//! through ([`HeaderFooterDialog::follow`]), and the page realized from the
//! state that leaves, as the screen draws it; the header set again replaces
//! the one drawn there, and those Acrobat or an older sheaf drew. It says
//! what others drew of headers and footers on the selected pages, or all
//! of them, which applying replaces and Remove takes off, counted once off
//! the UI thread as it opens (`generator_dialog::Count`; proposal 31,
//! D61).

use std::sync::Arc;

use anyhow::Context as _;
use futures::channel::oneshot;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton,
    Subscription, Window, div, prelude::*, px,
};
use sheaf_doc::generate::header::{self, TOKENS};
use sheaf_doc::model::{Frozen, PageCtx};
use sheaf_doc::tx::LowerCx;
use sheaf_doc::{DocState, Document, Snapshot, ViewInputs};
use sheaf_edit::{
    BatesSpec, Color, FontChoice, HeaderFooterSpec, Margins, Op, PageSel, Rotation, ScopeSel,
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

pub const KEY_CONTEXT: &str = "HeaderFooterDialog";

const COLORS: [(Color, &str); 4] = [
    ([0, 0, 0], "Black"),
    ([110, 110, 110], "Gray"),
    ([20, 60, 170], "Blue"),
    ([190, 20, 20], "Red"),
];

/// Each names pages of the version the dialog opened on
/// ([`HeaderFooterDialog::opened`]).
pub enum HeaderFooterEvent {
    /// The op that sets the settings on the pages they name.
    Apply(Box<Op>),
    /// Take the header and footer off every page: others' too (§6.5, D61),
    /// and the author's own with `include_authored`.
    Remove {
        include_authored: bool,
    },
    Dismiss,
}

pub struct HeaderFooterContext {
    /// The document as the dialog opened on it: its pages are counted and
    /// named by id there, its header and footer is what the dialog starts
    /// from, and the change is made on that version.
    pub opened: Snapshot,
    /// The user's lowering context on it, which the preview lowers on,
    /// and what the preview is drawn with.
    pub lower: Arc<LowerCx>,
    pub previewer: Previewer,
    /// Opened as Bates Numbering…: a new header and footer starts with
    /// Bates numbering on.
    pub bates: bool,
    /// The selected pages (0-based), which the dialog counts others' on:
    /// none for all of them.
    pub selected: Vec<usize>,
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

/// The preview of `set`, the op applying the dialog's settings makes:
/// lowered on `now` with `lower` as applying lowers it; then the page the
/// previewer shows (the first, when `now` hasn't it) of the state that
/// leaves, drawn `width` device px wide by the previewer's views under the
/// screen's inputs, in the screen's budget. An error when it can't be set
/// so, and why.
fn draw_preview(
    now: &Snapshot,
    lower: Arc<LowerCx>,
    previewer: &Previewer,
    set: Op,
    width: u32,
) -> anyhow::Result<oneshot::Receiver<anyhow::Result<Rendered>>> {
    let page = previewer.page_in(now)?;
    let mut tx = Document::tx_at(now, lower);
    tx.apply(set, &NoProgress).map_err(why)?;
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
    /// Whether the document had a header and footer as the dialog opened,
    /// which applying updates.
    current: bool,
    fields: Vec<Entity<TextInput>>,
    font: FontChoice,
    color: Color,
    bates: bool,
    /// Remove takes the author's own off too.
    include_authored: bool,
    /// What others drew of headers and footers on the pages `counted_on`
    /// names.
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

/// What the dialog starts from: the document's header and footer, as the
/// model has it, or a new one's defaults.
struct Start {
    /// Whether there's one, which applying updates.
    current: bool,
    spec: HeaderFooterSpec,
    /// Whether Bates numbering is on.
    bates: bool,
    /// The pages it's on, as the Pages field says them: empty for all.
    pages: String,
}

impl Start {
    /// A new header and footer: nothing in the slots, 10 pt black
    /// Helvetica, half an inch from the top and bottom and an inch from the
    /// sides, numbered from 1, on every page; with `bates`, a Bates number
    /// at the footer's right, numbered from the defaults.
    fn new(bates: bool) -> Start {
        let mut spec = HeaderFooterSpec {
            texts: Default::default(),
            font: FontChoice::Helvetica,
            size: 10.0,
            color: [0, 0, 0],
            margins: Margins {
                top: 36.0,
                bottom: 36.0,
                left: 72.0,
                right: 72.0,
            },
            first_number: 1,
            bates: None,
        };
        if bates {
            spec.texts[5] = "<<bates>>".into();
        }
        Start {
            current: false,
            spec,
            bates,
            pages: String::new(),
        }
    }

    /// What `st`'s header and footer sets, or a new one's defaults; Bates
    /// numbering on when it numbers pages, or `bates` asks for it.
    fn at(st: &DocState, bates: bool) -> Start {
        let on = st
            .header_footer()
            .and_then(|g| Some((g, g.spec.header_footer()?)));
        let Some((g, spec)) = on else {
            return Start::new(bates);
        };
        Start {
            current: true,
            spec: HeaderFooterSpec::clone(spec),
            bates: bates || spec.bates.is_some(),
            pages: scope_text(st, &g.scope),
        }
    }
}

impl HeaderFooterDialog {
    pub fn new(ctx: HeaderFooterContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let start = Start::at(&ctx.opened.state, ctx.bates);
        let hf = &start.spec;
        let bates = hf.bates.clone().unwrap_or_default();
        let margins = &hf.margins;
        let texts: Vec<String> = hf
            .texts
            .iter()
            .cloned()
            .chain([
                format!("{}", hf.size),
                inches(margins.top),
                inches(margins.bottom),
                inches(margins.left.min(margins.right)),
                format!("{}", hf.first_number),
                start.pages.clone(),
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
        let first = if ctx.bates && !start.current {
            F_PREFIX
        } else {
            4
        };
        window.focus(&fields[first].read(cx).focus_handle(cx), cx);
        let (pages, counted_on) = counted_pages(&ctx.opened, &ctx.selected);
        let count = Count::begin(
            &ctx.opened,
            header::kinds(),
            pages,
            |d: &mut Self| &mut d.count,
            cx,
        );
        let mut d = Self {
            focus: cx.focus_handle(),
            now: ctx.opened.clone(),
            current: start.current,
            fields,
            font: start.spec.font,
            color: start.spec.color,
            bates: start.bates,
            include_authored: false,
            count,
            counted_on,
            ctx,
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
        self.preview.release(window);
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

    /// The version it opened on.
    pub fn opened(&self) -> &Snapshot {
        &self.ctx.opened
    }

    /// Whether it opened as Bates Numbering….
    pub fn bates(&self) -> bool {
        self.ctx.bates
    }

    /// The header and footer the fields describe, and the pages it's on of
    /// the version the dialog opened on, or what's wrong with them.
    fn settings(&self, cx: &App) -> Result<(HeaderFooterSpec, ScopeSel), String> {
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
        let size = num(F_SIZE, "Size", 4.0, 72.0)?;
        let top = num(F_TOP, "Top", 0.0, 5.0)? * 72.0;
        let bottom = num(F_BOTTOM, "Bottom", 0.0, 5.0)? * 72.0;
        let sides = num(F_SIDES, "Sides", 0.0, 5.0)? * 72.0;
        let first = num(F_FIRST, "Start numbers at", 1.0, 1e6)? as u32;
        let scope = match self.text(F_PAGES, cx) {
            t if t.is_empty() => ScopeSel::All,
            t => {
                let indexes = sheaf_ops::pages::parse(&t, self.ctx.page_count())
                    .map_err(|e| format!("Pages: {e}"))?;
                let ids = crate::page_insert::ids_at(&self.ctx.opened, &indexes);
                ScopeSel::Pages(PageSel::ids(ids))
            }
        };
        let bates = if self.bates {
            let digits = num(F_DIGITS, "Digits", 1.0, 12.0)? as u8;
            let start = self
                .text(F_START, cx)
                .parse::<u64>()
                .map_err(|_| "Bates start must be a whole number".to_string())?;
            Some(BatesSpec {
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
        let hf = HeaderFooterSpec {
            texts,
            font: self.font,
            size,
            color: self.color,
            margins: Margins {
                top,
                bottom,
                left: sides,
                right: sides,
            },
            first_number: first,
            bates,
        };
        Ok((hf, scope))
    }

    /// The op the settings make, setting the header and footer on the pages
    /// they name of the version the dialog opened on, or what's wrong with
    /// them.
    fn asked(&self, cx: &App) -> Result<Op, String> {
        let (spec, scope) = self.settings(cx)?;
        Ok(Op::SetHeaderFooter { spec, scope })
    }

    /// What `<<date>>` and `<<file>>` say: what the header there is fixed,
    /// or now and the document's name, as applying fixes them.
    fn frozen(&self) -> Frozen {
        let host = self.ctx.lower.host();
        match self.ctx.opened.state.header_footer() {
            Some(g) if !g.frozen.date.is_empty() => Frozen {
                bates: None,
                ..g.frozen.clone()
            },
            _ => Frozen {
                date: host.date.clone(),
                file: host.file.clone(),
                bates: None,
            },
        }
    }

    /// The document's first page as `hf`'s tokens read it, its Bates
    /// number the first.
    fn first(&self, hf: &HeaderFooterSpec) -> PageCtx {
        let st = &self.ctx.opened.state;
        let label = st.page_at(0).and_then(|p| st.label_of(p));
        PageCtx {
            geom: sheaf_doc::geom::Geom::read(None, None, 0),
            position: 0,
            count: self.ctx.page_count() as u32,
            label: label.unwrap_or_else(|| "1".into()),
            bates: hf.bates.as_ref().map(|b| b.start),
        }
    }

    fn go(&mut self, cx: &mut Context<Self>) {
        match self.asked(cx) {
            Ok(set) => cx.emit(HeaderFooterEvent::Apply(Box::new(set))),
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

impl Render for HeaderFooterDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.preview.drop_retired(window);
        let picture = (self.preview).element(self.now.geom(self.ctx.previewer.page).shown());
        let slots = |base: usize| {
            div()
                .flex()
                .gap_2()
                .children((0..3).map(|k| div().flex_1().child(self.fields[base + k].clone())))
                .into_any_element()
        };
        let (family, style) = font_at(self.font);
        let families = self.chips(
            "hf-family",
            (FAMILIES.iter().enumerate())
                .map(|(i, f)| (FONTS[i][style], f.to_string()))
                .collect(),
            self.font,
            |d, f| d.font = f,
            cx,
        );
        let styles = self.chips(
            "hf-style",
            (STYLES.iter().enumerate())
                .map(|(i, s)| (FONTS[family][i], s.to_string()))
                .collect(),
            self.font,
            |d, f| d.font = f,
            cx,
        );
        let colors = self.chips(
            "hf-color",
            (COLORS.iter()).map(|(c, l)| (*c, l.to_string())).collect(),
            self.color,
            |d, c| d.color = c,
            cx,
        );
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
        let check = |on: bool| if on { "☑" } else { "☐" };
        let bates_toggle = Chip::new(
            ("hf-bates", 0usize),
            format!("{} Bates numbering", check(self.bates)),
        )
        .on_click(cx.listener(|d, _, _, cx| {
            d.bates = !d.bates;
            d.error = None;
            d.relower(false, cx);
            cx.notify();
        }))
        .into_any_element();
        let authored = Chip::new(
            ("hf-authored", 0usize),
            format!("{} the author's own too", check(self.include_authored)),
        )
        .on_click(cx.listener(|d, _, _, cx| {
            d.include_authored = !d.include_authored;
            cx.notify();
        }))
        .into_any_element();
        let found = self.count.said(&self.counted_on, "header or footer");
        let preview = match self.settings(cx) {
            Ok((hf, _)) => {
                let shown: Vec<String> = (header::slots(&hf, &self.frozen(), &self.first(&hf)))
                    .map(|(_, text)| text)
                    .collect();
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
                    .child(row("Font", families))
                    .child(row("", styles))
                    .child(row("Text", pair(self.field(F_SIZE, 60.0), "pt")))
                    .child(row("Color", colors))
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
                    .child(row("Found", dialog::subtitle(found).into_any_element()))
                    .child(row("Remove", authored))
                    .child(match (&self.error, preview) {
                        (Some(e), _) => dialog::message(e.clone(), true),
                        (None, Ok(p)) => dialog::message(p, false),
                        (None, Err(e)) => dialog::message(e, true),
                    })
                    .child(row("Preview", picture))
                    .child(
                        dialog::footer()
                            .child(dialog::hint("Tab moves, Enter applies"))
                            .child(Button::new("hf-remove", "Remove").on_click(cx.listener(
                                |d, _, _, cx| {
                                    let include_authored = d.include_authored;
                                    cx.emit(HeaderFooterEvent::Remove { include_authored })
                                },
                            )))
                            .child(Button::new("hf-cancel", "Cancel").on_click(
                                cx.listener(|_, _, _, cx| cx.emit(HeaderFooterEvent::Dismiss)),
                            ))
                            .child(
                                Button::new("hf-go", if self.current { "Update" } else { "Add" })
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
    use sheaf_doc::tx::fixture::{header, set_header};
    use sheaf_doc::{Actor, PageId};
    use sheaf_image::raster::Raster;
    use sheaf_ops::views::Views;
    use sheaf_render::EnginePool;

    /// `bytes` opened as the file `name` in `dir`.
    fn opened(dir: &std::path::Path, name: &str, bytes: &[u8]) -> Document {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        sheaf_doc::open::file(&path, None).unwrap().doc
    }

    /// The Header & Footer dialog's preview is its page as applying the
    /// settings draws it (M10.5 §8.6): on a file sheaf saved with a header
    /// drawn into its pages, found again as it opened, an update's preview
    /// of the first page is that page with the header set again, which
    /// replaces its old blocks, pixel for pixel the page the apply leaves,
    /// not the page as it shows now; settings that can't be set say why;
    /// once the page is gone, the first page's preview is shown. A
    /// lowering's picture shows only while no later lowering has begun:
    /// settings that make the op lowered last aren't lowered again, an
    /// older lowering's picture is dropped, and a change of the document
    /// lowers the same op again.
    #[test]
    fn the_header_dialog_previews_the_header_and_drops_a_stale_result() {
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
        let g = now.state.header_footer().expect("the file's header");
        let mut hf = HeaderFooterSpec::clone(g.spec.header_footer().unwrap());
        hf.texts[0] = "NEW".into();
        let set = set_header(hf, None);
        let lower = || doc.lower_cx(Actor::User);
        let preview = draw_preview(&now, lower(), &previewer, set.clone(), 300).unwrap();
        let preview = block_on(preview).unwrap().unwrap().raster;

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
        let mut applied = Document::tx_at(&now, lower());
        applied.apply(set.clone(), &NoProgress).unwrap();
        assert!(
            preview.data == drawn(applied.state()),
            "as applying draws it"
        );
        assert!(preview.data != drawn(&now.state), "not as it shows now");
        let mut unfit = header("NEW");
        unfit.size = 0.0;
        let none = draw_preview(&now, lower(), &previewer, set_header(unfit, None), 300);
        assert!(none.is_err(), "settings that can't be set");
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

    /// The dialog starts from the document's header and footer as the model
    /// has it, and from what a file sheaf saved has after it's opened again,
    /// found again from its blocks: its six texts, face, size, colour,
    /// margins, first number, Bates numbering and pages, Bates numbering on
    /// for Bates Numbering… too; with none there, from its defaults, a Bates
    /// number at the footer's right for Bates Numbering….
    #[test]
    fn the_dialog_starts_from_the_model_after_a_save_and_reopen() {
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let blank = sheaf_doc::generated::blank_pages(612.0, 792.0, 3).unwrap();
        let mut doc = opened(dir.path(), "blank.pdf", &blank);
        let fresh = Start::at(doc.state(), false);
        assert!(!fresh.current && !fresh.bates && fresh.pages.is_empty());
        assert!(header::is_empty(&fresh.spec));
        assert_eq!(
            (fresh.spec.font, fresh.spec.size, fresh.spec.first_number),
            (FontChoice::Helvetica, 10.0, 1)
        );
        let numbering = Start::at(doc.state(), true);
        assert!(numbering.bates && !numbering.current);
        assert_eq!(numbering.spec.texts[5], "<<bates>>");

        let spec = HeaderFooterSpec {
            texts: [
                "<<label>>".into(),
                String::new(),
                "Page <<1>> of <<N>>".into(),
                String::new(),
                "Café".into(),
                "<<bates>>".into(),
            ],
            font: FontChoice::TimesBold,
            size: 9.0,
            color: [20, 60, 170],
            margins: Margins {
                top: 30.0,
                bottom: 40.0,
                left: 54.0,
                right: 54.0,
            },
            first_number: 3,
            bates: Some(BatesSpec {
                prefix: "ABC".into(),
                suffix: "-X".into(),
                digits: 5,
                start: 100,
            }),
        };
        let pages = [doc.page(1).unwrap(), doc.page(2).unwrap()];
        let set = set_header(spec.clone(), Some(&pages));
        doc.commit_ops(vec![set], "Header").unwrap();
        let unsaved = Start::at(doc.state(), false);
        assert!(unsaved.current && unsaved.bates);
        assert_eq!((&unsaved.spec, unsaved.pages.as_str()), (&spec, "2-3"));
        let saved = sheaf_doc::save::rewrite(&doc.snapshot()).unwrap();
        let again = opened(dir.path(), "saved.pdf", &saved);

        let reopened = Start::at(again.state(), false);
        assert!(reopened.current && reopened.bates);
        assert_eq!((&reopened.spec, reopened.pages.as_str()), (&spec, "2-3"));
        assert_eq!(font_at(reopened.spec.font), (1, 1));
        let numbering = Start::at(again.state(), true);
        assert_eq!(numbering.spec, spec, "a header there is kept");
    }
}
