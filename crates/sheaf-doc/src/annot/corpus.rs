//! What sheaf makes of every annotation of the corpus's files
//! ([`annots_dump`]): how the open reads each slot of a page's
//! `/Annots`, what a flatten reads of it, and what a save writes of the
//! mark it converts to, a line each, numbers and checksums only. It
//! asserts nothing: it is for setting a change to what reads, flattens or
//! draws annotations beside what was there. The files are the ones the
//! glyph dump reads ([`each_dumped`]).

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use crate::annot::{Annot, Box2, Pt, emission};
use crate::attachments::Objects;
use crate::content::corpus::{each_dumped, written};
use crate::content::sink::{self, Sink};
use crate::content::{Res, Reserved};
use crate::file::{ForeignAnnot, Found, InFile, annots_in, is_ours};
use crate::forms::corpus::Corpus;
use crate::geom::{Affine, Rect};
use crate::import::family::{Get, ReadAnnot, limits, read};
use crate::import::{NotAMark, convert_page};
use crate::pdf::annots::SynthesizedAp;
use crate::pdf::annots::{AppearanceEntry, Rules, page_annots_with, stream_placement_matrix};
use crate::pdf::gfx;
use crate::pdf::{Dictionary, Object, ObjectId, Value};
use crate::serialize::built::{Held, StampForm};
use crate::source::ObjRef;
use crate::source::appearance::{Shows, frame};
use crate::store::{BlobKind, Src, Store};

/// Where [`annots_dump`] writes: a path. Unset, it reads nothing.
/// `just annots-dump <file>` sets it.
const DUMP: &str = "SHEAF_ANNOTS_DUMP";

/// A corpus path whose records [`annots_dump`] prints in full to stderr,
/// reading that file alone: to set two runs beside each other.
const DUMP_ONLY: &str = "SHEAF_ANNOTS_DUMP_ONLY";

/// Set to `1`, [`annots_dump`] reads each annotation through `pdf-annots`
/// (M18 slice 24): the open's walk with each other tool's annotation read
/// by the family and converted from what it read
/// ([`crate::file::family`], [`crate::import::family`]), and a flatten's
/// line from what the family reads of it ([`family_flatten_fields`]), the
/// lines the same as without it where the two read alike. Beside them,
/// each page's annotations as the family's own walk of the page reads them
/// ([`page_annots_with`]) set beside sheaf's walk's, and each annotation
/// the family refused, a line each in `<the dump>.family.tsv`.
const FAMILY: &str = "SHEAF_ANNOTS_DUMP_FAMILY";

/// One of the dump's lines: where it is, what it says of it, and its
/// fields, `name=value` each, tab apart. Where the records are printed
/// in full, each value is too, texts and bytes as they are.
struct Line {
    tsv: String,
    full: Option<String>,
}

impl Line {
    /// A line for `at` (the file, the page, the slot and its object, tab
    /// apart) saying `what`.
    fn new(at: &str, what: &str, full: bool) -> Line {
        Line {
            tsv: format!("{at}\t{what}"),
            full: full.then(|| format!("{at}\t{what}")),
        }
    }

    /// A value written as it is: a number, an object's id, a name
    /// escaped.
    fn field(&mut self, name: &str, value: &str) {
        self.tsv.push_str(&format!("\t{name}={value}"));
        if let Some(full) = &mut self.full {
            full.push_str(&format!("\t{name}={value}"));
        }
    }

    /// A text: its checksum, or nothing for an empty one.
    fn text(&mut self, name: &str, text: &str) {
        let sum = match text.is_empty() {
            true => String::new(),
            false => sheaf_util::checksum_hex(text.as_bytes()),
        };
        self.tsv.push_str(&format!("\t{name}={sum}"));
        if let Some(full) = &mut self.full {
            full.push_str(&format!("\t{name}=\"{}\"", escaped(text)));
        }
    }

    /// Bytes: their length and checksum.
    fn bytes(&mut self, name: &str, bytes: &[u8]) {
        let sum = sheaf_util::checksum_hex(bytes);
        self.tsv
            .push_str(&format!("\t{name}={}:{sum}", bytes.len()));
        if let Some(full) = &mut self.full {
            let text = escaped(&String::from_utf8_lossy(bytes));
            full.push_str(&format!("\t{name}={}:\"{text}\"", bytes.len()));
        }
    }

    /// Numbers by their bits.
    fn bits(&mut self, name: &str, vs: &[f32]) {
        let bits: Vec<String> = vs.iter().map(|v| format!("{:08x}", v.to_bits())).collect();
        self.tsv.push_str(&format!("\t{name}={}", bits.join(",")));
        if let Some(full) = &mut self.full {
            let values: Vec<String> = vs.iter().map(|v| format!("{v}")).collect();
            full.push_str(&format!(
                "\t{name}={}({})",
                bits.join(","),
                values.join(",")
            ));
        }
    }

    /// A matrix by its bits, `I` for the identity's.
    fn matrix(&mut self, name: &str, m: Affine) {
        let vs = [m.a, m.b, m.c, m.d, m.e, m.f];
        let identity = Affine::IDENTITY;
        let is = [
            identity.a, identity.b, identity.c, identity.d, identity.e, identity.f,
        ];
        match vs.map(f32::to_bits) == is.map(f32::to_bits) {
            true => self.field(name, "I"),
            false => self.bits(name, &vs),
        }
    }

