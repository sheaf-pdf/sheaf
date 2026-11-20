//! sheaf's reader of files' fonts ([`Foreign`]) set beside the PDF
//! layer's (`pdf_fonts::Font`), while both are in the tree: every font a
//! file's pages and forms name, read by each from the same objects within
//! the same budget, and everything a page's interpretation asks of a font
//! asked of both. Where they part is a row ([`Row`]), with the class of
//! difference it's taken for ([`class`]). It goes with [`Foreign`].
//!
//! [`the_two_readers_agree_on_every_font`] is the corpus, written to
//! the path in [`DIFF`]; [`the_two_readers_agree_on_the_fixtures`] the
//! fixtures built in code, and [`compared`] the fonts the tests beside
//! this build one by one. "Agree" is as far as the classes go: a
//! difference of no class fails, and the classes are few, each ruled
//! once, before the reader that goes is deleted.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::{self, Debug};
use std::ops::{ControlFlow, Range};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use super::{Descendant, FontKind, FontProgram, Fonts, Foreign};
use crate::content::interp::{Kind, Opts, interpret};
use crate::content::page::{Concat, Objects, ReadObjects};
use crate::content::{PageContent, fixture};
use crate::forms::corpus::Corpus;
use crate::geom::Rect;
use crate::pdf::fonts as new;
use crate::pdf::{Dictionary, Document as Pdf, Object, ObjectId};
use crate::source::Limits;
use crate::store::{BlobKind, Store};

const LIM: &Limits = &Limits::DEFAULT;

/// Where [`the_two_readers_agree_on_every_font`] writes its rows: a path.
const DIFF: &str = "SHEAF_FONT_DIFF";

/// A corpus path (`forms/irs/f1040.pdf`): the one file
/// [`the_two_readers_agree_on_every_font`] then reads.
const ONLY: &str = "SHEAF_FONT_DIFF_ONLY";

/// How many rows one observable of one font, read one way, is written
/// in: the first codes or strings it differs at. [`Row::n`] says how many
/// there were.
const ROWS: usize = 3;

/// How many codes of a Type 0 font are asked of beyond those its strings
/// show, spread over two bytes.
const SPREAD: u32 = 4096;

/// One place the two readers part.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Row {
    /// What the difference is taken for ([`class`]).
    class: &'static str,
    /// The font: its object, or its name where it's written in its
    /// resources; `(descendant)` after a Type 0 font's CIDFont read as a
    /// font of its own.
    font: String,
    /// Read with its program (`programs`), or for text (`text`).
    mode: &'static str,
    /// What was asked.
    observable: &'static str,
    /// The code or the string it was asked of, or `-`.
    code: String,
    /// What sheaf's reader says.
    old: String,
    /// What the PDF layer's says.
    new: String,
    /// Whether a page shows it: the code in this font, or the string; of
    /// the font itself, whether a page shows a string in it.
    shown: bool,
    /// How many codes or strings this observable differs at in this font,
    /// of which this row is one of the first, and how many of them a page
    /// shows.
    n: usize,
    n_shown: usize,
    /// What the font is: its kind, what each read of its program, its
    /// `/BaseFont` and `/Encoding`.
    about: String,
}

impl Row {
    fn tsv(&self, file: &str) -> String {
        let Row {
            class,
            font,
            mode,
            observable,
            code,
            old,
            new,
            shown,
            n,
            n_shown,
            about,
        } = self;
        let shown = if *shown { "shown" } else { "-" };
        format!(
            "{class}\t{file}\t{font}\t{mode}\t{observable}\t{code}\t{old}\t{new}\t{shown}\t{n}\t\
             {n_shown}\t{about}"
        )
    }
}

const HEADER: &str =
    "class\tfile\tfont\tmode\tobservable\tcode\tsheaf\tfamily\tshown\tn\tn_shown\tfont_is";

/// The codes WinAnsiEncoding leaves empty, where PDFium's table, and the
/// PDF layer's, has the bullet (class `D5`).
const EMPTY_IN_WIN_ANSI: [&str; 6] = [
    "0x007f/1", "0x0081/1", "0x008d/1", "0x008f/1", "0x0090/1", "0x009d/1",
];

