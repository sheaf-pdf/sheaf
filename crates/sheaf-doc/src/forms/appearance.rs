//! A widget's appearance, sheaf's half (M10 slices 6 and 8): what one
//! widget's appearance is made from, as plain data out of the model, by
//! its kind ([`Look`], [`look`]), with what every kind shares read once
//! ([`Frame`]: the box, `/MK`, the border and the `/DA`); which widgets need
//! one made ([`stale`]), which a `/NeedAppearances` file left without one
//! ([`missing`]), and which show a state their file's doesn't
//! ([`state_changed`]); the font it's drawn in, read once a name from its
//! source's `/DR` (`read_font`, through
//! [`Source::field_font`](crate::Source::field_font), and [`font`]); the
//! name the appearance gives that font ([`font_res_id`], [`font_res`]); the
//! caps it's made under ([`limits`]); and the making ([`make`]), which is
//! pdf-forms': its `/DA` parser, encoders, layout, borders, `/MK /R` and
//! `/Tx BMC`, text in runs of the field's font and sheaf's fallback
//! font ([`Fallback`](crate::content::Fallback), M10 slice 7) for what the
//! field's can't draw, a check box's or radio button's mark in
//! ZapfDingbats, and a combo box's or list box's text and a push button's
//! captions in the field's font and the fallback. What's made is a form for
//! each slot of the widget's `/AP` ([`Made`], [`Slot`]).
//!
//! Each kind is a module of its own: a text field's widget (`text`,
//! [`TextLook`]), a check box's or radio button's (`toggle`,
//! [`ToggleLook`]), a combo box's or list box's (`choice`,
//! [`ChoiceLook`]) and a push button's (`push`, [`PushLook`]). What they
//! share is here.
//!
//! What's sheaf's and stays here: the model and its records, which widgets
//! are made, the caps, a source's font cache, the names a made form gives
//! a font, and which fallback it draws in. Nothing here writes: the
//! emitter binds what [`make`] gives, the fallback's glyphs with it, where
//! a mark's appearance is bound.

use std::fmt;
use std::sync::{Arc, LazyLock};

use super::field::{self, Border, BorderStyle, FieldKind};
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

mod choice;
mod push;
mod text;
mod toggle;

pub use choice::{ChoiceLook, ChoiceShows, selected_by_value};
pub use push::PushLook;
pub use text::TextLook;
pub use toggle::{ToggleLook, state_changed};

#[cfg(test)]
mod choice_tests;
#[cfg(test)]
mod push_tests;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod toggle_tests;

/// What one widget's appearance is made from, by its kind: owned plain data
/// out of the model, as [`look`] gives it, and what [`make`] makes it of.
///
/// `Debug` prints lengths and numbers, never a text.
#[derive(Debug, Clone, PartialEq)]
pub enum Look {
    /// A text field's widget.
    Text(TextLook),
    /// A check box's or radio button's widget.
    Toggle(ToggleLook),
    /// A combo box's or list box's widget.
    Choice(ChoiceLook),
    /// A push button's widget.
    Push(PushLook),
}