    /// The line, and where the records are printed in full, its record
    /// to stderr.
    fn finish(self, lines: &mut Vec<String>) {
        if let Some(full) = self.full {
            eprintln!("== {full}");
        }
        lines.push(self.tsv);
    }
}

/// `s` with what isn't printable ASCII, a backslash, a quote and a tab
/// among it, as `\u{…}`.
fn escaped(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' '..='~' if c != '\\' && c != '"' => c.to_string(),
            c => format!("\\u{{{:x}}}", u32::from(c)),
        })
        .collect()
}

/// A name as written, escaped.
fn name_of(bytes: &[u8]) -> String {
    escaped(&String::from_utf8_lossy(bytes))
}

/// An object's id, `-` for none.
fn id_of(id: Option<ObjectId>) -> String {
    match id {
        Some(id) => format!("{}.{}", id.number, id.generation),
        None => "-".to_string(),
    }
}

/// A box's corners and size, by their bits.
fn box_bits(b: Box2) -> [f32; 4] {
    [b.x, b.y, b.w, b.h]
}

fn rect_bits(r: Rect) -> [f32; 4] {
    [r.x0, r.y0, r.x1, r.y1]
}

/// A sink that writes nothing: each resource it's asked for given the
/// next object number, and its id kept, to say what an appearance's
/// resources bind.
#[derive(Default)]
struct Asked(Vec<u128>);

impl Sink for Asked {
    fn dict(&mut self, _: ObjectId) -> Option<Dictionary> {
        None
    }

    fn array(&mut self, _: ObjectId) -> Option<Vec<Object>> {
        None
    }

    fn resource(&mut self, res: &Res) -> ObjectId {
        self.0.push(res.id().0);
        ObjectId::new(self.0.len() as u32, 0)
    }

    fn reserved(&mut self, _: Option<&Object>) -> Reserved {
        Reserved::default()
    }
}

/// What appearance `ap` of dictionary `dict`, as a save writes them,
/// says: the dictionary written, the appearance's box, its content and
/// the resources its form binds, each `category/name/id`, under names
/// that start with `prefix`.
fn made(line: &mut Line, prefix: &str, dict: &Dictionary, ap: SynthesizedAp) {
    let mut out = Vec::new();
    match crate::pdf::write::write(&mut out, &Object::from(dict.clone())) {
        Ok(()) => line.bytes(&format!("{prefix}dict"), &out),
        Err(_) => line.field(&format!("{prefix}dict"), "unwritable"),
    }
    line.bits(&format!("{prefix}bbox"), &rect_bits(Rect::from(ap.bbox)));
    let mut asked = Asked::default();
    let form = sink::form_dict(&mut asked, ap.bbox, ap.matrix, &ap.needs);
    let resources = form.get(b"Resources").and_then(Object::as_dict);
    let mut needs = Vec::new();
    for (i, id) in asked.0.iter().enumerate() {
        let at = ObjectId::new(i as u32 + 1, 0);
        let mut bound = "unbound".to_string();
        for (key, category) in resources.into_iter().flat_map(Dictionary::iter) {
            for (name, object) in category.as_dict().into_iter().flat_map(Dictionary::iter) {
                if object.as_reference() == Some(at) {
                    bound = format!("{}/{}", name_of(key), name_of(name));
                }
            }
        }
        needs.push(format!("{bound}/{id:032x}"));
    }
    line.field(&format!("{prefix}needs"), &needs.join(","));
    line.bytes(&format!("{prefix}content"), &ap.content);
}

/// What a save writes of mark `a`, holding `held`: its dictionary and
/// appearance, and each companion's, `-` with none.
fn make_line(at: &str, a: Option<(&Annot, Held)>, full: bool, lines: &mut Vec<String>) {
    let mut line = Line::new(at, "make", full);
    match a {
        None => line.field("mark", "-"),
        Some((a, held)) => {
            let emitted = emission(a, held);
            made(&mut line, "", &emitted.main, emitted.appearance);
            for (i, (dict, ap)) in emitted.companions.into_iter().enumerate() {
                made(&mut line, &format!("companion{}.", i + 1), &dict, ap);
            }
        }
    }
    line.finish(lines);
}

/// What a page stamp's appearance draws, form `form` of `pdf`, as a save
/// holds it: the form by its own number, with its frame.
fn stamp_of<P: Objects + ?Sized>(pdf: &P, form: Option<ObjectId>) -> Option<StampForm> {
    let id = form?;
    let (bbox, matrix) = crate::stamp_lib::form_frame(pdf, id)?;
    Some(StampForm::written(id, bbox, matrix))
}

/// A mark's JSON: its length and checksum.
fn json_of(a: &Annot) -> Vec<u8> {
    serde_json::to_vec(a).expect("a mark is JSON")
}

/// Why a conversion gave no mark, as the dump says it.
fn not_a_mark(why: &NotAMark) -> String {
    match why {
        NotAMark::NotComment => "-:not-comment".to_string(),
        NotAMark::Unsupported(sub) => format!("-:unsupported:{}", escaped(sub)),
        NotAMark::Part => "-:part".to_string(),
    }
}