/// The class a difference is taken for: one the slice's plan names, or
/// one named here, or `unclassed`, which fails the tests here. `about` is
/// what the font is, as both read it. The classes, each ruled in the
/// plan's record of this step:
///
/// - `D5`: an encoding's empty slots. WinAnsiEncoding's six have the
///   bullet and MacRomanEncoding's 0xF0 the apple, as in PDFium's tables:
///   their text, and in a standard 14 font drawn without a program their
///   widths and boxes, which sheaf's reader guessed.
/// - `P1`: a simple font's program says what its codes are, where its
///   dictionary names no encoding or only differences from one: the
///   program's own encoding, or a TrueType program's character map.
///   sheaf's reader took StandardEncoding, or no text. `P1n` where the
///   program gives a code no text and sheaf's reader had StandardEncoding's.
/// - `P2`: as `P1`, the font read without its program (for text, or one
///   that embeds none): the text its name and its encoding's names give.
/// - `T3`: a Type 3 font's text and glyphs by its encoding's names, a
///   base encoding's among them.
/// - `U1`: a `/ToUnicode` value of U+FFFF is no text. `U2`: half a
///   surrogate pair is U+FFFD. `U3`: a Type 0 font's code its
///   `/ToUnicode` leaves out has its CID's text, U+FFFD for CID 0.
/// - `M1`: a predefined CMap's code with no mapping is CID 0, as in
///   PDFium's tables; sheaf's reader took Adobe's `notdef` ranges.
/// - `K1`: an `/Encoding` name that's no CMap: two bytes a code, each its
///   CID, as PDFium reads it; sheaf's reader guessed.
/// - `N1`: a simple font's widths as PDFium's accessors read its numbers
///   (a negative width, a code with no glyph in a standard 14 font).
/// - `B1`: a font whose program doesn't read, named as one of the
///   standard 14 is: drawn as that one. One damaged file.
/// - `D8`: a crafted program one reader reads and the other doesn't, and
///   all that follows of the font's heights and boxes. No file's.
/// - `E1`: an embedded CMap that takes from one it uses, which PDFium
///   doesn't read: the PDF layer's says its codes are guessed. No file's.
/// - `Z1`: a zero's sign, in a Type 3 font's box turned by its matrix.
/// - `C1`: what reading a Type 1 program is charged.
/// - `X0`: a CIDFont read as a font of its own, which no page does.
fn class(row: &Row, about: &About) -> &'static str {
    let (old, new, observable) = (row.old.as_str(), row.new.as_str(), row.observable);
    let text = matches!(observable, "code_text" | "text");
    let metric = matches!(
        observable,
        "width"
            | "byte_widths"
            | "width_guessed"
            | "width_guessed_in"
            | "glyph_box"
            | "char_box"
            | "extent"
    );
    if row.font.ends_with("(descendant)") {
        return "X0";
    }
    if observable == "budget left" {
        return "C1";
    }
    if about.embeds.0 != about.embeds.1 {
        return "D8";
    }
    match about.kind.0 {
        3 if old.replace("-0[80000000]", "0[00000000]") == new => "Z1",
        3 if text || metric => "T3",
        4 | 5 if about.codes_guessed == (true, false) => "K1",
        4 | 5 if observable == "approx.codes" => "E1",
        4 | 5 if text && about.encoding.starts_with("Identity") => match (old, new) {
            (old, "None") if old.contains("\\u{ffff}") => "U1",
            ("None", _) => "U3",
            (_, "Some(\"\u{fffd}\")") => "U2",
            _ => "unclassed",
        },
        4 | 5 if !about.encoding.starts_with('(') => "M1",
        0..=2 => {
            let empty = EMPTY_IN_WIN_ANSI.contains(&row.code.as_str());
            let bullet = new.contains('\u{2022}') && old == "None";
            let apple = row.code == "0x00f0/1" && new.contains("\\u{f8ff}");
            let unread = about.program == (2, 2) && about.embeds == (2, 2);
            match () {
                () if text && (bullet || apple) => "D5",
                () if metric && empty => "D5",
                () if observable == "width_guessed_in" => "D5",
                () if text && new == "None" && old.contains("\\u{ffff}") => "U1",
                () if unread && (metric || observable.starts_with("approx.")) => "B1",
                () if text && about.program == (1, 1) && new == "None" => "P1n",
                () if (text || observable == "char_box") && about.program == (1, 1) => "P1",
                () if text => "P2",
                () if metric => "N1",
                () => "unclassed",
            }
        }
        _ => "unclassed",
    }
}

/// What a font is, for a row and for [`class`].
#[derive(Debug, Clone, Default)]
struct About {
    /// The kinds the two read it as ([`kind_number`]).
    kind: (u8, u8),
    /// What each read of its program ([`program_number`]).
    program: (u8, u8),
    /// What each read of its program when programs are read, whichever
    /// way this reading is.
    embeds: (u8, u8),
    /// Whether each guessed its codes ([`FontApprox::codes`]).
    ///
    /// [`FontApprox::codes`]: super::FontApprox
    codes_guessed: (bool, bool),
    subtype: String,
    base: String,
    encoding: String,
}