impl Look {
    /// What's drawn around it, and its effective `/DA`.
    pub fn frame(&self) -> &Frame {
        match self {
            Look::Text(look) => &look.frame,
            Look::Toggle(look) => &look.frame,
            Look::Choice(look) => &look.frame,
            Look::Push(look) => &look.frame,
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
/// field's widget's ([`TextLook`]), a check box's or radio button's
/// ([`ToggleLook`]), a combo box's or list box's ([`ChoiceLook`]), a push
/// button's ([`PushLook`]). Every kind's frame is the widget's box, `/MK`
/// and border, and its effective `/DA`: the widget's own, else the
/// terminal's (its own or a parent's, as read), else its instance's form's
/// (`form_defaults`). Why none is read is sheaf's to say ([`NoLook`]):
/// [`NoLook::NotWidget`] for any other annotation and a widget whose field
/// isn't a terminal, [`NoLook::Kind`] for a field of a kind no look is
/// read for (a signature's), and pdf-forms' own refusal of what was read,
/// [`Unmade::Icon`] for a push button's that shows an icon, as
/// [`NoLook::Unmade`]. It doesn't ask [`stale`]: a look is read for any
/// widget.
pub fn look(st: &DocState, aid: AnnotId) -> Result<Look, NoLook> {
    let found = (st.annot(aid).and_then(|rec| rec.widget())).and_then(|w| {
        let node = st.field(w.field)?;
        Some((w, node, node.term.as_deref()?))
    });
    let Some((w, node, term)) = found else {
        return Err(NoLook::NotWidget);
    };
    match term.kind {
        FieldKind::Text => Ok(Look::Text(text::look(st, w, node, term))),
        FieldKind::CheckBox | FieldKind::Radio => Ok(Look::Toggle(toggle::look(st, w, node, term))),
        FieldKind::Combo | FieldKind::List => Ok(Look::Choice(choice::look(st, w, node, term))),
        FieldKind::PushButton => match push::look(st, w, node) {
            Ok(look) => Ok(Look::Push(look)),
            Err(why) => Err(NoLook::Unmade(why)),
        },
        kind @ (FieldKind::Signature | FieldKind::Unknown) => Err(NoLook::Kind(kind)),
    }
}

/// Why [`look`] reads no look for an annotation. The first two are sheaf's
/// own reasons, kept apart from pdf-forms' refusals ([`Unmade`]), whose
/// kind checks (`NotText`, `NotButton`, `NotChoice`) are its own read
/// layer's and mean what that crate says of a field it read.
#[derive(Debug, Clone, PartialEq)]
pub enum NoLook {
    /// It isn't a widget, or its field isn't a terminal.
    NotWidget,
    /// Its field is of a kind no look is read for: a signature's, or of no
    /// kind sheaf knows.
    Kind(FieldKind),
    /// pdf-forms' refusal of what was read: [`Unmade::Icon`], a push
    /// button's that shows an icon, which isn't drawn.
    Unmade(Unmade),
}

impl NoLook {
    /// Which kind of reason it is, as a warning or a dump says it:
    /// `not-widget`, `no-look:` and the field's kind
    /// ([`FieldKind::short`]), or pdf-forms' by [`unmade_kind`].
    pub fn kind(&self) -> String {
        match self {
            NoLook::NotWidget => "not-widget".into(),
            NoLook::Kind(kind) => format!("no-look:{}", kind.short()),
            NoLook::Unmade(why) => unmade_kind(why),
        }
    }
}

/// Widget `w`'s frame, `node` its field: its `/Rect`'s size, its `/MK`
/// colors and rotation, its border (width 1, solid, where it has none), and
/// its effective `/DA` ([`effective_da`]).
fn frame(st: &DocState, w: &Widget, node: &FieldNode) -> Frame {
    let da = effective_da(st, w, node);
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

/// Widget `w`'s effective `/DA`, `node` its field: its own, else its
/// terminal's (its own or a parent's, as read), else its instance's form's
/// ([`form_defaults`]); none where none of them has one.
fn effective_da<'a>(st: &'a DocState, w: &'a Widget, node: &'a FieldNode) -> Option<&'a str> {
    let term = node.term.as_deref();
    let (form_da, _) = form_defaults(st, w, node);
    (w.da.as_deref())
        .or(term.and_then(|t| t.props.da.as_deref()))
        .or(form_da)
}

/// Widget `w`'s source widget, as its source's form was read: none for a
/// widget read from no source, or not among its source's widgets as read.
fn read_widget(w: &Widget) -> Option<&crate::forms::Widget> {
    let r = w.src.as_ref()?;
    r.src.form().widget_by_object(r.id)
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

/// Where in a widget's `/AP` a made form goes. A check box's or radio
/// button's go in state dictionaries: its on state's under the name it
/// goes under ([`ToggleLook::on_state`]), `Off`'s under `Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Slot {
    /// `/N`, the normal appearance: a text field's, a choice's and a push
    /// button's one form; a check box's or radio button's on state's.
    Normal,
    /// A check box's or radio button's normal appearance of `Off`, in
    /// `/N`.
    NormalOff,
    /// `/D`, the down appearance: a check box's or radio button's on
    /// state's; a push button's one form.
    Down,
    /// A check box's or radio button's down appearance of `Off`, in `/D`.
    DownOff,
    /// `/R`, a push button's rollover appearance.
    Rollover,
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
pub struct DaFont {
    pub font: Arc<FieldFont>,
    pub res: Res,
}

/// `look`'s appearance, a form for each slot of its widget's `/AP`, under
/// [`limits`]: a text field's widget's, its `/N` alone (`text`), in the
/// font its `/DA` names and the fallback font ([`Fallback::SANS`]) for
/// what that can't draw; a check box's or radio button's, its on state's
/// and `Off`'s normal and down forms (`toggle`), the mark in ZapfDingbats;
/// a combo box's or list box's, its `/N` alone (`choice`), in the two fonts
/// as a text field's; a push button's, its `/N`, and its `/R` and `/D`
/// where its `/H` calls for them (`push`), its captions in the two fonts.
/// `font` gives the `/DA`'s font for the name the `/DA` gives (none where
/// it names none), and is asked only by a kind that draws in it: a check
/// box's or radio button's mark isn't, so its `/DA`'s font is never read,
/// and can't refuse it. Pure: the same look, font and name give the same
/// bytes and glyphs. It doesn't ask [`stale`]: whatever reads a look for a
/// widget nothing changed makes one as for any other.
///
/// [`Fallback::SANS`]: crate::content::Fallback::SANS
pub fn make(
    look: &Look,
    font: impl FnOnce(Option<&[u8]>) -> Result<DaFont, Unmade>,
) -> Result<Made, Unmade> {
    match look {
        Look::Text(look) => text::make(look, font(look.frame.font_name().as_deref())?),
        Look::Toggle(look) => toggle::make(look),
        Look::Choice(look) => choice::make(look, font(look.frame.font_name().as_deref())?),
        Look::Push(look) => push::make(look, font(look.frame.font_name().as_deref())?),
    }
}

/// `look`'s appearance as the emitter makes it for a widget read from `src`
/// ([`make`]): in the font its `/DA` names there ([`font`]), named for copy
/// 0 of `src` ([`font_res`]), a font named by an entry bound to no object,
/// which changes nothing of what's drawn. What lowering makes to learn
/// whether a field's widget can show what it now holds.
pub fn make_in(look: &Look, src: &Source) -> Result<Made, Unmade> {
    make(look, |name| {
        let font = font(src, name)?;
        let written = |at: FontAt<'_>| match at {
            FontAt::Object(id) => id,
            FontAt::Entry(_) => ObjectId::new(0, 0),
        };
        let res = font_res(&font, src.id(), 0, name.unwrap_or_default(), written);
        Ok(DaFont { font, res })
    })
}

/// Whether widget `aid` needs its appearance made, by its kind:
///
/// - a text field's widget whose terminal has a display (in no file), or
///   whose text isn't what its source's field held as read (no value and
///   an empty one are the same), or whose field was read from no source,
///   or isn't among its source's fields as read: its value and display are
///   all an op can change that makes the file's appearance wrong;
/// - a check box's or radio button's widget that shows a state its source
///   widget didn't as read ([`state_changed`]) and whose file's normal
///   appearance has no on state (none, a stream, or `Off` alone): one
///   that has keeps its appearances, and only its `/AS` moves;
/// - a combo box's or list box's widget whose terminal has a display, or
///   whose value holds other texts than its source's field's as read (no
///   value and an empty array alike, a text and an array of it alike), or
///   whose field was read from no source, or isn't among its source's
///   fields as read;
/// - a push button's widget whose captions (`/MK /CA`, `/RC`, `/AC`) aren't
///   its source widget's as read (one not read had none): one nothing
///   changed keeps its appearances, and one with none stays so.
///
/// False for anything else. A map lookup or two and a compare of two
/// strings, or of a list box's selected texts, or of three captions.
pub fn stale(st: &DocState, aid: AnnotId) -> bool {
    let Some(w) = st.annot(aid).and_then(|rec| rec.widget()) else {
        return false;
    };
    let Some(node) = st.field(w.field) else {
        return false;
    };
    let Some(term) = node.term.as_deref() else {
        return false;
    };
    match term.kind {
        FieldKind::Text => text::stale(node, term),
        FieldKind::CheckBox | FieldKind::Radio => toggle::stale(st, aid, w),
        FieldKind::Combo | FieldKind::List => choice::stale(node, term),
        FieldKind::PushButton => push::stale(w),
        _ => false,
    }
}

/// Whether widget `aid` is one save policy makes an appearance for where
/// its file's form says `/NeedAppearances` and left it to the viewer
/// (`SaveTransform::FillAppearances`, M10 slice 9): its file gave it no
/// normal appearance (none, or a `/N` of no states), and a viewer that
/// ignores the flag, as PDFium's screen does, would show nothing of what
/// it holds:
///
/// - a text field's widget whose field holds a text that isn't empty;
/// - a combo box's or list box's whose field's value holds one;
/// - a check box's or radio button's, whatever state it shows;
/// - a push button's with a caption (`/MK /CA`).
///
/// False for anything else, and for a widget whose file has an appearance,
/// which may be the stale one the flag was there to cover: it's made only
/// where [`stale`] says. What's made is as for a stale widget, and a widget
/// both is made once.
pub fn missing(st: &DocState, aid: AnnotId) -> bool {
    let Some(w) = st.annot(aid).and_then(|rec| rec.widget()) else {
        return false;
    };
    let Some(term) = st.field(w.field).and_then(|node| node.term.as_deref()) else {
        return false;
    };
    if w.props.has_appearance {
        return false;
    }
    let value = term.value.as_ref();
    match term.kind {
        FieldKind::Text => !field::text_shown(value).is_empty(),
        FieldKind::Combo | FieldKind::List => {
            field::choice_texts(value).iter().any(|t| !t.is_empty())
        }
        FieldKind::CheckBox | FieldKind::Radio => true,
        FieldKind::PushButton => w.mk.caption.as_deref().is_some_and(|c| !c.is_empty()),
        FieldKind::Signature | FieldKind::Unknown => false,
    }
}

/// The caps a widget's appearance and its font are made and read under:
/// pdf-forms' defaults, with each sheaf has a cap for from sheaf's
/// ([`crate::source::Limits::DEFAULT`]): references followed as deep as
/// sheaf's reader follows them; a widget's caps as sheaf reads an
/// annotation (`source::annots::limits`), among them the one a
/// `/DA` is parsed under; the text as long as a value sheaf reads
/// (`FormLimits::text`), a list box's options together too; a list box's
/// options shown from its top as many as sheaf reads of an `/Opt`
/// (`FormLimits::items`); a comb field's cells as many as sheaf lays out
/// ([`MAX_LAYOUT_LEN`](super::read::MAX_LAYOUT_LEN)); a font read under
/// sheaf's font caps ([`Limits::fonts`](crate::source::Limits::fonts)),
/// its streams decompressed within what a page's content and its fonts
/// may decompress to. The tree's caps (`fields`, `depth`) are the family's
/// read layer's, which only tests call.
pub fn limits() -> Limits {
    let ours = crate::source::Limits::DEFAULT;
    let mut limits = Limits::DEFAULT;
    limits.hops = ours.reader.max_depth;
    limits.annots = crate::source::annots::limits();
    limits.text = ours.forms.text;
    limits.items = ours.forms.items;
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
        Unmade::NotButton => "not-button".into(),
        Unmade::Icon => "icon".into(),
        Unmade::NotChoice => "not-choice".into(),
        Unmade::UnencodableOption { .. } => "unencodable-option".into(),
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
