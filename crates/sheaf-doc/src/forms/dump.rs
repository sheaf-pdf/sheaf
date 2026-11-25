//! Every text widget of the corpus's forms, as sheaf reads it and as the
//! family's read layer (`pdf-forms`) reads it, set beside each other, and
//! the appearance each makes of it for a sample value ([`fields_dump`]): a
//! line a widget, numbers, names and checksums only. A value is never
//! printed, only its length, and the sample's. It asserts nothing: it is
//! for setting a change to what reads a form, or makes a text field's
//! appearance, beside what was there (M10 slice 6).
//!
//! **sheaf's reading** is the plain data its model holds for the widget
//! ([`TextLook`], as [`text_look`] reads it from the file opened as a
//! document): its text, flags, `/MaxLen` and justification (its own, else
//! its field's, else the form's); its box, `/MK` colors and rotation, and
//! its border and dashes (none is the width of 1, solid, that a widget
//! with no `/BS` and no `/Border` has); and its effective `/DA` (its own,
//! as a kid, else its field's, else the form's) parsed as the family
//! parses one. **The family's** is [`forms::widget_text`]'s, under
//! PDFium's rules. Where the two are equal the line says `=`; else which
//! parts differ, and the family's value of each, its field's object among
//! them where it reads the widget as another field's. A widget the family
//! reads as no text field's, or not at all, is said so, and a file's line
//! counts the text widgets on no page that only the family reads (sheaf's
//! are its fields' `outside`).
//!
//! **The font** is the one the family's `/DA` names, found in the form's
//! `/DR /Font` ([`FieldFont::find`]): by reference (`ref`), a direct
//! dictionary (`direct`), one that holds a reference (`direct-refs`), or
//! missing and the face it falls back to (`missing:`); none where the
//! `/DA` names no font (Helvetica), and `error:` where it can't be read.
//! With its kind, its program, its `/BaseFont` and its `/Encoding` (a
//! name, or a dictionary's base and how many names its `/Differences`
//! gives).
//!
//! **The appearances,** each of the sample value ([`sample`]) of sheaf's
//! text, in the field's font and, for what that can't draw, the fallback
//! font sheaf bundles ([`Fallback`], M10 slice 7), named by its marker as
//! the emitter names it: the family's of its own reading (`made`,
//! [`forms::text_with_fallback`]), and sheaf's of its model's (`ours`,
//! [`appearance::make`]) in the font its source reads for the `/DA`'s name
//! ([`appearance::font`]), named as the emitter names it
//! ([`appearance::font_res`]). Each is its size, whether it overflowed,
//! its content's length and checksum, and what it draws with, by their
//! ids; or which [`Unmade`] it is. Where sheaf's draws in the fallback,
//! `fallback` says how many of its glyphs; a line that draws nothing in it
//! has no such field. `ours.vs` sets sheaf's, its font named as the family
//! names it, beside the family's: `=`, or the parts that differ.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use in_progress::NoProgress;

use crate::annot::AnnotId;
use crate::content::corpus::{each_path, written};
use crate::content::{Fallback, Objects, Unkept};
use crate::forms::FormClass;
use crate::forms::appearance::{self, FontAt, TextLook, text_look};
use crate::forms::corpus::Corpus;
use crate::forms::field::{self, Field, FieldKind};
use crate::forms::fixture::sample;
use crate::history::FixedCount;
use crate::model::Document;
use crate::pdf::content::{Color, Res, ResBody};
use crate::pdf::fonts::{Descendant, FontKind, FontProgram};
use crate::pdf::forms::{
    self, Appearance, Error, FieldFont, Limits, MadeWithFallback, Quadding, Unmade,
};
use crate::pdf::gfx::{BoxBorderStyle, DefaultAppearance};
use crate::pdf::{Dictionary, LazyPdf, Object, ObjectId, Resolve, Value as PdfValue};
use crate::source::Source;
use crate::store::{BlobKind, Src, Store};

/// Where [`fields_dump`] writes: a path. Unset, it reads nothing.
/// `just fields-dump <file>` sets it.
const DUMP: &str = "SHEAF_FIELDS_DUMP";

/// A corpus path whose records [`fields_dump`] prints in full to stderr,
/// reading that file alone: to set two runs beside each other.
const DUMP_ONLY: &str = "SHEAF_FIELDS_DUMP_ONLY";

/// The family's caps, as the dump reads with them: its own defaults, so
/// its column is the family's reading alone.
const LIMITS: Limits = Limits::DEFAULT;

/// One text widget's appearance as plain data: what the field shows and
/// how, the widget's box and what's drawn around it, and its effective
/// `/DA` parsed. Either reading gives one.
#[derive(Debug, Clone, PartialEq)]
struct Plain {
    text: String,
    flags: u32,
    max_len: Option<u32>,
    quadding: Quadding,
    width: f32,
    height: f32,
    background: Option<Color>,
    border_color: Option<Color>,
    rotation: i32,
    /// The border's width, style and dash array.
    border: (f32, BoxBorderStyle, Vec<f32>),
    da: DefaultAppearance,
}