impl fmt::Display for About {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let About {
            kind,
            program,
            embeds: _,
            codes_guessed: _,
            subtype,
            base,
            encoding,
        } = self;
        write!(
            f,
            "kind={}/{} program={}/{} /{subtype} /{base} {encoding}",
            kind.0, kind.1, program.0, program.1
        )
    }
}

/// A number by its bits: equal only where they're the same, and shown
/// with its value.
#[derive(Clone, Copy, PartialEq)]
struct Bits(u32);

impl Debug for Bits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{:08x}]", f32::from_bits(self.0), self.0)
    }
}

fn bits(v: f32) -> Bits {
    Bits(v.to_bits())
}

fn old_box(r: Rect) -> [Bits; 4] {
    [r.x0, r.y0, r.x1, r.y1].map(bits)
}

fn new_box(b: new::GlyphBox) -> [Bits; 4] {
    [b.x0, b.y0, b.x1, b.y1].map(bits)
}

/// A font's kind as a number, the same for both readers' types; one this
/// has no number for is 255, so it shows.
fn kind_number(kind: FontKind) -> u8 {
    match kind {
        FontKind::Type1 => 0,
        FontKind::MMType1 => 1,
        FontKind::TrueType => 2,
        FontKind::Type3 => 3,
        FontKind::Type0(Descendant::CidFontType0) => 4,
        FontKind::Type0(Descendant::CidFontType2) => 5,
    }
}

fn new_kind_number(kind: new::FontKind) -> u8 {
    match kind {
        new::FontKind::Type1 => 0,
        new::FontKind::MMType1 => 1,
        new::FontKind::TrueType => 2,
        new::FontKind::Type3 => 3,
        new::FontKind::Type0(new::Descendant::CidFontType0) => 4,
        new::FontKind::Type0(new::Descendant::CidFontType2) => 5,
        _ => 255,
    }
}

/// What was read of a font's program as a number: none 0, read 1,
/// malformed 2, and the PDF layer's "past a cap" 3, which sheaf's reader
/// hasn't.
fn program_number(program: FontProgram) -> u8 {
    match program {
        FontProgram::None => 0,
        FontProgram::Read => 1,
        FontProgram::Malformed => 2,
    }
}

fn new_program_number(program: new::FontProgram) -> u8 {
    match program {
        new::FontProgram::None => 0,
        new::FontProgram::Read => 1,
        new::FontProgram::Malformed => 2,
        new::FontProgram::Capped => 3,
        _ => 255,
    }
}

/// What an observable is asked of.
#[derive(Clone, Copy)]
enum Of<'s> {
    Font,
    /// A code, and how many bytes of its string it was.
    Code(u32, usize),
    String(&'s [u8]),
}

impl fmt::Display for Of<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Of::Font => write!(f, "-"),
            Of::Code(code, len) => write!(f, "{code:#06x}/{len}"),
            Of::String(s) => {
                write!(f, "<")?;
                for b in s.iter().take(48) {
                    write!(f, "{b:02x}")?;
                }
                match s.len() > 48 {
                    true => write!(f, "…{}>", s.len()),
                    false => write!(f, ">"),
                }
            }
        }
    }
}

/// One font read one way, its two readings being compared: the rows of
/// where they part.
struct Diff<'r> {
    font: &'r str,
    mode: &'static str,
    about: About,
    /// The codes pages show in it, and whether they show any string.
    shown: HashSet<u32>,
    any_shown: bool,
    /// This font's rows, and how many times each observable differed, in
    /// all and where a page shows it.
    rows: Vec<Row>,
    counts: BTreeMap<&'static str, (usize, usize)>,
    /// How many answers were set beside each other.
    asked: usize,
}

impl Diff<'_> {
    fn check<T: PartialEq + Debug>(&mut self, observable: &'static str, of: Of, old: T, new: T) {
        self.asked += 1;
        if old == new {
            return;
        }
        let shown = match of {
            Of::Font => self.any_shown,
            Of::Code(code, _) => self.shown.contains(&code),
            Of::String(_) => true,
        };
        let (n, n_shown) = self.counts.entry(observable).or_default();
        *n += 1;
        *n_shown += usize::from(shown);
        // The first few, and the first few a page shows.
        if *n > ROWS && !(shown && *n_shown <= ROWS) {
            return;
        }
        let (old, new) = (format!("{old:?}"), format!("{new:?}"));
        self.rows.push(Row {
            class: "",
            font: self.font.to_string(),
            mode: self.mode,
            observable,
            code: of.to_string(),
            old,
            new,
            shown,
            n: 0,
            n_shown: 0,
            about: String::new(),
        });
    }

    /// Its rows, each with its count, its font and its class.
    fn finish(self, into: &mut Vec<Row>) -> usize {
        let about = self.about.to_string().replace(['\t', '\n', '\r'], " ");
        for mut row in self.rows {
            (row.n, row.n_shown) = self.counts[row.observable];
            row.class = class(&row, &self.about);
            row.about = about.clone();
            into.push(row);
        }
        self.asked
    }
}

