//! A widget's appearance, sheaf's half (M10 slices 6 and 8): what one
//! widget's appearance is made from, as plain data out of the model, by
//! its kind ([`Look`], [`look`]), with what every kind shares read once
//! ([`Frame`]: the box, `/MK`, the border and the `/DA`); which widgets need
//! one made ([`stale`]); the font it's drawn in, read once a name from its
//! source's `/DR` (`read_font`, through
//! [`Source::field_font`](crate::Source::field_font), and [`font`]); the
//! name the appearance gives that font ([`font_res_id`], [`font_res`]); the
//! caps it's made under ([`limits`]); and the making ([`make`]), which is
//! pdf-forms': its `/DA` parser, encoders, layout, borders, `/MK /R` and
//! `/Tx BMC`, and text in runs of the field's font and sheaf's fallback
//! font ([`Fallback`](crate::content::Fallback), M10 slice 7) for what the
//! field's can't draw. What's made is a form for each slot of the
//! widget's `/AP` ([`Made`], [`Slot`]).
//!
//! Each kind is a module of its own: a text field's widget (`text`,
//! [`TextLook`]). What they share is here.
//!
//! What's sheaf's and stays here: the model and its records, which widgets
//! are made, the caps, a source's font cache, the names a made form gives
//! a font, and which fallback it draws in. Nothing here writes: the
//! emitter binds what [`make`] gives, the fallback's glyphs with it, where
//! a mark's appearance is bound.

use std::fmt;
use std::sync::{Arc, LazyLock};

use super::field::{self, Border, BorderStyle, FieldKind, Value};
use crate::annot::AnnotId;
use crate::content::{Glyphs, Objects, Unkept};
use crate::model::{DocState, FieldNode, Widget};
use crate::pdf::annots::Cap as AnnotsCap;
use crate::pdf::content::{Color, Res, ResBody, ResId};
use crate::pdf::fonts::Std14;
use crate::pdf::forms::{Appearance, Cap, Error, FieldFont, Limits, Quadding, Unmade, WidgetLook};
use crate::pdf::gfx::{BoxBorder, BoxBorderStyle, DefaultAppearance};
use crate::pdf::{ObjectId, Why};
use crate::source::{FieldFontRead, Source};
use crate::store::BlobId;

mod text;

pub use text::TextLook;

#[cfg(test)]
pub(crate) mod tests;

/// What one widget's appearance is made from, by its kind: owned plain data
/// out of the model, as [`look`] gives it, and what [`make`] makes it of.
///
/// `Debug` prints lengths and numbers, never a text.
#[derive(Debug, Clone, PartialEq)]
pub enum Look {
    /// A text field's widget.
    Text(TextLook),
}

impl Look {
    /// What's drawn around it, and its effective `/DA`.
    pub fn frame(&self) -> &Frame {
        match self {
            Look::Text(look) => &look.frame,
        }
    }
}

/// What every kind of widget's appearance is drawn around and in, read
/// once from the model (`frame`, as [`look`] reads a look): the widget's
/// box, its `/MK` colors and rotation, its border, and its effective
/// `/DA`.
///
/// `Debug` prints numbers and the `/DA`'s length, never its bytes.
#[derive(Clone, PartialEq)]
pub struct Frame {
    /// The widget's box: its `/Rect`'s width and height.
    pub width: f32,
    pub height: f32,
    /// `/MK /BG` and `/BC`; none for no fill and no border.
    pub background: Option<field::Color>,
    pub border_color: Option<field::Color>,
    /// `/MK /R`: 0, 90, 180 or 270.
    pub rotation: u16,
    /// Its border, its width, style and dash array: width 1, solid, where
    /// it has none, as a widget with neither `/BS` nor `/Border` draws.
    pub border: Border,
    /// The effective `/DA`, its bytes: the widget's own, else its field's
    /// (its own or a parent's), else its form's; empty for none.
    pub da: Vec<u8>,
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Frame {
            width,
            height,
            background,
            border_color,
            rotation,
            border,
            da,
        } = self;
        f.debug_struct("Frame")
            .field("width", width)
            .field("height", height)
            .field("background", background)
            .field("border_color", border_color)
            .field("rotation", rotation)
            .field("border", border)
            .field("da_len", &da.len())
            .finish()
    }
}