impl Plain {
    /// What sheaf's model gives of a text widget ([`TextLook`], as
    /// [`text_look`] reads it from the opened document), as [`Plain`]
    /// says, its `/DA` parsed as the family parses one.
    fn of_look(look: &TextLook) -> Plain {
        let da = match look.da.is_empty() {
            true => DefaultAppearance::default(),
            false => {
                (DefaultAppearance::parse(&look.da, &LIMITS.annots.content)).unwrap_or_default()
            }
        };
        Plain {
            text: look.text.to_string(),
            flags: look.flags,
            max_len: look.max_len,
            quadding: appearance::quadding(look.align),
            width: look.width,
            height: look.height,
            background: look.background.map(appearance::color),
            border_color: look.border_color.map(appearance::color),
            rotation: i32::from(look.rotation),
            border: (
                look.border.width,
                appearance::box_style(look.border.style),
                look.border.dash.clone(),
            ),
            da,
        }
    }

    /// What the family's read layer gives of a widget, as [`Plain`] says.
    fn of_family(read: &forms::WidgetText<'_>) -> Plain {
        let field = read.field();
        let look = read.look();
        Plain {
            text: field.text.to_string(),
            flags: field.flags.bits(),
            max_len: field.max_len,
            quadding: field.quadding,
            width: look.width,
            height: look.height,
            background: look.background,
            border_color: look.border_color,
            rotation: look.rotation,
            border: (
                look.border.width,
                look.border.style,
                look.border.dash.to_vec(),
            ),
            da: read.default_appearance().clone(),
        }
    }

    /// Each part, by the name the dump gives it, as the dump writes it.
    /// The text is its length and checksum: never the value.
    fn parts(&self) -> [(&'static str, String); 13] {
        [
            ("text", text_sum(&self.text)),
            ("ff", format!("{:x}", self.flags)),
            ("maxlen", self.max_len.map_or("-".into(), |n| n.to_string())),
            ("q", quadding_name(self.quadding).into()),
            ("w", self.width.to_string()),
            ("h", self.height.to_string()),
            ("bg", color_name(self.background)),
            ("bc", color_name(self.border_color)),
            ("r", self.rotation.to_string()),
            ("bw", self.border.0.to_string()),
            ("bs", style_name(self.border.1).into()),
            ("dash", numbers(&self.border.2)),
            ("da", escaped(&self.da.to_bytes())),
        ]
    }
}

fn quadding_name(q: Quadding) -> &'static str {
    match q {
        Quadding::Left => "left",
        Quadding::Center => "center",
        Quadding::Right => "right",
    }
}

fn style_name(style: BoxBorderStyle) -> &'static str {
    match style {
        BoxBorderStyle::Solid => "solid",
        BoxBorderStyle::Dashed => "dashed",
        BoxBorderStyle::Beveled => "beveled",
        BoxBorderStyle::Inset => "inset",
        BoxBorderStyle::Underline => "underline",
        _ => "other",
    }
}

/// A color, `-` for none.
fn color_name(c: Option<Color>) -> String {
    match c {
        None => "-".into(),
        Some(Color::Gray(g)) => format!("g:{g}"),
        Some(Color::Rgb(c)) => format!("rgb:{}", numbers(&c)),
        Some(Color::Cmyk(c)) => format!("cmyk:{}", numbers(&c)),
    }
}

/// Numbers, comma apart, as Rust writes each (the shortest that reads
/// back as the same `f32`); `-` for none.
fn numbers(vs: &[f32]) -> String {
    match vs.is_empty() {
        true => "-".into(),
        false => (vs.iter().map(f32::to_string))
            .collect::<Vec<_>>()
            .join(","),
    }
}

/// A text's length and checksum, or `0` for an empty one.
fn text_sum(text: &str) -> String {
    match text.is_empty() {
        true => "0".into(),
        false => format!(
            "{}:{}",
            text.len(),
            sheaf_util::checksum_hex(text.as_bytes())
        ),
    }
}

/// Bytes with what isn't printable ASCII, a backslash and a quote among
/// it, as `\xNN`: a name or a `/DA`, never a value.
fn escaped(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            b' '..=b'~' if b != b'\\' && b != b'"' => out.push(b as char),
            b => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out
}

/// An object's id, `-` for none.
fn id_of(id: Option<ObjectId>) -> String {
    match id {
        Some(id) => format!("{}.{}", id.number, id.generation),
        None => "-".to_string(),
    }
}

fn kind_name(kind: FontKind) -> &'static str {
    match kind {
        FontKind::Type1 => "Type1",
        FontKind::MMType1 => "MMType1",
        FontKind::TrueType => "TrueType",
        FontKind::Type3 => "Type3",
        FontKind::Type0(Descendant::CidFontType0) => "Type0:CIDFontType0",
        FontKind::Type0(Descendant::CidFontType2) => "Type0:CIDFontType2",
        FontKind::Type0(_) => "Type0:other",
        _ => "other",
    }
}

fn program_name(program: FontProgram) -> &'static str {
    match program {
        FontProgram::None => "none",
        FontProgram::Read => "read",
        FontProgram::Malformed => "malformed",
        FontProgram::Capped => "capped",
        _ => "other",
    }
}