/// `object`, its references followed.
fn direct<'a, R: ReadObjects>(objects: &'a R, object: &'a Object) -> Option<&'a Object> {
    match object.as_reference() {
        Some(id) => objects.peek(id),
        None => Some(object),
    }
}

/// A font as resources name it.
struct Named<'a> {
    written: &'a Object,
    label: String,
}

/// Every font the resources `roots` name, and those of the forms they
/// name, and of theirs; then each Type 0 font's CIDFonts, as fonts of
/// their own. A font by reference once; one written in its resources
/// once as it's written.
fn named_fonts<'a, R: ReadObjects>(
    objects: &'a R,
    roots: impl IntoIterator<Item = &'a Object>,
) -> Vec<Named<'a>> {
    let mut found: Vec<Named<'a>> = Vec::new();
    let (mut by_id, mut inline) = (HashSet::new(), HashSet::new());
    let mut name = |written: &'a Object, name: &[u8], descendant: bool| {
        let label = match written.as_reference() {
            Some(id) if !by_id.insert(id) => return None,
            Some(id) => format!("{} {} R", id.number, id.generation),
            None if !inline.insert(format!("{written:?}")) => return None,
            None => format!("/{} written in resources", String::from_utf8_lossy(name)),
        };
        let label = match descendant {
            true => format!("{label} (descendant)"),
            false => label,
        };
        Some(Named { written, label })
    };
    let (mut resources, mut forms) = (HashSet::new(), HashSet::new());
    let mut stack: Vec<&'a Object> = roots.into_iter().collect();
    stack.reverse();
    while let Some(written) = stack.pop() {
        if written
            .as_reference()
            .is_some_and(|id| !resources.insert(id))
        {
            continue;
        }
        let Some(dict) = direct(objects, written).and_then(Object::as_dict) else {
            continue;
        };
        let of = |key: &[u8]| {
            let entry = dict.get(key).and_then(|o| direct(objects, o));
            entry.and_then(Object::as_dict)
        };
        for (key, font) in of(b"Font").into_iter().flat_map(Dictionary::iter) {
            found.extend(name(font, key, false));
        }
        for (_, xobject) in of(b"XObject").into_iter().flat_map(Dictionary::iter) {
            let Some(id) = xobject.as_reference().filter(|&id| forms.insert(id)) else {
                continue;
            };
            let form = objects.peek(id).and_then(Object::as_stream);
            let form =
                form.filter(|f| f.dict.get(b"Subtype").and_then(Object::as_name) == Some(b"Form"));
            if let Some(resources) = form.and_then(|f| f.dict.get(b"Resources")) {
                stack.push(resources);
            }
        }
    }
    let mut at = 0;
    while at < found.len() {
        let font = direct(objects, found[at].written).and_then(Object::as_dict);
        let descendants = font
            .and_then(|f| direct(objects, f.get(b"DescendantFonts")?))
            .and_then(Object::as_array);
        for descendant in descendants.into_iter().flatten() {
            found.extend(name(descendant, b"DescendantFonts", true));
        }
        at += 1;
    }
    found
}

/// The four sets of fonts a file's are read by: each reader's, with
/// programs and for text.
struct Readers {
    old: Fonts,
    old_text: Fonts,
    new: new::Fonts,
    new_text: new::Fonts,
}

impl Readers {
    fn new() -> Readers {
        Readers {
            old: Fonts::default(),
            old_text: Fonts::for_text(),
            new: new::Fonts::default(),
            new_text: new::Fonts::for_text(),
        }
    }
}

fn name_of<R: ReadObjects>(objects: &R, dict: Option<&Dictionary>, key: &[u8]) -> String {
    let Some(entry) = dict.and_then(|d| d.get(key)) else {
        return "-".into();
    };
    match direct(objects, entry) {
        Some(o) if o.as_name().is_some() => String::from_utf8_lossy(o.as_name().unwrap()).into(),
        Some(o) if o.as_stream().is_some() => "(stream)".into(),
        Some(o) if o.as_dict().is_some() => "(dictionary)".into(),
        Some(_) => "(other)".into(),
        None => "(unread)".into(),
    }
}