impl Frame {
    /// The font name its `/DA` gives (`Tf`), parsed as [`make`] parses it:
    /// none where it names none, and the text is drawn in Helvetica.
    pub fn font_name(&self) -> Option<Vec<u8>> {
        self.da(&limits()).font_name
    }

    /// Its `/DA` parsed as pdf-forms parses one under `limits`: none, or
    /// one that doesn't parse, is the empty one, which draws in Helvetica
    /// at an automatic size.
    fn da(&self, limits: &Limits) -> DefaultAppearance {
        match self.da.is_empty() {
            true => DefaultAppearance::default(),
            false => DefaultAppearance::parse(&self.da, &limits.annots.content).unwrap_or_default(),
        }
    }

    /// The widget's box and what's drawn around it, as pdf-forms' makers
    /// take them.
    fn widget(&self) -> WidgetLook<'_> {
        let mut widget: WidgetLook<'_> = WidgetLook::new(self.width, self.height);
        widget.background = self.background.map(color);
        widget.border_color = self.border_color.map(color);
        widget.rotation = i32::from(self.rotation);
        let Border { width, style, dash } = &self.border;
        widget.border = BoxBorder::new(*width, box_style(*style)).with_dash(dash);
        widget
    }
}

/// What widget `aid`'s appearance is made from, by its kind: a text
/// field's widget's ([`TextLook`]). Every kind's frame is the widget's box,
/// `/MK` and border, and its effective `/DA`: the widget's own, else the
/// terminal's (its own or a parent's, as read), else its instance's form's
/// (`form_defaults`). [`Unmade::NotText`] for any other annotation, a
/// widget whose field isn't a terminal, and a field of a kind no look is
/// read for. It doesn't ask [`stale`]: a look is read for any widget.
pub fn look(st: &DocState, aid: AnnotId) -> Result<Look, Unmade> {
    let found = (st.annot(aid).and_then(|rec| rec.widget())).and_then(|w| {
        let node = st.field(w.field)?;
        Some((w, node, node.term.as_deref()?))
    });
    let Some((w, node, term)) = found else {
        return Err(Unmade::NotText);
    };
    match term.kind {
        FieldKind::Text => Ok(Look::Text(text::look(st, w, node, term))),
        _ => Err(Unmade::NotText),
    }
}

/// Widget `w`'s frame, `node` its field: its `/Rect`'s size, its `/MK`
/// colors and rotation, its border (width 1, solid, where it has none), and
/// the effective `/DA`, the widget's own, else its terminal's, else its
/// instance's form's ([`form_defaults`]).
fn frame(st: &DocState, w: &Widget, node: &FieldNode) -> Frame {
    let term = node.term.as_deref();
    let (form_da, _) = form_defaults(st, w, node);
    let da = (w.da.as_deref())
        .or(term.and_then(|t| t.props.da.as_deref()))
        .or(form_da);
    Frame {
        width: w.rect[2] - w.rect[0],
        height: w.rect[3] - w.rect[1],
        background: w.mk.background,
        border_color: w.mk.border_color,
        rotation: w.mk.rotation,
        border: (w.props.border.clone()).unwrap_or(Border::solid(1.0)),
        // A `/DA` is held a character a byte (Latin-1), so its bytes come
        // back whole.
        da: da.map_or_else(Vec::new, |da| da.chars().map(|c| c as u8).collect()),
    }
}

/// The `/DA` and `/Q` widget `w`'s instance's form gives the fields that
/// set neither: the document's ([`FormDoc`]) for an instance of the file
/// it opened, else the instance's source's as read; the document's where
/// the widget names no source. As the emitter writes each instance's form
/// (`Fields::defaults`).
///
/// [`FormDoc`]: crate::model::FormDoc
fn form_defaults<'a>(
    st: &'a DocState,
    w: &'a Widget,
    node: &'a FieldNode,
) -> (Option<&'a str>, u8) {
    let doc = st.form();
    let inst = w.inst.src;
    let src = [&w.src, &node.src]
        .into_iter()
        .flatten()
        .find(|r| r.src.id() == inst);
    match src {
        Some(r) if Some(inst) != st.opened_file() => {
            let facts = &r.src.form().facts;
            (facts.da.as_deref(), facts.align)
        }
        _ => (doc.da.as_deref(), doc.align),
    }
}