/// What the open lists of another tool's annotation `f`: every field,
/// texts as checksums, and the mark it converts to (`converted`, beside
/// the one the open kept), or why it doesn't.
fn foreign_fields(line: &mut Line, f: &ForeignAnnot, converted: Option<&Result<Annot, NotAMark>>) {
    line.field("subtype", &escaped(&f.subtype));
    line.text("author", &f.author);
    line.text("contents", &f.contents);
    line.text("modified", &f.modified);
    line.bits("rect", &box_bits(f.rect));
    line.text("name", &f.name);
    match &f.parent {
        Some(parent) => line.text("parent", parent),
        None => line.field("parent", "-"),
    }
    line.field("grouped", &u8::from(f.grouped).to_string());
    match &f.state {
        Some((model, state)) => {
            line.text("state.model", model);
            line.text("state", state);
        }
        None => line.field("state", "-"),
    }
    line.text("created", &f.created);
    line.field("slot", &f.slot.to_string());
    line.field("hidden", &u8::from(f.hidden).to_string());
    match f.color {
        Some([r, g, b]) => line.field("color", &format!("{r:02x}{g:02x}{b:02x}")),
        None => line.field("color", "-"),
    }
    match (&f.mark, converted) {
        (Some(mark), _) => line.bytes("mark", &json_of(mark)),
        (None, Some(Err(why))) => line.field("mark", &not_a_mark(why)),
        (None, None) if f.is_link() => line.field("mark", "-:link"),
        (None, _) => line.field("mark", "-"),
    }
    line.field("stamp", &id_of(f.stamp));
}

/// `/Rect` of `dict`, its four numbers, as the open reads another tool's:
/// None where it isn't four.
fn raw_rect<P: Objects + ?Sized>(pdf: &P, dict: &Dictionary) -> Option<Box2> {
    let (_, rect) = pdf.dereference(dict.get(b"Rect")?).ok()?;
    let r: Vec<f32> = (rect.as_array()?.iter())
        .map(|v| v.as_number().unwrap_or(0.0) as f32)
        .collect();
    let [x0, y0, x1, y1] = <[f32; 4]>::try_from(r).ok()?;
    Some(Box2::from_corners(Pt { x: x0, y: y0 }, Pt { x: x1, y: y1 }))
}

/// What a flatten reads of the annotation in slot `slot` of page `index`
/// of `src`, `entry` there ([`Shows`]): its flags, the state it shows,
/// and the form it shows by its value (another tool's comment) and by
/// its state alone (a widget), each with its `/BBox`, its `/Matrix` and
/// the matrix that places it on `rect`.
fn flatten_fields(
    line: &mut Line,
    src: &Src,
    at: (u32, usize),
    entry: &Object,
    rect: Option<Box2>,
) {
    let shows = match entry.as_reference() {
        Some(id) => Shows::read(&ObjRef {
            src: src.clone(),
            id,
        }),
        None => Shows::in_slot(src, at.0, at.1),
    };
    let Some(shows) = shows else {
        line.field("shows", "-");
        return;
    };
    line.field("flags", &shows.flags.to_string());
    match &shows.state {
        Some(state) => line.field("state", &name_of(state)),
        None => line.field("state", "-"),
    }
    let by_value = shows.form(src, shows.state.as_deref(), true).map(|f| f.id);
    let by_state = shows.form(src, shows.state.as_deref(), false).map(|f| f.id);
    for (tag, form) in [("by_value", by_value), ("by_state", by_state)] {
        if tag == "by_state" && by_state == by_value {
            line.field(tag, "same");
            continue;
        }
        line.field(tag, &id_of(form));
        let Some(id) = form else {
            continue;
        };
        let read = frame(&ObjRef {
            src: src.clone(),
            id,
        });
        let Some((bbox, matrix)) = read else {
            line.field(&format!("{tag}.frame"), "-");
            continue;
        };
        line.bits(&format!("{tag}.bbox"), &rect_bits(bbox));
        line.matrix(&format!("{tag}.matrix"), matrix);
        match rect.and_then(|rect| Affine::onto(matrix.bounds(bbox), rect)) {
            Some(onto) => line.matrix(&format!("{tag}.onto"), onto),
            None => line.field(&format!("{tag}.onto"), "-"),
        }
    }
}