/// `font` read by both readers, both ways, and each answer of the one
/// set beside the other's: where they part goes into `rows`. `strings`
/// are those shown in it. How many answers were compared.
fn compare<'a, R: ReadObjects>(
    objects: &'a R,
    font: &Named<'a>,
    strings: Option<&BTreeSet<Vec<u8>>>,
    readers: &Readers,
    rows: &mut Vec<Row>,
) -> usize {
    let dict = direct(objects, font.written).and_then(Object::as_dict);
    let mut about = About {
        subtype: name_of(objects, dict, b"Subtype"),
        base: name_of(objects, dict, b"BaseFont"),
        encoding: name_of(objects, dict, b"Encoding"),
        ..About::default()
    };
    let mut asked = 0;
    let modes = [
        ("programs", &readers.old, &readers.new),
        ("text", &readers.old_text, &readers.new_text),
    ];
    for (mode, old_fonts, new_fonts) in modes {
        let (mut old_left, mut new_left) = (LIM.content.decompressed, LIM.content.decompressed);
        let old = old_fonts.get(objects, font.written, LIM, &mut old_left);
        let new = new_fonts.get(Objects(objects), font.written, &LIM.fonts(), &mut new_left);
        let mut diff = Diff {
            font: &font.label,
            mode,
            about: about.clone(),
            shown: HashSet::new(),
            any_shown: strings.is_some_and(|s| !s.is_empty()),
            rows: Vec::new(),
            counts: BTreeMap::new(),
            asked: 0,
        };
        let said = |font: bool, error: Option<String>| match (font, error) {
            (_, Some(error)) => error,
            (true, None) => "a font".to_string(),
            (false, None) => "none".to_string(),
        };
        let old_said = match &old {
            Ok(font) => said(font.is_some(), None),
            Err(_) => said(false, Some("too large".into())),
        };
        let new_said = match &new {
            Ok(font) => said(font.is_some(), None),
            Err(new::Error::TooLarge) => said(false, Some("too large".into())),
            Err(error) => said(false, Some(format!("unreadable: {error}"))),
        };
        diff.check("result", Of::Font, old_said, new_said);
        diff.check("budget left", Of::Font, old_left, new_left);
        if let (Ok(Some(old)), Ok(Some(new))) = (&old, &new) {
            diff.about.kind = (kind_number(old.kind()), new_kind_number(new.kind()));
            diff.about.program = (
                program_number(old.program()),
                new_program_number(new.program()),
            );
            diff.about.codes_guessed = (old.approx().codes, new.approx().codes);
            if mode == "programs" {
                diff.about.embeds = diff.about.program;
            }
            about = diff.about.clone();
            answers(&mut diff, old, new, strings);
        }
        asked += diff.finish(rows);
    }
    asked
}