/// Where in a widget's `/AP` a made form goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Slot {
    /// `/N`, the normal appearance: a text field's one form.
    Normal,
}

/// What [`make`] makes of a look: each form, by the slot of the widget's
/// `/AP` it goes in, and the glyphs they draw in the fallback font
/// ([`Fallback::SANS`](crate::content::Fallback::SANS)), which whatever
/// writes a form that names the fallback joins where it binds it
/// ([`sink::form`](crate::content::sink::form)): none where the field's font
/// draws all of the text.
///
/// `Debug` prints each form's slot and parts, and how many glyphs, never
/// a glyph or its text.
#[derive(Clone, PartialEq)]
pub struct Made {
    pub forms: Vec<(Slot, Appearance)>,
    pub glyphs: Glyphs,
}

impl fmt::Debug for Made {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Made")
            .field("forms", &self.forms)
            .field("glyphs", &self.glyphs.len())
            .finish()
    }
}

impl Made {
    /// The form made for `slot`, where one was.
    pub fn form(&self, slot: Slot) -> Option<&Appearance> {
        (self.forms.iter())
            .find(|(s, _)| *s == slot)
            .map(|(_, form)| form)
    }
}

/// The font a look's text is drawn in: the one its `/DA` names, as its
/// widget's source reads it ([`font`]), and the resource its forms name
/// that font by ([`font_res`]). What it can't draw is drawn in the
/// fallback.
pub struct DaFont<'a> {
    pub font: &'a FieldFont,
    pub res: Res,
}

/// `look`'s appearance, a form for each slot of its widget's `/AP`, in
/// `font` and the fallback font ([`Fallback::SANS`]) for what that can't
/// draw, under [`limits`]: a text field's widget's, its `/N` alone
/// (`text`). Pure: the same look, font and name give the same bytes and
/// glyphs. It doesn't ask [`stale`]: whatever reads a look for a widget
/// nothing changed makes one as for any other.
///
/// [`Fallback::SANS`]: crate::content::Fallback::SANS
pub fn make(look: &Look, font: DaFont<'_>) -> Result<Made, Unmade> {
    match look {
        Look::Text(look) => text::make(look, font),
    }
}

/// The text a text field's value shows, borrowed: none and an empty one
/// alike.
fn shows(value: Option<&Value>) -> &str {
    match value {
        Some(Value::Text(text)) => text,
        _ => "",
    }
}

/// Whether widget `aid` needs its appearance made: it's a text field's
/// widget whose terminal's text isn't what its source's field held as read
/// (no value and an empty one are the same), or whose field was read from
/// no source, or isn't among its source's fields as read. Until an op can
/// change anything else of a widget, its value is all that can make the
/// file's appearance wrong. False for anything else, a check box among
/// them. A map lookup and a compare of two strings.
pub fn stale(st: &DocState, aid: AnnotId) -> bool {
    let Some(w) = st.annot(aid).and_then(|rec| rec.widget()) else {
        return false;
    };
    let Some(node) = st.field(w.field) else {
        return false;
    };
    let Some(term) = (node.term.as_deref()).filter(|t| t.kind == FieldKind::Text) else {
        return false;
    };
    let Some(src) = &node.src else {
        return true;
    };
    match src.src.form().field_by_object(src.id) {
        Some(read) => shows(read.value.as_ref()) != shows(term.value.as_ref()),
        None => true,
    }
}

/// The caps a text field's appearance and its font are made and read
/// under: pdf-forms' defaults, with each sheaf has a cap for from sheaf's
/// ([`crate::source::Limits::DEFAULT`]): references followed as deep as
/// sheaf's reader follows them; a widget's caps as sheaf reads an
/// annotation (`source::annots::limits`), among them the one a
/// `/DA` is parsed under; the text as long as a value sheaf reads
/// (`FormLimits::text`); a comb field's cells as many as sheaf lays out
/// ([`MAX_LAYOUT_LEN`](super::read::MAX_LAYOUT_LEN)); a font read under
/// sheaf's font caps ([`Limits::fonts`](crate::source::Limits::fonts)),
/// its streams decompressed within what a page's content and its fonts
/// may decompress to. The tree's caps (`fields`, `depth`, `items`) are
/// the family's read layer's, which only tests call.
pub fn limits() -> Limits {
    let ours = crate::source::Limits::DEFAULT;
    let mut limits = Limits::DEFAULT;
    limits.hops = ours.reader.max_depth;
    limits.annots = crate::source::annots::limits();
    limits.text = ours.forms.text;
    limits.comb_cells = super::read::MAX_LAYOUT_LEN as usize;
    limits.fonts = ours.fonts();
    limits.font_budget = ours.content.decompressed;
    limits
}