/// `o`, followed where it's a reference.
fn reached<'o, R: Resolve<'o>>(objects: R, o: &'o Object) -> Option<&'o Object> {
    match o.as_reference() {
        Some(id) => objects.resolve(id),
        None => Some(o),
    }
}

/// A font dictionary's `/BaseFont` and `/Encoding`, as the dump says
/// them: the name, escaped, or `-`; the encoding's name, or a
/// dictionary's `dict:` with its `/BaseEncoding` (`-` for none) and how
/// many names its `/Differences` gives, or `-` for none.
fn face_of<'o, R: Resolve<'o> + Copy>(objects: R, font: &'o Dictionary) -> (String, String) {
    let name = |o: Option<&'o Object>| match o.and_then(Object::as_name) {
        Some(n) => escaped(n),
        None => "-".into(),
    };
    let entry = |d: &'o Dictionary, key: &[u8]| d.get(key).and_then(|o| reached(objects, o));
    let base = name(entry(font, b"BaseFont"));
    let encoding = match entry(font, b"Encoding").map(Object::value) {
        None => "-".into(),
        Some(PdfValue::Name(n)) => escaped(n.as_bytes()),
        Some(PdfValue::Dict(d)) => {
            let names = (entry(d, b"Differences").and_then(Object::as_array))
                .map_or(0, |a| a.iter().filter(|o| o.as_name().is_some()).count());
            format!("dict:{}:{names}", name(entry(d, b"BaseEncoding")))
        }
        Some(_) => "other".into(),
    };
    (base, encoding)
}

/// What the dump says of the font `name` names in `form`'s default
/// resources, and the font, or why it can't be read. `None` for a `/DA`
/// that names no font: then the text is drawn in Helvetica.
fn font_of<'o, R>(
    objects: R,
    form: &forms::Form<'o>,
    name: Option<&[u8]>,
) -> (Vec<(&'static str, String)>, Result<FieldFont, Error>)
where
    R: Resolve<'o> + crate::pdf::Streams + Copy,
{
    let Some(name) = name else {
        let helvetica = FieldFont::standard(crate::pdf::fonts::Std14::Helvetica);
        return (vec![("font", "none".into())], Ok(helvetica));
    };
    let mut said = vec![("font.name", escaped(name))];
    let font = match form.field_font(objects, name, &LIMITS, &mut ()) {
        Ok(font) => font,
        Err(e) => {
            said.insert(0, ("font", format!("error:{}", appearance::error_kind(&e))));
            return (said, Err(e));
        }
    };
    let class = match font.res().as_ref().map(Res::body) {
        _ if font.is_fallback() => {
            let base = match font.res().as_ref().map(Res::body) {
                Some(ResBody::Objects(objects)) => (objects.first())
                    .and_then(Object::as_dict)
                    .and_then(|d| d.get(b"BaseFont"))
                    .and_then(Object::as_name)
                    .map_or("-".into(), escaped),
                _ => "-".into(),
            };
            format!("missing:{base}")
        }
        None => "direct-refs".into(),
        Some(ResBody::Written(id)) => format!("ref:{}", id_of(Some(*id))),
        Some(ResBody::Objects(_)) => "direct".into(),
        Some(_) => "other".into(),
    };
    said.insert(0, ("font", class));
    said.push(("font.kind", kind_name(font.font().kind()).into()));
    said.push(("font.program", program_name(font.font().program()).into()));
    if let Ok(Some((dict, _))) = form.font(objects, name, &LIMITS) {
        let (base, encoding) = face_of(objects, dict);
        said.push(("font.base", base));
        said.push(("font.enc", encoding));
    }
    (said, Ok(font))
}

/// The family's appearance of `read` with its text `text`, in `font`, the
/// family's reading of its `/DA`'s font, and the fallback: what
/// [`made_name`] says.
fn family_made(
    read: &forms::WidgetText<'_>,
    text: &str,
    font: &Result<FieldFont, Error>,
) -> Result<MadeWithFallback, Unmade> {
    let font = font.as_ref().map_err(|e| Unmade::from(e.clone()))?;
    let res = font.res().ok_or(Unmade::FontNotBindable)?;
    let mut field = read.field();
    field.text = text;
    let look = read.look();
    forms::text_with_fallback(
        &field,
        &look,
        read.default_appearance(),
        res,
        &font.layout(),
        &Fallback::SANS.field(),
        &LIMITS,
    )
}

/// sheaf's appearance of `look` with its text `text`, as the emitter makes
/// one: in the font its `/DA` names in `src`'s `/DR`, read once a name by
/// the source ([`appearance::font`]), named as sheaf names it (copy
/// `copy`'s, [`appearance::font_res`]); and the same made with the font
/// named as the family names it, to set beside the family's. A font named
/// by an entry, which the emitter writes as an object of its own, is
/// bound here to no object: the content's bytes are the same.
fn ours_made(src: &Source, look: &TextLook, text: &str, copy: u32) -> (Made, Made) {
    let look = TextLook {
        text: text.into(),
        ..look.clone()
    };
    let font = match appearance::font(src, &look) {
        Ok(font) => font,
        Err(e) => return (Err(e.clone().into()), Err(e.into())),
    };
    let name = look.font_name().unwrap_or_default();
    let written = |at: FontAt<'_>| match at {
        FontAt::Object(id) => id,
        FontAt::Entry(_) => ObjectId::new(0, 0),
    };
    let res = appearance::font_res(&font, src.id(), copy, &name, written);
    let ours = appearance::make(&look, &font, res);
    let theirs = match font.res() {
        Some(res) => appearance::make(&look, &font, res),
        None => Err(Unmade::FontNotBindable),
    };
    (ours, theirs)
}

/// An appearance made, and the fallback's glyphs it draws; or why none was.
type Made = Result<MadeWithFallback, Unmade>;

/// An appearance as the dump says it: its size, whether it overflowed, its
/// content's length and checksum and the resources it draws with, by their
/// ids; or `unmade:` and why.
fn made_name(made: &Made) -> String {
    match made.as_ref().map(|m| &m.appearance) {
        Ok(a) => {
            let needs: Vec<String> = (a.needs.0.iter())
                .map(|need| format!("{:032x}", need.res.id().0))
                .collect();
            format!(
                "{},{},{}:{},{}",
                a.font_size,
                u8::from(a.overflowed),
                a.content.len(),
                sheaf_util::checksum_hex(&a.content),
                needs.join("+")
            )
        }
        Err(why) => format!("unmade:{}", appearance::unmade_kind(why)),
    }
}

/// sheaf's appearance, its font named as the family names it, beside the
/// family's: `=` where they're the same (or unmade alike), else the parts
/// that differ, `made` where one is made and the other isn't.
fn versus(ours: &Made, theirs: &Made) -> String {
    fn appearance(m: &Made) -> Result<&Appearance, &Unmade> {
        m.as_ref().map(|m| &m.appearance)
    }
    match (appearance(ours), appearance(theirs)) {
        (Ok(a), Ok(b)) => {
            let parts = [
                ("bbox", a.bbox == b.bbox),
                ("matrix", a.matrix == b.matrix),
                ("size", a.font_size == b.font_size),
                ("ovf", a.overflowed == b.overflowed),
                ("content", a.content == b.content),
                ("needs", a.needs == b.needs),
            ];
            let differ: Vec<&str> = (parts.iter())
                .filter(|(_, same)| !same)
                .map(|(name, _)| *name)
                .collect();
            match differ.is_empty() {
                true => "=".into(),
                false => differ.join(","),
            }
        }
        (Err(a), Err(b)) if appearance::unmade_kind(a) == appearance::unmade_kind(b) => "=".into(),
        (Err(_), Err(_)) => "unmade".into(),
        _ => "made".into(),
    }
}

/// One of the dump's lines: where it is, then its fields, `name=value`
/// each, tab apart.
struct Line(String);

impl Line {
    fn new(at: &str, what: &str) -> Line {
        Line(format!("{at}\t{what}"))
    }

    fn field(&mut self, name: &str, value: &str) {
        self.0.push_str(&format!("\t{name}={value}"));
    }
}

/// What the family read of the file's form, where it read one: its text
/// fields' widgets by their objects.
struct Family<'o> {
    form: forms::Form<'o>,
    /// Each terminal field's widgets by object, with the field's index:
    /// every kind's, so a widget the two read as other kinds is said.
    widgets: HashMap<ObjectId, (usize, usize)>,
    /// Text fields' widgets written as direct dictionaries, which no
    /// widget of sheaf's is.
    direct: usize,
}