/// Every answer of `old` beside `new`'s.
fn answers(diff: &mut Diff, old: &Foreign, new: &new::Font, strings: Option<&BTreeSet<Vec<u8>>>) {
    let composite = matches!(old.kind(), FontKind::Type0(_));
    for s in strings.into_iter().flatten() {
        match composite {
            false => diff.shown.extend(s.iter().map(|&b| u32::from(b))),
            true => {
                diff.shown.extend(old.codes(s).map(|(_, code)| code));
                diff.shown.extend(new.codes(s).map(|(_, code)| code));
            }
        }
    }
    diff.check("id", Of::Font, old.id(), new.id());
    diff.check("kind", Of::Font, diff.about.kind.0, diff.about.kind.1);
    diff.check(
        "program",
        Of::Font,
        diff.about.program.0,
        diff.about.program.1,
    );
    diff.check(
        "is_vertical",
        Of::Font,
        old.is_vertical(),
        new.is_vertical(),
    );
    let (was, now) = (old.approx(), new.approx());
    diff.check("approx.codes", Of::Font, was.codes, now.codes);
    diff.check("approx.widths", Of::Font, was.widths, now.widths);
    diff.check("approx.heights", Of::Font, was.heights, now.heights);
    diff.check("approx.boxes", Of::Font, was.boxes, now.boxes);
    diff.check("outlined", Of::Font, old.outlined(), new.outlined());
    let (was, now) = (old.metrics(), new.metrics());
    diff.check(
        "metrics.ascent",
        Of::Font,
        bits(was.ascent),
        bits(now.ascent),
    );
    diff.check(
        "metrics.descent",
        Of::Font,
        bits(was.descent),
        bits(now.descent),
    );
    diff.check(
        "metrics.bbox",
        Of::Font,
        was.bbox.map(old_box),
        now.bbox.map(new_box),
    );
    diff.check("extent", Of::Font, bits(old.extent()), bits(new.extent()));
    let (was, now) = (old.byte_widths(), new.byte_widths());
    diff.check("byte_widths", Of::Font, was.is_some(), now.is_some());
    if let (Some(was), Some(now)) = (was, now) {
        for code in 0..256 {
            let of = Of::Code(code as u32, 1);
            diff.check("byte_widths", of, bits(was[code]), bits(now[code]));
        }
    }

    // The codes asked of: a simple font's 256; a Type 0 font's that its
    // strings show, as either reader splits them, and a spread of two
    // bytes.
    let split = |diff: &mut Diff, s: &[u8], codes: &mut BTreeSet<(u32, usize)>| {
        let was: Vec<(Range<usize>, u32)> = old.codes(s).collect();
        let now: Vec<(Range<usize>, u32)> = new.codes(s).collect();
        codes.extend(was.iter().map(|(at, code)| (*code, at.len())));
        if was != now {
            codes.extend(now.iter().map(|(at, code)| (*code, at.len())));
        }
        diff.check("codes", Of::String(s), was, now);
    };
    let mut codes = BTreeSet::new();
    match composite {
        false => {
            for code in 0..=255u8 {
                split(diff, &[code], &mut codes);
            }
        }
        true => {
            for i in 0..SPREAD {
                let code = (i * 16 + (i * 7) % 16) as u16;
                split(diff, &code.to_be_bytes(), &mut codes);
            }
        }
    }
    for s in strings.into_iter().flatten() {
        let of = Of::String(s);
        match composite {
            true => split(diff, s, &mut codes),
            // A byte each: split once already, each byte alone.
            false => {
                let was = old.codes(s).count();
                diff.check("codes", of, was, new.codes(s).count());
            }
        }
        diff.check("text", of, old.text(s), new.text(s));
        diff.check(
            "width_guessed_in",
            of,
            old.width_guessed_in(s),
            new.width_guessed_in(s),
        );
        // What drawing its glyphs costs, each charged.
        let (mut old_left, mut new_left) = (usize::MAX, usize::MAX);
        let was = old.charge_outlines(s, |_| true, &mut old_left).is_ok();
        let now = new.charge_outlines(s, |_| true, &mut new_left).is_ok();
        diff.check(
            "charge_outlines",
            of,
            (was, usize::MAX - old_left),
            (now, usize::MAX - new_left),
        );
    }
    for (code, len) in codes {
        let of = Of::Code(code, len);
        diff.check("cid", of, old.cid(code), new.cid(code));
        diff.check("width", of, bits(old.width(code)), bits(new.width(code)));
        diff.check(
            "width_guessed",
            of,
            old.width_guessed(code),
            new.width_guessed(code),
        );
        diff.check(
            "word_spaced",
            of,
            old.word_spaced(code, len),
            new.word_spaced(code, len),
        );
        diff.check(
            "vertical",
            of,
            old.vertical(code).map(|v| [v.w1, v.vx, v.vy].map(bits)),
            new.vertical(code).map(|v| [v.w1, v.vx, v.vy].map(bits)),
        );
        diff.check(
            "glyph_box",
            of,
            old.glyph_box(code).map(old_box),
            new.glyph_box(code).map(new_box),
        );
        diff.check(
            "char_box",
            of,
            old.char_box(code).map(old_box),
            new.char_box(code).map(new_box),
        );
        diff.check(
            "code_text",
            of,
            old.code_text(code).map(Cow::into_owned),
            new.code_text(code).map(Cow::into_owned),
        );
    }
}

/// The strings `content` shows, its forms' among them, by the font each
/// is shown in, where that's an object: read with sheaf's reader, as far
/// as the content interprets.
fn shown<R: ReadObjects>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    fonts: &Fonts,
    into: &mut HashMap<ObjectId, BTreeSet<Vec<u8>>>,
) {
    let _ = interpret(
        objects,
        content,
        resources,
        LIM,
        fonts,
        Opts::default(),
        |event| {
            if let Kind::Text(show) = event.kind
                && let Some(id) = show.font.and_then(Foreign::id)
            {
                let strings = into.entry(id).or_default();
                if !strings.contains(show.string) {
                    strings.insert(show.string.to_vec());
                }
            }
            ControlFlow::Continue(())
        },
    );
}

/// What one file's fonts came to.
#[derive(Default)]
struct Outcome {
    rows: Vec<Row>,
    fonts: usize,
    strings: usize,
    asked: usize,
    /// Why it wasn't read, where it wasn't.
    unread: Option<String>,
}

/// The fonts `roots` name, compared, with the strings `strings` has for
/// each.
fn compare_all<'a, R: ReadObjects>(
    objects: &'a R,
    roots: impl IntoIterator<Item = &'a Object>,
    strings: &HashMap<ObjectId, BTreeSet<Vec<u8>>>,
    readers: &Readers,
    out: &mut Outcome,
) {
    for font in named_fonts(objects, roots) {
        let strings = font.written.as_reference().and_then(|id| strings.get(&id));
        out.fonts += 1;
        out.strings += strings.map_or(0, BTreeSet::len);
        out.asked += compare(objects, &font, strings, readers, &mut out.rows);
    }
}