/// What a flatten reads of annotation `dict` of `pdf`, object `id`, as
/// the family reads it ([`flatten_fields`]'s line): its flags and the
/// state it shows from the family's `/F`, `/AP` and `/AS`, the form it
/// shows by its value and by its state by sheaf's rule over the family's
/// [`AppearanceEntry`] (with none named, another tool's comment shows the
/// state its `/V` names, or its parent's, else `/Off`; a widget, none;
/// an empty `/AS` names none), each with the box, matrix and placement
/// [`stream_placement_matrix`] gives on `rect` (on a unit box where there
/// is no rect, its placement then `-`), or `.frame=-` where it gives none.
fn family_flatten_fields<P: Objects + ?Sized>(
    line: &mut Line,
    pdf: &P,
    dict: &Dictionary,
    id: Option<ObjectId>,
    rect: Option<Box2>,
) {
    let a = match read(pdf, dict, id) {
        Ok(a) => a,
        Err(why) => {
            line.field("shows", &format!("refused:{}", escaped(&why.to_string())));
            return;
        }
    };
    line.field("flags", &a.flags().bits().to_string());
    let aps = a.appearances();
    let state = (aps.and_then(|ap| ap.state.as_deref())).filter(|s| !s.is_empty());
    match state {
        Some(state) => line.field("state", &name_of(state)),
        None => line.field("state", "-"),
    }
    // A value named, or written as a string, as PDFium reads either; an
    // empty one is none.
    let named = |o: &Object| -> Option<Vec<u8>> {
        let (_, o) = pdf.dereference(o).ok()?;
        let bytes = match o.value() {
            Value::Name(n) => n.as_bytes(),
            Value::String(s, _) => s.as_bytes()?,
            _ => return None,
        };
        (!bytes.is_empty()).then(|| bytes.to_vec())
    };
    let value = || -> Option<Vec<u8>> {
        if let Some(v) = dict.get(b"V").and_then(named) {
            return Some(v);
        }
        let (_, parent) = pdf.dereference(dict.get(b"Parent")?).ok()?;
        parent.as_dict()?.get(b"V").and_then(named)
    };
    let normal = aps.and_then(|ap| ap.normal.as_ref());
    let by_value = match normal {
        None => None,
        Some(AppearanceEntry::Single(id)) => Some(*id),
        Some(entry @ AppearanceEntry::States(states)) => {
            let chosen = match state {
                Some(state) => state.to_vec(),
                None => match value() {
                    Some(v) if states.iter().any(|(k, _)| **k == *v) => v,
                    _ => b"Off".to_vec(),
                },
            };
            entry.resolve(Some(&chosen), true)
        }
    };
    let by_state = match normal {
        None => None,
        Some(AppearanceEntry::Single(id)) => Some(*id),
        Some(entry) => state.and_then(|state| entry.resolve(Some(state), true)),
    };
    for (tag, form) in [("by_value", by_value), ("by_state", by_state)] {
        if tag == "by_state" && by_state == by_value {
            line.field(tag, "same");
            continue;
        }
        line.field(tag, &id_of(form));
        let Some(id) = form else {
            continue;
        };
        let on = match rect {
            Some(r) => gfx::Rect {
                x0: r.x,
                y0: r.y,
                x1: r.x + r.w,
                y1: r.y + r.h,
            },
            None => gfx::Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 1.0,
                y1: 1.0,
            },
        };
        let placed = stream_placement_matrix(Get(pdf), id, on, &limits());
        let Ok(Some((bbox, matrix, onto))) = placed else {
            line.field(&format!("{tag}.frame"), "-");
            continue;
        };
        line.bits(&format!("{tag}.bbox"), &rect_bits(Rect::from(bbox)));
        line.matrix(&format!("{tag}.matrix"), Affine::from(matrix));
        match rect {
            Some(_) => line.matrix(&format!("{tag}.onto"), Affine::from(onto)),
            None => line.field(&format!("{tag}.onto"), "-"),
        }
    }
}

/// Page `page` of `pdf`, `/Annots` `entries`, as the family's own walk
/// of a page reads it ([`page_annots_with`]) beside sheaf's walk, a line
/// at `at`: whether it yields an annotation in each slot sheaf's walk
/// finds a dictionary in, and in no other, or why it read none.
fn family_page_line<P: Objects + ?Sized>(
    at: &str,
    pdf: &P,
    page: ObjectId,
    entries: &[Object],
) -> String {
    let sheaf: Vec<usize> = (entries.iter().enumerate())
        .filter(|(_, e)| {
            (pdf.dereference(e).ok()).is_some_and(|(_, o)| matches!(o.value(), Value::Dict(_)))
        })
        .map(|(i, _)| i)
        .collect();
    let slots = |s: &[usize]| {
        let s: Vec<String> = s.iter().map(usize::to_string).collect();
        s.join(",")
    };
    let Ok(dict) = pdf.get_dictionary(page) else {
        return format!("{at}\tfamily-page\tpage=unread");
    };
    match page_annots_with(
        Get(pdf),
        dict,
        Some(page),
        Rules::Pdfium,
        &limits(),
        &mut (),
    ) {
        Ok(list) => {
            let family: Vec<usize> = list.iter().filter_map(ReadAnnot::index).collect();
            match family == sheaf {
                true => format!("{at}\tfamily-page\tslots=same\tcount={}", sheaf.len()),
                false => format!(
                    "{at}\tfamily-page\tslots=differ\tsheaf={}\tfamily={}",
                    slots(&sheaf),
                    slots(&family)
                ),
            }
        }
        Err(why) => format!(
            "{at}\tfamily-page\terror={}\tsheaf={}",
            escaped(&why.to_string()),
            slots(&sheaf)
        ),
    }
}