impl<'o> Family<'o> {
    fn of(form: forms::Form<'o>) -> Family<'o> {
        let mut widgets = HashMap::new();
        let mut direct = 0;
        for (i, field) in form.terminals() {
            for (n, w) in field.widgets().iter().enumerate() {
                match w.id() {
                    Some(id) => {
                        widgets.entry(id).or_insert((i, n));
                    }
                    None if field.kind() == Some(forms::FieldKind::Text) => direct += 1,
                    None => {}
                }
            }
        }
        Family {
            form,
            widgets,
            direct,
        }
    }

    /// Its text fields' widgets that have objects.
    fn text_widgets(&self) -> impl Iterator<Item = ObjectId> + '_ {
        (self.widgets.iter())
            .filter(|(_, (i, _))| self.form.fields()[*i].kind() == Some(forms::FieldKind::Text))
            .map(|(id, _)| *id)
    }
}

/// The dump's counts for a file, which its first line says.
#[derive(Default)]
struct Counts {
    text: usize,
    equal: usize,
    differ: usize,
    sheaf_only: usize,
    made: usize,
    unmade: usize,
    /// Text widgets the opened document gives no look of: none should be.
    unlooked: usize,
    ours_made: usize,
    ours_unmade: usize,
    /// Widgets whose appearance sheaf's plain data make other than the
    /// family's reading does.
    versus_differ: usize,
}