/// The corpus file at `path`: every font its pages' resources name, with
/// the strings its pages show in each.
fn compare_file(corpus: &Corpus, path: &str) -> Outcome {
    let mut out = Outcome::default();
    let bytes = match std::fs::read(corpus.root.join(path)) {
        Ok(bytes) => bytes,
        Err(e) => {
            out.unread = Some(format!("unopened: {e}"));
            return out;
        }
    };
    let kind = BlobKind::file(Path::new(path), None);
    let src = match Store::new().open(bytes.into(), kind) {
        Ok(src) => src,
        Err(e) => {
            out.unread = Some(format!("unopened: {e:#}").replace(['\n', '\t'], " "));
            return out;
        }
    };
    let readers = Readers::new();
    let mut strings = HashMap::new();
    let pages = src.pages();
    for page in pages.iter() {
        if let Ok(content) = PageContent::read(src.pdf(), &page.contents, LIM) {
            let resources = page.resources.as_deref();
            shown(
                src.pdf(),
                content.concat(),
                resources,
                &readers.old,
                &mut strings,
            );
        }
    }
    let roots = pages.iter().filter_map(|page| page.resources.as_deref());
    compare_all(src.pdf(), roots, &strings, &readers, &mut out);
    out
}

/// The fonts of every file the glyph dump reads (every form, the pool's
/// sample, every scan, the PDF/UA files), each read by both readers:
/// where they part is a row of the file at the path in [`DIFF`], and a
/// line of each class here. It fails on a row of no class.
#[test]
#[ignore = "both font readers over the corpus, their differences written to $SHEAF_FONT_DIFF"]
fn the_two_readers_agree_on_every_font() {
    let to = std::env::var_os(DIFF);
    let only = std::env::var(ONLY).ok();
    if to.is_none() && only.is_none() {
        eprintln!("skipped: neither {DIFF} nor {ONLY} is set");
        return;
    }
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let mut picks = crate::content::corpus::dump_picks(&corpus);
    eprintln!("{} files", picks.len());
    if let Some(only) = &only {
        picks.retain(|path, _| path == only);
        assert_eq!(picks.len(), 1, "{ONLY} names no file this reads");
    }
    let paths: Vec<&String> = picks.keys().collect();
    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let outcomes: Mutex<BTreeMap<usize, Outcome>> = Mutex::new(BTreeMap::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(16));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(i) else {
                        break;
                    };
                    let read = || compare_file(&corpus, path);
                    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(read));
                    let outcome = read.unwrap_or_else(|panic| {
                        let said = (panic.downcast_ref::<&str>().map(|s| s.to_string()))
                            .or_else(|| panic.downcast_ref::<String>().cloned());
                        let said = said.unwrap_or_else(|| "a panic".into());
                        Outcome {
                            unread: Some(format!("panicked: {said}").replace(['\n', '\t'], " ")),
                            ..Outcome::default()
                        }
                    });
                    outcomes.lock().unwrap().insert(i, outcome);
                }
            });
        }
    });
    let outcomes = outcomes.into_inner().unwrap();

    let mut lines = vec![HEADER.to_string()];
    let (mut fonts, mut strings, mut asked) = (0, 0, 0);
    // By class and observable: rows, differences, fonts, files, and the
    // first file.
    type Tally = (
        usize,
        usize,
        usize,
        BTreeSet<(usize, String)>,
        BTreeSet<usize>,
    );
    let mut classes: BTreeMap<(&str, &str, &str), Tally> = BTreeMap::new();
    let mut unread = Vec::new();
    for (i, outcome) in &outcomes {
        let path = paths[*i];
        fonts += outcome.fonts;
        strings += outcome.strings;
        asked += outcome.asked;
        if let Some(why) = &outcome.unread {
            unread.push(format!("{path}: {why}"));
        }
        for row in &outcome.rows {
            lines.push(row.tsv(path));
            let key = (row.class, row.observable, row.mode);
            let tally = classes.entry(key).or_default();
            tally.0 += 1;
            if tally.3.insert((*i, row.font.clone())) {
                tally.1 += row.n;
                tally.2 += row.n_shown;
            }
            tally.4.insert(*i);
        }
    }
    eprintln!(
        "{fonts} fonts of {} files, {strings} strings shown, {asked} answers compared, \
         in {:.0} s: {} rows",
        outcomes.len(),
        started.elapsed().as_secs_f64(),
        lines.len() - 1,
    );
    for why in &unread {
        eprintln!("  not read: {why}");
    }
    for ((class, observable, mode), (rows, n, shown, fonts, files)) in &classes {
        let first = files.first().map_or("", |&i| paths[i].as_str());
        eprintln!(
            "  {class}\t{observable}\t{mode}\t{rows} rows, {n} differences, {shown} of them \
             shown, {} fonts, {} files, first {first}",
            fonts.len(),
            files.len(),
        );
    }
    if let Some(to) = to {
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(&to, text).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
    }
    assert!(
        unread.iter().all(|why| why.contains(": unopened: ")),
        "a file panicked"
    );
    let unclassed = classes.keys().filter(|key| key.0 == "unclassed").count();
    assert_eq!(unclassed, 0, "differences of no class");
}