/// What a slot sheaf passes over is, as written: whether it names
/// anything, a dictionary, its `/Subtype` and how many numbers its
/// `/Rect` has.
fn passed_fields<P: Objects + ?Sized>(line: &mut Line, pdf: &P, entry: &Object) {
    let Ok((_, object)) = pdf.dereference(entry) else {
        line.field("entry", "unresolved");
        return;
    };
    let Value::Dict(dict) = object.value() else {
        line.field("entry", "not-dict");
        return;
    };
    line.field("entry", "dict");
    match dict.get(b"Subtype").map(Object::value) {
        Some(Value::Name(n)) => line.field("subtype", &name_of(n.as_bytes())),
        Some(_) => line.field("subtype", "not-name"),
        None => line.field("subtype", "-"),
    }
    let rect = (dict.get(b"Rect"))
        .and_then(|r| pdf.dereference(r).ok())
        .and_then(|(_, r)| r.as_array().map(<[Object]>::len));
    match rect {
        Some(n) => line.field("rect", &n.to_string()),
        None => line.field("rect", "-"),
    }
}

/// The entries of page `page`'s `/Annots`, as the open walks them: the
/// array its page dictionary names, its references followed.
fn slots<P: Objects + ?Sized>(pdf: &P, page: ObjectId) -> Vec<Object> {
    let Ok(dict) = pdf.get_dictionary(page) else {
        return Vec::new();
    };
    let annots = (dict.get(b"Annots")).and_then(|a| pdf.dereference(a).ok().map(|(_, o)| o));
    annots
        .and_then(Object::as_array)
        .map(<[Object]>::to_vec)
        .unwrap_or_default()
}