/// The font a field whose `/DA` names `name` is drawn in, read from `src`:
/// its form's `/DR /Font` entry of that name, found from the catalog and
/// read by pdf-forms (`FieldFont::find_in_catalog`: the walk to
/// `/AcroForm /DR /Font` is the family's, as its own reading of a form
/// walks it) under [`limits`]; the standard 14 font the name falls back to
/// where `/DR` has none, or the source no catalog. An error where what it
/// reads is locked, past a reader's cap, or the font past its budget.
pub(crate) fn read_font(src: &Source, name: &[u8]) -> Result<FieldFont, Error> {
    let limits = limits();
    let unkept = Unkept::default();
    let objects = Objects::new(src.pdf(), &unkept);
    match src.pdf().catalog() {
        Ok(catalog) => FieldFont::find_in_catalog(objects, catalog, name, &limits, &mut ()),
        Err(_) => FieldFont::find(objects, None, name, &limits, &mut ()),
    }
}

/// The font a widget whose `/DA` names `name` ([`Frame::font_name`]) is
/// drawn in, from `src`, its widget's source: the one that names
/// ([`Source::field_font`]), or Helvetica where it names none, as PDFium
/// draws such a field.
pub fn font(src: &Source, name: Option<&[u8]>) -> FieldFontRead {
    static HELVETICA: LazyLock<Arc<FieldFont>> =
        LazyLock::new(|| Arc::new(FieldFont::standard(Std14::Helvetica)));
    match name {
        Some(name) => src.field_font(name),
        None => Ok(HELVETICA.clone()),
    }
}

/// Where a font a made appearance names is in its source, where the
/// appearance names it by an object: a font `/DR` holds by reference, by
/// that object; or a direct entry of `/DR /Font` that holds references,
/// which a writer writes as an object of its own (the dictionary the font
/// lends, `FieldFont::direct`), by its name there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontAt<'a> {
    Object(ObjectId),
    Entry(&'a [u8]),
}

/// The name a made appearance gives a `/DR` font of copy `copy` of the
/// source `blob` that it names by an object ([`FontAt`]): a digest of the
/// three, so the content naming it doesn't depend on the number the font
/// is written under, and an overlay's form and a save's are the same
/// bytes, run after run (a blob's id is its bytes' checksum). What the
/// emitter, the fields dump and the appearance golden all name such a font
/// by.
pub fn font_res_id(blob: BlobId, copy: u32, at: FontAt<'_>) -> ResId {
    let mut sum = sheaf_util::Checksum::new();
    sum.write(b"sheaf field font\0");
    sum.write(&blob.hash.to_le_bytes());
    sum.write(&blob.len.to_le_bytes());
    sum.write(&copy.to_le_bytes());
    match at {
        FontAt::Object(id) => {
            sum.write(&[0]);
            sum.write(&id.number.to_le_bytes());
            sum.write(&id.generation.to_le_bytes());
        }
        FontAt::Entry(name) => {
            sum.write(&[1]);
            sum.write(&(name.len() as u64).to_le_bytes());
            sum.write(name);
        }
    }
    ResId(sum.finish())
}