/// The dump's lines for file `path`, open as `src` in `store`: one for
/// the file, then one for each text widget sheaf reads on a page, in page
/// order and each page's in `/Annots` order (a widget on two pages is on
/// both). sheaf's reading of each is its model's, of the file opened as a
/// document.
fn lines_of(path: &str, store: &Arc<Store>, src: &Src) -> Vec<String> {
    let pdf: &LazyPdf = src.pdf();
    let form = src.form();
    let unkept = Unkept::default();
    let objects = Objects::new(pdf, &unkept);
    let family = match pdf.catalog() {
        Ok(catalog) => forms::read(objects, catalog, &LIMITS),
        Err(_) => Ok(None),
    };
    let (family, said) = match family {
        Ok(Some(f)) => (Some(Family::of(f)), "read".to_string()),
        Ok(None) => (None, "none".to_string()),
        Err(e) => (None, format!("error:{}", appearance::error_kind(&e))),
    };
    let doc = opened(store, src, path);
    let st = doc.as_ref().map(Document::state);
    // Each widget of the document by its page's index and its object.
    let mut aids: HashMap<(u32, ObjectId), AnnotId> = HashMap::new();
    for (index, &page) in st.iter().flat_map(|st| st.order().iter().enumerate()) {
        for (aid, w) in st.into_iter().flat_map(|st| st.widgets_on(page)) {
            if let Some(r) = &w.src {
                aids.entry((index as u32, r.id)).or_insert(aid);
            }
        }
    }
    let fields: HashMap<_, &Field> = form.fields.iter().map(|f| (f.id, f)).collect();
    let mut fonts: HashMap<Option<Vec<u8>>, _> = HashMap::new();
    let mut counts = Counts::default();
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for (&page, widgets) in &form.widgets {
        for w in widgets {
            let Some(field) = fields.get(&w.field).filter(|f| f.kind == FieldKind::Text) else {
                continue;
            };
            counts.text += 1;
            seen.insert(w.object);
            let at = format!(
                "{path}\t{}\t{}\t{}",
                page + 1,
                w.slot,
                id_of(Some(w.object))
            );
            let mut line = Line::new(&at, "text");
            line.field("field", &id_of(Some(field.object)));
            line.field("merged", &u8::from(w.merged).to_string());
            let aid = aids.get(&(page, w.object)).copied();
            let look = st.zip(aid).and_then(|(st, aid)| text_look(st, aid));
            let Some(look) = look else {
                counts.unlooked += 1;
                line.field("look", "-");
                lines.push(line.0);
                continue;
            };
            let ours = Plain::of_look(&look);
            for (name, value) in ours.parts() {
                line.field(name, &value);
            }
            let sample = sample(&ours.text, ours.flags, ours.max_len);
            line.field("sample", &text_sum(&sample));
            let theirs = 'family: {
                let Some(family) = &family else {
                    counts.sheaf_only += 1;
                    line.field("family", "-");
                    break 'family None;
                };
                let Some(&(i, n)) = family.widgets.get(&w.object) else {
                    counts.sheaf_only += 1;
                    line.field("family", "-");
                    break 'family None;
                };
                let theirs = &family.form.fields()[i];
                // The field the family reads the widget as, where it isn't
                // sheaf's: the tree read otherwise.
                let other = (theirs.id() != Some(field.object)).then(|| id_of(theirs.id()));
                if theirs.kind() != Some(forms::FieldKind::Text) {
                    counts.sheaf_only += 1;
                    let kind = theirs.kind().map_or("-".into(), |k| format!("{k:?}"));
                    line.field("family", &format!("kind:{kind}"));
                    if let Some(other) = &other {
                        line.field("family.field", other);
                    }
                    break 'family None;
                }
                let widget = &theirs.widgets()[n];
                let read = match forms::widget_text(theirs, widget, objects, &LIMITS) {
                    Ok(read) => read,
                    Err(why) => {
                        counts.differ += 1;
                        line.field(
                            "family",
                            &format!("unmade:{}", appearance::unmade_kind(&why)),
                        );
                        break 'family None;
                    }
                };
                let plain = Plain::of_family(&read);
                let differ: Vec<(&str, String)> = (other.map(|other| ("field", other)).into_iter())
                    .chain(
                        (ours.parts().into_iter())
                            .zip(plain.parts())
                            .filter(|(a, b)| a.1 != b.1)
                            .map(|(_, b)| b),
                    )
                    .collect();
                match differ.is_empty() {
                    true => {
                        counts.equal += 1;
                        line.field("family", "=");
                    }
                    false => {
                        counts.differ += 1;
                        let names: Vec<&str> = differ.iter().map(|(n, _)| *n).collect();
                        line.field("family", &names.join(","));
                        for (name, value) in &differ {
                            line.field(&format!("family.{name}"), value);
                        }
                    }
                }
                let name = read.font_name().map(<[u8]>::to_vec);
                let (said, font) = fonts
                    .entry(name.clone())
                    .or_insert_with(|| font_of(objects, &family.form, name.as_deref()));
                for (key, value) in said.iter() {
                    line.field(key, value);
                }
                let made = family_made(&read, &sample, font);
                match made.is_ok() {
                    true => counts.made += 1,
                    false => counts.unmade += 1,
                }
                line.field("made", &made_name(&made));
                Some(made)
            };
            // sheaf's, as the emitter makes it, and beside the family's.
            let copy = aid.and_then(|aid| st?.annot(aid)?.widget().map(|w| w.inst.copy));
            let (made, named) = ours_made(src, &look, &sample, copy.unwrap_or(0));
            match made.is_ok() {
                true => counts.ours_made += 1,
                false => counts.ours_unmade += 1,
            }
            line.field("ours", &made_name(&made));
            if let Some(n) = (made.as_ref().ok())
                .map(|m| m.glyphs.len())
                .filter(|&n| n > 0)
            {
                line.field("fallback", &n.to_string());
            }
            let vs = theirs.map_or("-".into(), |theirs| versus(&named, &theirs));
            counts.versus_differ += usize::from(vs != "=" && vs != "-");
            line.field("ours.vs", &vs);
            lines.push(line.0);
        }
    }
    let family_only = family.as_ref().map_or(0, |f| {
        f.text_widgets().filter(|id| !seen.contains(id)).count() + f.direct
    });
    let mut head = Line::new(&format!("{path}\t-\t-\t-"), "file");
    head.field("pages", &src.pages().len().to_string());
    head.field("class", form_class(form.facts.class));
    if let Some(why) = &form.facts.damaged {
        head.field("damaged", &escaped(why.as_bytes()));
    }
    head.field("family", &said);
    head.field("text", &counts.text.to_string());
    // Text fields' widgets on no page of the page tree: on page templates.
    let outside: usize = (form.fields.iter())
        .filter(|f| f.kind == FieldKind::Text)
        .map(|f| usize::from(f.outside))
        .sum();
    head.field("outside", &outside.to_string());
    head.field("equal", &counts.equal.to_string());
    head.field("differ", &counts.differ.to_string());
    head.field("sheaf_only", &counts.sheaf_only.to_string());
    head.field("family_only", &family_only.to_string());
    head.field("made", &counts.made.to_string());
    head.field("unmade", &counts.unmade.to_string());
    if counts.unlooked > 0 || doc.is_none() {
        head.field("document", if doc.is_some() { "read" } else { "-" });
        head.field("unlooked", &counts.unlooked.to_string());
    }
    head.field("ours_made", &counts.ours_made.to_string());
    head.field("ours_unmade", &counts.ours_unmade.to_string());
    head.field("vs_differ", &counts.versus_differ.to_string());
    let mut file = vec![head.0];
    file.extend(lines);
    file
}