/// The rows of the fixtures built in code ([`fixture::every`] and the
/// fuzz target's page): each font their pages and forms name, with the
/// strings they show.
fn fixture_rows() -> Vec<(String, Row)> {
    let mut all = Vec::new();
    let mut fixtures = fixture::every();
    fixtures.push(("fuzz page", fixture::fuzz_page()));
    for (name, f) in &fixtures {
        let readers = Readers::new();
        let mut strings = HashMap::new();
        let content = f.content();
        shown(
            &f.pdf,
            content.concat(),
            f.resources(),
            &readers.old,
            &mut strings,
        );
        let mut out = Outcome::default();
        compare_all(&f.pdf, f.resources(), &strings, &readers, &mut out);
        assert!(out.fonts > 0 || !name.contains("font"), "{name}: no font");
        all.extend(out.rows.into_iter().map(|row| (name.to_string(), row)));
    }
    all
}

/// The classes the fixtures' fonts differ in, each with its fixture, and
/// whether the fixture's page shows a code or a string it differs at.
const FIXTURES: [&str; 23] = [
    "D5 in clips",
    "D5 in font programs",
    "D5 in fonts",
    "D5 in fuzz page",
    "M1 in fuzz page",
    "M1 in predefined CMaps",
    "N1 in fonts",
    "N1 in fuzz page",
    "T3 in chain at form_depth",
    "T3 in chain past form_depth",
    "T3 in clips",
    "T3 in cycles",
    "T3 in fan out",
    "T3 in fonts",
    "T3 in forms",
    "T3 in fuzz page",
    "T3 in inherited",
    "T3 in marked",
    "T3 in operators",
    "X0 in font programs",
    "X0 in fonts",
    "X0 in fuzz page",
    "X0 in predefined CMaps",
];

/// The fixtures' fonts, read by both readers, part only in ways that
/// have a class, and in these: each class, in the fixtures it's in.
#[test]
fn the_two_readers_agree_on_the_fixtures() {
    let rows = fixture_rows();
    let lines = |class: &str| -> Vec<String> {
        let of = rows.iter().filter(|(_, row)| row.class == class);
        of.map(|(name, row)| row.tsv(name)).collect()
    };
    assert!(
        lines("unclassed").is_empty(),
        "{}",
        lines("unclassed").join("\n")
    );
    let mut found: BTreeMap<(&str, &str), bool> = BTreeMap::new();
    for (name, row) in &rows {
        *found.entry((row.class, name.as_str())).or_default() |= row.shown;
    }
    let found: Vec<String> = (found.iter())
        .map(|((class, name), shown)| match shown {
            true => format!("{class} in {name}, shown"),
            false => format!("{class} in {name}"),
        })
        .collect();
    let all: Vec<String> = rows.iter().map(|(name, row)| row.tsv(name)).collect();
    assert_eq!(found, FIXTURES, "{}", all.join("\n"));
}

/// `font`, a font dictionary the tests beside this build over `pdf`'s
/// objects, and its CIDFonts, read by both readers: its rows.
pub(super) fn rows_of(pdf: &Pdf, font: &Dictionary) -> Vec<Row> {
    let resources = Object::from(crate::pdf::dictionary! {
        "Font" => crate::pdf::dictionary! { "F" => font.clone() },
    });
    let mut out = Outcome::default();
    compare_all(
        pdf,
        [&resources],
        &HashMap::new(),
        &Readers::new(),
        &mut out,
    );
    out.rows
}

/// `font` compared ([`rows_of`]): where the two readers part on it has a
/// class.
pub(super) fn compared(pdf: &Pdf, font: &Dictionary) {
    let rows = rows_of(pdf, font);
    let unclassed = rows.iter().filter(|row| row.class == "unclassed");
    let lines: Vec<String> = unclassed.map(|row| row.tsv("a test's font")).collect();
    assert!(lines.is_empty(), "{}", lines.join("\n"));
}