/// The resource a made appearance names `font` by, `font` the one `name`
/// names in the `/DR` of copy `copy` of source `blob`: the font's own
/// where it's a dictionary that reaches nothing else (a fallback, or a
/// direct entry with no reference), else named by [`font_res_id`] and
/// bound to the object `written` gives for where it is (the emitter's
/// placeholder for it).
pub fn font_res(
    font: &FieldFont,
    blob: BlobId,
    copy: u32,
    name: &[u8],
    written: impl FnOnce(FontAt<'_>) -> ObjectId,
) -> Res {
    let at = match font.res() {
        None => FontAt::Entry(name),
        Some(res) => match res.body() {
            &ResBody::Written(id) => FontAt::Object(id),
            _ => return res,
        },
    };
    let body = ResBody::Written(written(at));
    Res::named(font_res_id(blob, copy, at), body).expect("an object written is a resource")
}

/// Which kind of reason `why` is that an appearance wasn't made, as a
/// warning or a dump says it: never a character of the text. Each kind is
/// named here, by hand, so no text of the PDF layer's types (their `Debug`
/// output, which isn't their API) reaches a warning, a dump or a count; a
/// kind the layer adds later reads `other` until it's named.
pub fn unmade_kind(why: &Unmade) -> String {
    match why {
        Unmade::Unencodable(_) => "unencodable".into(),
        Unmade::NoBox => "no-box".into(),
        Unmade::NotFinite => "not-finite".into(),
        Unmade::Limit(cap) => format!("limit:{}", cap_kind(*cap)),
        Unmade::NotText => "not-text".into(),
        Unmade::FontNotBindable => "font-not-bindable".into(),
        Unmade::Unreadable(u) => format!("unreadable:{}", why_kind(u.why)),
        _ => "other".into(),
    }
}

/// Which kind of reason `e` is that a form or a font couldn't be read, as
/// [`unmade_kind`] says it: what the fields dump and the corpus tier say
/// of a font's error.
#[cfg(any(test, feature = "fixtures"))]
pub fn error_kind(e: &Error) -> String {
    match e {
        Error::Limit(cap) => format!("limit:{}", cap_kind(*cap)),
        Error::Unreadable(u) => format!("unreadable:{}", why_kind(u.why)),
        _ => "other".into(),
    }
}

/// A cap of pdf-forms', by name; one of pdf-annots' it reads a widget
/// under, as `annots:` and that cap's name.
fn cap_kind(cap: Cap) -> String {
    match cap {
        Cap::Fields => "fields".into(),
        Cap::Items => "items".into(),
        Cap::Annots(cap) => format!("annots:{}", annots_cap_kind(cap)),
        Cap::Text => "text".into(),
        Cap::CombCells => "comb-cells".into(),
        Cap::FontBudget => "font-budget".into(),
        _ => "other".into(),
    }
}

/// A cap of pdf-annots', by name.
fn annots_cap_kind(cap: AnnotsCap) -> &'static str {
    match cap {
        AnnotsCap::AnnotsPerPage => "annots-per-page",
        AnnotsCap::ActionChain => "action-chain",
        AnnotsCap::Quads => "quads",
        AnnotsCap::Points => "points",
        AnnotsCap::DestTree => "dest-tree",
        _ => "other",
    }
}

/// Why an object couldn't be read, by name.
fn why_kind(why: Why) -> &'static str {
    match why {
        Why::Damaged => "damaged",
        Why::Limit => "limit",
        Why::Capped => "capped",
        Why::Locked => "locked",
        Why::Source => "source",
        Why::Filter => "filter",
        Why::Detached => "detached",
        _ => "other",
    }
}

/// sheaf's border style as pdf-gfx's.
pub(crate) fn box_style(style: BorderStyle) -> BoxBorderStyle {
    match style {
        BorderStyle::Solid => BoxBorderStyle::Solid,
        BorderStyle::Dashed => BoxBorderStyle::Dashed,
        BorderStyle::Beveled => BoxBorderStyle::Beveled,
        BorderStyle::Inset => BoxBorderStyle::Inset,
        BorderStyle::Underline => BoxBorderStyle::Underline,
    }
}

/// sheaf's `/Q`, 0 to 2, as pdf-forms' justification.
pub(crate) fn quadding(q: u8) -> Quadding {
    match q {
        1 => Quadding::Center,
        2 => Quadding::Right,
        _ => Quadding::Left,
    }
}

/// An `/MK` color of sheaf's as content's.
pub(crate) fn color(c: field::Color) -> Color {
    match c {
        field::Color::Gray(g) => Color::Gray(g),
        field::Color::Rgb(c) => Color::Rgb(c),
        field::Color::Cmyk(c) => Color::Cmyk(c),
    }
}