/// `src`, in `store`, opened as a document is, as file `path`: what sheaf's
/// reading of each widget comes from. None where its facts don't read.
fn opened(store: &Arc<Store>, src: &Src, path: &str) -> Option<Document> {
    let policy = Box::new(FixedCount(1));
    let mut doc = crate::open::start(store.clone(), src.clone(), path.into(), None, policy);
    doc.ready(crate::open::finish(src, &NoProgress)).ok()?;
    Some(doc)
}

fn form_class(class: FormClass) -> &'static str {
    match class {
        FormClass::None => "none",
        FormClass::AcroForm => "acroform",
        FormClass::Hybrid => "hybrid",
        FormClass::XfaOnly => "xfa-only",
    }
}

/// The dump's lines for the file at `path` of the corpus ([`lines_of`]):
/// one line, `unopened` and why, for a file that doesn't open.
fn dump_file(corpus: &Corpus, path: &str) -> Vec<String> {
    let none = |why: String| vec![format!("{path}\t-\t-\t-\tunopened: {why}")];
    let bytes = match std::fs::read(corpus.root.join(path)) {
        Ok(bytes) => bytes,
        Err(e) => return none(e.to_string()),
    };
    let kind = BlobKind::file(Path::new(path), None);
    let store = Store::new();
    match store.open(bytes.into(), kind) {
        Ok(src) => lines_of(path, &store, &src),
        Err(e) => none(format!("{e:#}").replace(['\n', '\t'], " ")),
    }
}