/// The dump's lines for file `path`, open as `src`: one for the file, and
/// for each slot of each page's `/Annots`, three: what the open reads of
/// it (`read`: `mark`, its `part`, another tool's `foreign`, or `passed`),
/// what a flatten reads of it (`flatten`) and what a save writes of the
/// mark it is (`make`). With `family`, each other tool's annotation is
/// read through `pdf-annots` ([`FAMILY`]): one it refuses is `refused`,
/// with why and what it is as written, and each page with annotations
/// has a line more, `family-page`, after its slots.
fn lines_of(path: &str, src: &Src, full: bool, family: bool) -> Vec<String> {
    let pdf = src.pdf();
    let pages = src.pages();
    let turned = pages.iter().map(|p| (p.id, p.geom.rotate));
    let (found, refused) = match family {
        true => {
            let walked = crate::file::family::annots_in(pdf, turned);
            (walked.found, walked.refused)
        }
        false => (annots_in(pdf, turned), HashMap::new()),
    };
    let mut lines = Vec::new();
    let mut counts: [usize; 4] = [0; 4];
    for (index, page) in pages.iter().enumerate() {
        let index = index as u32;
        let entries = slots(pdf, page.id);
        let listed: HashMap<usize, &InFile> = (found.pages.get(&index).into_iter().flatten())
            .map(|a| (a.slot, a))
            .collect();
        // Other tools' comments, converted again together as the open
        // converts them, to say why one gives no mark.
        let comments: Vec<usize> = (found.pages.get(&index).into_iter().flatten())
            .filter(|a| matches!(&a.found, Found::Foreign(f) if !f.is_link()))
            .map(|a| a.slot)
            .collect();
        let dicts: Vec<(Option<ObjectId>, &Dictionary)> = (comments.iter())
            .map(|&slot| {
                let (id, object) = pdf
                    .dereference(&entries[slot])
                    .expect("listed, so it resolves");
                (id, object.as_dict().expect("listed, so a dictionary"))
            })
            .collect();
        let converted = match family {
            true => {
                let read: Vec<ReadAnnot<'_>> = (dicts.iter())
                    .map(|&(id, d)| read(pdf, d, id).expect("listed, so it reads"))
                    .collect();
                let pairs: Vec<(&Dictionary, &ReadAnnot<'_>)> =
                    dicts.iter().map(|(_, d)| *d).zip(&read).collect();
                crate::import::family::convert_page(
                    pdf,
                    &pairs,
                    page.geom.rotate,
                    Default::default(),
                )
            }
            false => {
                let dicts: Vec<&Dictionary> = dicts.iter().map(|(_, d)| *d).collect();
                convert_page(pdf, &dicts, page.geom.rotate, Default::default())
            }
        };
        let converted: HashMap<usize, &Result<Annot, NotAMark>> =
            comments.iter().copied().zip(&converted).collect();
        let count = entries
            .len()
            .max(listed.keys().map(|s| s + 1).max().unwrap_or(0));
        for slot in 0..count {
            let entry = entries.get(slot);
            let object = match listed.get(&slot) {
                Some(a) => a.object,
                None => entry
                    .and_then(|e| pdf.dereference(e).ok())
                    .and_then(|(id, _)| id),
            };
            let at = format!("{path}\t{}\t{slot}\t{}", index + 1, id_of(object));
            let mut read = Line::new(&at, "read", full);
            let mut flatten = Line::new(&at, "flatten", full);
            let mut make = None;
            // What a flatten reads of the dictionary in this slot, on
            // `rect`.
            let flattened = |flatten: &mut Line, entry: &Object, rect: Option<Box2>| match family {
                true => match pdf.dereference(entry).ok() {
                    Some((id, o)) => match o.as_dict() {
                        Some(dict) => family_flatten_fields(flatten, pdf, dict, id, rect),
                        None => flatten.field("shows", "-"),
                    },
                    None => flatten.field("shows", "-"),
                },
                false => flatten_fields(flatten, src, (index, slot), entry, rect),
            };
            match (listed.get(&slot).map(|a| &a.found), entry) {
                (Some(Found::Mark { annot, held }), _) => {
                    counts[0] += 1;
                    read.field("is", "mark");
                    read.bytes("annot", &json_of(annot));
                    read.field("held", &id_of(*held));
                    flatten.field("pinned", &annot.kind.flags().to_string());
                    let held = Held {
                        stamp: stamp_of(pdf, *held)
                            .filter(|_| matches!(annot.kind, super::AnnotKind::PageStamp { .. })),
                        file: held.filter(|_| {
                            matches!(annot.kind, super::AnnotKind::FileAttachment { .. })
                        }),
                    };
                    make = Some((annot, held));
                }
                (Some(Found::Foreign(f)), entry) => {
                    counts[1] += 1;
                    read.field("is", "foreign");
                    let again = converted.get(&slot).copied();
                    if let Some(again) = again {
                        assert_eq!(again.as_ref().ok(), f.mark.as_deref(), "{at}");
                    }
                    foreign_fields(&mut read, f, again);
                    match entry {
                        Some(entry) => flattened(&mut flatten, entry, Some(f.rect)),
                        None => flatten.field("shows", "-"),
                    }
                    if let Some(mark) = &f.mark {
                        let stamp = stamp_of(pdf, f.stamp);
                        counts[3] += 1;
                        make = Some((&**mark, Held { stamp, file: None }));
                    }
                }
                (None, Some(entry)) if is_ours(pdf, entry) => {
                    counts[2] += 1;
                    read.field("is", "part");
                    flatten.field("part", "-");
                }
                (None, Some(entry)) => {
                    counts[2] += 1;
                    match refused.get(&(index, slot)) {
                        Some(why) => {
                            read.field("is", "refused");
                            read.field("why", &escaped(&why.to_string()));
                        }
                        None => read.field("is", "passed"),
                    }
                    passed_fields(&mut read, pdf, entry);
                    let rect = (pdf.dereference(entry).ok())
                        .and_then(|(_, o)| o.as_dict())
                        .and_then(|d| raw_rect(pdf, d));
                    flattened(&mut flatten, entry, rect);
                }
                (None, None) => {
                    counts[2] += 1;
                    read.field("is", "passed");
                    read.field("entry", "absent");
                    flatten.field("shows", "-");
                }
            }
            read.finish(&mut lines);
            flatten.finish(&mut lines);
            make_line(&at, make, full, &mut lines);
        }
        if family && !entries.is_empty() {
            let at = format!("{path}\t{}\t-\t{}", index + 1, id_of(Some(page.id)));
            lines.push(family_page_line(&at, pdf, page.id, &entries));
        }
    }
    let [marks, foreign, passed, converted] = counts;
    let mut head = Line::new(&format!("{path}\t-\t-\t-"), "file", full);
    head.field("pages", &pages.len().to_string());
    head.field("slots", &(marks + foreign + passed).to_string());
    head.field("marks", &marks.to_string());
    head.field("foreign", &foreign.to_string());
    head.field("converted", &converted.to_string());
    head.field("passed", &passed.to_string());
    let mut file = Vec::new();
    head.finish(&mut file);
    file.extend(lines);
    file
}

/// The dump's lines for the file at `path` of the corpus ([`lines_of`]):
/// one line, `unopened` and why, for a file that doesn't open.
fn dump_file(corpus: &Corpus, path: &str, full: bool, family: bool) -> Vec<String> {
    let none = |why: String| vec![format!("{path}\t-\t-\t-\tunopened: {why}")];
    let bytes = match std::fs::read(corpus.root.join(path)) {
        Ok(bytes) => bytes,
        Err(e) => return none(e.to_string()),
    };
    let kind = BlobKind::file(Path::new(path), None);
    match Store::new().open(bytes.into(), kind) {
        Ok(src) => lines_of(path, &src, full, family),
        Err(e) => none(format!("{e:#}").replace(['\n', '\t'], " ")),
    }
}

/// Every annotation of the corpus's files, to set beside itself after a
/// change to what reads, flattens or draws annotations: a line a file and
/// three a slot ([`lines_of`]), the files in the order of their paths,
/// written to the path in [`DUMP`]. Two runs over the same code write the
/// same file. With [`DUMP_ONLY`] it reads the one file named, and prints
/// each record in full to stderr.
#[test]
#[ignore = "a dump of the corpus's annotations, written to $SHEAF_ANNOTS_DUMP"]
fn annots_dump() {
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
    let full = only.is_some();
    let family = std::env::var(FAMILY).is_ok_and(|v| v == "1");
    let lines = each_dumped(
        &corpus,
        (only.as_deref(), DUMP_ONLY),
        |path| dump_file(&corpus, path, full, family),
        |path, said| format!("{path}\t-\t-\t-\tpanicked: {said}"),
    );
    // The family's own lines go beside the dump, so that it holds the same
    // lines with the family as without.
    let (beside, lines): (Vec<String>, Vec<String>) = (lines.into_iter()).partition(|l| {
        l.split('\t')
            .nth(4)
            .is_some_and(|w| w.starts_with("family-"))
    });
    let mut said: std::collections::BTreeMap<&str, usize> = Default::default();
    for line in lines.iter().chain(&beside) {
        let mut columns = line.split('\t').skip(4);
        let what = columns.next().unwrap_or_default();
        let what = match what {
            "read" => columns.next().unwrap_or_default(),
            "family-page" => columns
                .next()
                .unwrap_or_default()
                .split('=')
                .next()
                .unwrap_or_default(),
            "make" if line.ends_with("\tmark=-") => "make -",
            what if what.starts_with("unopened") => "unopened",
            what if what.starts_with("panicked") => "panicked",
            what => what,
        };
        *said.entry(what).or_default() += 1;
    }
    eprintln!(
        "{} lines, {said:?}, in {:.0} s",
        lines.len(),
        started.elapsed().as_secs_f64()
    );
    if family {
        let mut pages: std::collections::BTreeMap<&str, usize> = Default::default();
        let mut why: std::collections::BTreeMap<&str, usize> = Default::default();
        for line in &beside {
            let field = line.split('\t').nth(5).unwrap_or_default();
            *pages
                .entry(field.split_once('=').map_or(field, |(k, v)| match k {
                    "slots" => v,
                    _ => k,
                }))
                .or_default() += 1;
        }
        for line in &lines {
            if let Some(w) = line.split('\t').find_map(|f| f.strip_prefix("why=")) {
                *why.entry(w).or_default() += 1;
            }
        }
        eprintln!("family: pages {pages:?}; refused {why:?}");
        let beside_to = to.as_ref().map(|to| {
            let mut to = to.clone();
            to.push(".family.tsv");
            to
        });
        written(&beside, false, beside_to);
    }
    written(&lines, full, to);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annot::{AnnotId, AnnotKind};
    use crate::pdf::{Document as Pdf, Stream, dictionary, literal};
    use sheaf_edit::Rotation;

    /// A file of one page whose `/Annots` holds one of each kind of slot
    /// the dump tells apart: a mark of sheaf's and its companion, another
    /// tool's comments (one written in the page's array, one stamp with
    /// its form, one attachment no conversion holds), a link, a widget
    /// showing a state, and an entry that's no dictionary.
    fn file() -> Src {
        let mut pdf = Pdf::with_version("1.7");
        let pages = pdf.new_object_id();
        let rect = || Object::from(vec![10.into(), 10.into(), 30.into(), 20.into()]);
        let at = Box2 {
            x: 10.0,
            y: 10.0,
            w: 30.0,
            h: 10.0,
        };
        let ours = Annot {
            id: AnnotId(9),
            kind: AnnotKind::Replace {
                rects: vec![at],
                struck: "old".into(),
                at,
            },
            color: [1, 2, 3],
            opacity: 1.0,
            stroke: 1.0,
            author: "me".into(),
            contents: "mine".into(),
            modified: String::new(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        };
        let written = crate::serialize::built::to_pdf(&ours, &mut pdf);
        let main = pdf.add_object(written.main);
        let [mut strike] = <[_; 1]>::try_from(written.companions).ok().unwrap();
        strike.set("IRT", main);
        strike.set("RT", Object::name("Group"));
        let part = pdf.add_object(strike);
        let square = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Square"), "Rect" => rect(),
            "C" => vec![1.into(), 0.into(), 0.into()], "CA" => 0.5, "T" => literal("Reviewer"),
        });
        let note = Object::from(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Text"), "Rect" => rect(),
            "Contents" => literal("Why?"),
        });
        let link = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Link"), "Rect" => rect(),
        });
        let form = |pdf: &mut Pdf, content: &[u8]| {
            pdf.add_object(Stream::new(
                dictionary! {
                    "Type" => Object::name("XObject"), "Subtype" => Object::name("Form"),
                    "BBox" => vec![0.into(), 0.into(), 20.into(), 10.into()],
                },
                content.to_vec(),
            ))
        };
        let on = form(&mut pdf, b"0 0 20 10 re f");
        let off = form(&mut pdf, b"");
        let widget = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Widget"), "Rect" => rect(),
            "F" => 4, "AS" => Object::name("On"),
            "AP" => dictionary! { "N" => dictionary! { "On" => on, "Off" => off } },
        });
        let page_form = form(&mut pdf, b"0 0 1 rg 0 0 20 10 re f");
        let stamp = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Stamp"),
            "Name" => Object::name("SBApproved"), "Rect" => rect(),
            "AP" => dictionary! { "N" => page_form },
        });
        let embedded = pdf.add_object(Stream::new(dictionary! {}, b"attached".to_vec()));
        let attached = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("FileAttachment"),
            "Rect" => rect(),
            "FS" => dictionary! {
                "Type" => Object::name("Filespec"), "F" => literal("a.txt"),
                "EF" => dictionary! { "F" => embedded },
            },
        });
        let number = pdf.add_object(Object::from(7));
        let list: Vec<Object> = vec![
            main.into(),
            part.into(),
            square.into(),
            note,
            link.into(),
            widget.into(),
            stamp.into(),
            attached.into(),
            number.into(),
        ];
        let page = pdf.add_object(dictionary! {
            "Type" => Object::name("Page"), "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Annots" => list,
        });
        pdf.objects.insert(
            pages,
            dictionary! { "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1 }
                .into(),
        );
        let catalog =
            pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages });
        pdf.trailer.set("Root", catalog);
        crate::open::tests::lazily(&mut pdf)
    }

    /// The fields of the line of slot `slot` that says `what`, after it.
    fn fields<'l>(lines: &'l [String], slot: usize, what: &str) -> Vec<&'l str> {
        let line = (lines.iter())
            .find(|l| {
                let c: Vec<&str> = l.split('\t').collect();
                c[2] == slot.to_string() && c[4] == what
            })
            .unwrap_or_else(|| panic!("no {what} line for slot {slot}:\n{}", lines.join("\n")));
        line.split('\t').skip(5).collect()
    }

    /// Field `name` of `fields`.
    fn field<'f>(fields: &[&'f str], name: &str) -> &'f str {
        let prefix = format!("{name}=");
        (fields.iter())
            .find_map(|f| f.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("no {name} in {fields:?}"))
    }

    /// Each slot is read as what it is, flattened as what it shows and
    /// made as the mark it is, and the dump says the same of it every
    /// time, its records printed in full or not.
    #[test]
    fn the_annots_dump_says_what_each_slot_is() {
        let src = file();
        let lines = lines_of("f.pdf", &src, false, false);
        assert_eq!(lines, lines_of("f.pdf", &src, false, false));
        assert_eq!(lines, lines_of("f.pdf", &src, true, false));
        // Read through the family, every slot reads alike, and the page's
        // own walk finds the slots sheaf's does.
        let (beside, family): (Vec<String>, Vec<String>) = (lines_of("f.pdf", &src, false, true))
            .into_iter()
            .partition(|l| l.contains("\tfamily-page\t"));
        assert_eq!(family, lines);
        assert_eq!(beside.len(), 1, "{beside:?}");
        assert!(beside[0].ends_with("\tslots=same\tcount=8"), "{beside:?}");
        assert_eq!(lines.len(), 1 + 9 * 3, "{}", lines.join("\n"));
        assert_eq!(
            lines[0],
            "f.pdf\t-\t-\t-\tfile\tpages=1\tslots=9\tmarks=1\tforeign=5\tconverted=3\tpassed=3"
        );
        let is: Vec<&str> = (0..9)
            .map(|s| field(&fields(&lines, s, "read"), "is"))
            .collect();
        assert_eq!(
            is,
            [
                "mark", "part", "foreign", "foreign", "foreign", "passed", "foreign", "foreign",
                "passed"
            ]
        );

        // The mark's own appearance is pinned; a save writes it and its
        // companion.
        assert_eq!(fields(&lines, 0, "flatten"), ["pinned=4"]);
        let made = fields(&lines, 0, "make");
        assert!(field(&made, "companion1.dict").contains(':'), "{made:?}");

        // Another tool's square: what it's listed by, the mark it is, and
        // what a save makes of that.
        let square = fields(&lines, 2, "read");
        assert_eq!(field(&square, "subtype"), "Square");
        assert_eq!(field(&square, "color"), "ff0000");
        assert_eq!(
            field(&square, "author"),
            sheaf_util::checksum_hex(b"Reviewer")
        );
        assert_eq!(field(&square, "contents"), "");
        assert!(field(&square, "mark").contains(':'));
        assert_eq!(
            fields(&lines, 2, "flatten"),
            ["flags=0", "state=-", "by_value=-", "by_state=same"]
        );
        let made = fields(&lines, 2, "make");
        assert!(field(&made, "needs").contains("ExtGState/"), "{made:?}");

        // Written in the page's array, it has no object; a link is no
        // mark, an attachment none without its file, a stamp one that
        // names its form.
        assert_eq!(lines[1 + 3 * 3].split('\t').nth(3), Some("-"));
        assert_eq!(field(&fields(&lines, 4, "read"), "mark"), "-:link");
        assert_eq!(fields(&lines, 4, "make"), ["mark=-"]);
        let attached = fields(&lines, 7, "read");
        assert_eq!(field(&attached, "mark"), "-:unsupported:FileAttachment");
        let stamp = fields(&lines, 6, "read");
        assert_ne!(field(&stamp, "stamp"), "-");
        assert!(field(&fields(&lines, 6, "make"), "needs").contains("XObject/"));

        // The widget shows its state's form, placed on its /Rect.
        let widget = fields(&lines, 5, "read");
        assert_eq!(
            widget,
            ["is=passed", "entry=dict", "subtype=Widget", "rect=4"]
        );
        let shows = fields(&lines, 5, "flatten");
        assert_eq!(field(&shows, "flags"), "4");
        assert_eq!(field(&shows, "state"), "On");
        assert_ne!(field(&shows, "by_value"), "-");
        assert_eq!(field(&shows, "by_state"), "same");
        assert_eq!(field(&shows, "by_value.matrix"), "I");
        // [0 0 20 10] onto the rect 10 10 30 20: moved by 10, 10.
        let ten = format!("{:08x}", 10f32.to_bits());
        let (one, zero) = (format!("{:08x}", 1f32.to_bits()), "00000000");
        let onto = [&*one, zero, zero, &*one, &*ten, &*ten].join(",");
        assert_eq!(field(&shows, "by_value.onto"), onto);

        // What's no dictionary is said so.
        assert_eq!(fields(&lines, 8, "read"), ["is=passed", "entry=not-dict"]);
        assert_eq!(fields(&lines, 8, "flatten"), ["shows=-"]);
    }
}