/// Every text widget of every form in the corpus's manifest, to set
/// beside itself after a change to what reads a form or makes a text
/// field's appearance: a line a file and one a text widget
/// ([`lines_of`]), the files in the order of their paths, written to the
/// path in [`DUMP`]. Two runs over the same code write the same file.
/// With [`DUMP_ONLY`] it reads the one file named, and prints its lines
/// to stderr.
#[test]
#[ignore = "a dump of the corpus's text fields, written to $SHEAF_FIELDS_DUMP"]
fn fields_dump() {
    let to = std::env::var_os(DUMP);
    let only = std::env::var(DUMP_ONLY).ok();
    if to.is_none() && only.is_none() {
        eprintln!("skipped: neither {DUMP} nor {DUMP_ONLY} is set");
        return;
    }
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let started = Instant::now();
    let paths: std::collections::BTreeSet<String> = (corpus.forms().unwrap().into_iter())
        .map(|e| e.path)
        .collect();
    eprintln!("{} forms", paths.len());
    let lines = each_path(
        paths.into_iter().collect(),
        (only.as_deref(), DUMP_ONLY),
        |path| dump_file(&corpus, path),
        |path, said| format!("{path}\t-\t-\t-\tpanicked: {said}"),
    );
    let mut said: BTreeMap<String, usize> = BTreeMap::new();
    for line in &lines {
        let fields: Vec<&str> = line.split('\t').skip(4).collect();
        let what = fields.first().copied().unwrap_or_default();
        let key = match what {
            "text" => {
                let family = (fields.iter())
                    .find_map(|f| f.strip_prefix("family="))
                    .unwrap_or("?");
                format!("text family={family}")
            }
            what if what.starts_with("unopened") => "unopened".into(),
            what if what.starts_with("panicked") => "panicked".into(),
            what => what.into(),
        };
        *said.entry(key).or_default() += 1;
    }
    eprintln!(
        "{} lines, {said:?}, in {:.0} s",
        lines.len(),
        started.elapsed().as_secs_f64()
    );
    written(&lines, only.is_some(), to);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms::fixture::Form;
    use crate::pdf::{dictionary, literal};

    /// A form of one page with a text field of each kind of reading the
    /// dump tells apart: one that's its own widget, drawn in `/DR`'s
    /// Helvetica by reference; a field of two kid widgets, one with a
    /// `/DA` of its own naming a font `/DR` lacks, the other inheriting
    /// its parent's dashed border style and a direct font; a comb field;
    /// one whose `/DA` names no font; and a check box, which isn't a line.
    /// In a store of its own, as the document it's opened as is.
    fn file() -> (Arc<Store>, Src) {
        let mut f = Form::new(1);
        let own = f.text("own", 0, [10.0, 700.0, 210.0, 720.0]);
        f.fill(own, "Trenton");
        f.dict(own).set("Q", 2);
        f.dict(own).set(
            "MK",
            dictionary! { "BG" => vec![Object::from(1)], "BC" => vec![0.into(), 0.into(), 1.into()] },
        );

        let parent = f.text("parent", 0, [10.0, 600.0, 210.0, 620.0]);
        let kid = |f: &mut Form, rect: [i64; 4]| {
            let r: Vec<Object> = rect.iter().map(|&v| Object::from(v)).collect();
            let id = f.pdf.add_object(dictionary! {
                "Type" => Object::name("Annot"), "Subtype" => Object::name("Widget"),
                "Rect" => r, "F" => 4, "P" => f.pages[0], "Parent" => parent,
            });
            let mut annots = f.annots(0);
            annots.push(id);
            f.set_annots(0, &annots);
            id
        };
        let a = kid(&mut f, [10, 560, 110, 580]);
        let b = kid(&mut f, [120, 560, 220, 580]);
        // The parent is no widget now: its kids are.
        let d = f.dict(parent);
        for key in ["Subtype", "Rect", "F", "P", "Type"] {
            d.remove(key.as_bytes());
        }
        d.set("Kids", vec![Object::from(a), Object::from(b)]);
        d.set("DA", literal("/Direct 9 Tf 0 0 0.502 rg"));
        let mut annots = f.annots(0);
        annots.retain(|&x| x != parent);
        f.set_annots(0, &annots);
        f.dict(a).set("DA", literal("/Gone 0 Tf 1 g"));
        f.dict(b).set(
            "BS",
            dictionary! { "W" => 2, "S" => Object::name("D"), "D" => vec![Object::from(4), 2.into()] },
        );
        let acroform = f.acroform;
        let fonts = (f.dict(acroform).get_mut(b"DR"))
            .and_then(|dr| dr.as_dict_mut())
            .and_then(|dr| dr.get_mut(b"Font"))
            .and_then(|fonts| fonts.as_dict_mut())
            .expect("the fixture's /DR /Font");
        fonts.set(
            "Direct",
            dictionary! {
                "Type" => Object::name("Font"), "Subtype" => Object::name("Type1"),
                "BaseFont" => Object::name("Times-Roman"),
                "Encoding" => dictionary! {
                    "Type" => Object::name("Encoding"),
                    "Differences" => vec![Object::from(128), Object::name("Euro"), Object::name("bullet")],
                },
            },
        );

        let comb = f.text("comb", 0, [10.0, 500.0, 110.0, 520.0]);
        f.dict(comb).set("Ff", 1 << 24);
        f.dict(comb).set("MaxLen", 5);

        let bare = f.text("bare", 0, [10.0, 400.0, 110.0, 420.0]);
        f.dict(bare).set("DA", literal("0 g"));

        f.check_box("box", 0, [10.0, 300.0, 20.0, 310.0], true);
        let store = Store::new();
        let kind = BlobKind::file(Path::new("f.pdf"), None);
        let src = store.open(f.bytes().into(), kind).unwrap();
        (store, src)
    }

    /// The fields of the line of the widget in slot `slot`, after its
    /// place.
    fn fields(lines: &[String], slot: usize) -> Vec<&str> {
        let line = (lines.iter())
            .find(|l| l.split('\t').nth(2) == Some(&*slot.to_string()))
            .unwrap_or_else(|| panic!("no line for slot {slot}:\n{}", lines.join("\n")));
        line.split('\t').skip(5).collect()
    }

    /// Field `name` of `fields`.
    fn field<'f>(fields: &[&'f str], name: &str) -> &'f str {
        let prefix = format!("{name}=");
        (fields.iter())
            .find_map(|f| f.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("no {name} in {fields:?}"))
    }

    /// Each text widget is a line, read by both and made by both, the two
    /// readings said equal where they are and the parts that differ where
    /// they don't, the font by what it is, sheaf's appearance beside the
    /// family's; and the dump says the same of it every time.
    #[test]
    fn the_fields_dump_says_what_each_text_widget_is() {
        let (store, src) = file();
        let lines = lines_of("f.pdf", &store, &src);
        assert_eq!(lines, lines_of("f.pdf", &store, &src));
        assert_eq!(lines.len(), 1 + 5, "{}", lines.join("\n"));
        assert_eq!(
            lines[0],
            "f.pdf\t-\t-\t-\tfile\tpages=1\tclass=acroform\tfamily=read\ttext=5\toutside=0\t\
             equal=5\tdiffer=0\tsheaf_only=0\tfamily_only=0\tmade=5\tunmade=0\t\
             ours_made=5\tours_unmade=0\tvs_differ=0"
        );
        // sheaf's plain data make what the family's do, each widget's.
        for slot in 0..5 {
            assert_eq!(field(&fields(&lines, slot), "ours.vs"), "=", "slot {slot}");
        }

        // Its own widget: its value's length and checksum, never the
        // value; right-justified, a white background and a blue border;
        // drawn in /DR's Helvetica, which /DR holds by reference.
        let own = fields(&lines, 0);
        assert_eq!(field(&own, "merged"), "1");
        let value = format!("7:{}", sheaf_util::checksum_hex(b"Trenton"));
        assert_eq!(field(&own, "text"), value);
        assert_eq!(field(&own, "sample"), value);
        assert_eq!(field(&own, "q"), "right");
        assert_eq!(field(&own, "bg"), "g:1");
        assert_eq!(field(&own, "bc"), "rgb:0,0,1");
        assert_eq!(field(&own, "da"), "/Helv 10 Tf 0 g");
        assert_eq!(field(&own, "family"), "=");
        assert!(field(&own, "font").starts_with("ref:"), "{own:?}");
        assert_eq!(field(&own, "font.base"), "Helvetica");
        assert_eq!(field(&own, "font.enc"), "WinAnsiEncoding");
        assert!(!lines.join("\n").contains("Trenton"));
        let made: Vec<&str> = field(&own, "made").split(',').collect();
        assert_eq!(made[..2], ["10", "0"], "{made:?}");
        // sheaf names /DR's font by its source, copy and object, so its
        // content names it otherwise, and is as long.
        let ours: Vec<&str> = field(&own, "ours").split(',').collect();
        assert_eq!(ours[..2], made[..2]);
        let len = |m: &[&str]| m[2].split(':').next().unwrap().to_string();
        assert_eq!(len(&ours), len(&made));
        let helv = match field(&own, "font").strip_prefix("ref:") {
            Some(id) => {
                let (n, g) = id.split_once('.').unwrap();
                ObjectId::new(n.parse().unwrap(), g.parse().unwrap())
            }
            None => panic!("{own:?}"),
        };
        let named = appearance::font_res_id(src.id(), 0, FontAt::Object(helv));
        assert_eq!(ours[3], format!("{:032x}", named.0));
        assert_ne!(ours[3], made[3]);

        // A kid with a /DA of its own, whose font /DR lacks: the family
        // falls back by name, to Helvetica.
        let gone = fields(&lines, 1);
        assert_eq!(field(&gone, "merged"), "0");
        assert_eq!(field(&gone, "da"), "/Gone 0 Tf 1 g");
        assert_eq!(field(&gone, "family"), "=");
        assert_eq!(field(&gone, "font"), "missing:Helvetica");
        assert_eq!(field(&gone, "font.name"), "Gone");
        // A fallback is named by its dictionary, by both.
        assert_eq!(field(&gone, "ours"), field(&gone, "made"));

        // The other kid: its parent's navy /DA in a direct font with
        // /Differences; its dashes, which sheaf's model holds.
        let dashed = fields(&lines, 2);
        assert_eq!(field(&dashed, "da"), "/Direct 9 Tf 0 0 0.502 rg");
        assert_eq!(field(&dashed, "bs"), "dashed");
        assert_eq!(field(&dashed, "bw"), "2");
        assert_eq!(field(&dashed, "dash"), "4,2");
        assert_eq!(field(&dashed, "family"), "=");
        assert_eq!(field(&dashed, "font"), "direct");
        assert_eq!(field(&dashed, "ours"), field(&dashed, "made"));
        assert_eq!(field(&dashed, "font.base"), "Times-Roman");
        assert_eq!(field(&dashed, "font.enc"), "dict:-:2");

        // A comb field of five cells: its sample fills them.
        let comb = fields(&lines, 3);
        assert_eq!(field(&comb, "maxlen"), "5");
        assert_eq!(field(&comb, "ff"), "1000000");
        assert_eq!(field(&comb, "sample"), text_sum("12345"));

        // A /DA that names no font draws in Helvetica.
        let bare = fields(&lines, 4);
        assert_eq!(field(&bare, "da"), "0 g");
        assert_eq!(field(&bare, "font"), "none");
        assert_eq!(field(&bare, "sample"), text_sum("Sample 123"));
    }

    /// A sample is the field's own value, cut at `/MaxLen`, else the
    /// digits to it, else two lines or one.
    #[test]
    fn a_sample_is_never_longer_than_max_len() {
        let multiline = field::flags::MULTILINE;
        assert_eq!(sample("Trenton", 0, Some(3)), "Tre");
        assert_eq!(sample("Trenton", 0, Some(0)), "Trenton");
        assert_eq!(sample("", 0, Some(12)), "123456789012");
        // To the cells sheaf lays out, the cap an appearance is made under.
        let cells = crate::forms::read::MAX_LAYOUT_LEN as usize;
        assert_eq!(sample("", 0, Some(u32::MAX)).len(), cells);
        assert_eq!(appearance::limits().comb_cells, cells);
        assert_eq!(sample("", multiline, None).lines().count(), 2);
        assert_eq!(sample("", 0, None), "Sample 123");
    }
}
