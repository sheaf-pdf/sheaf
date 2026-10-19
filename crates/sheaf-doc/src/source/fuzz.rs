//! The bodies of the fuzz targets in `fuzz/` (cargo-fuzz), here so a
//! stable test runs them too. Each reads what it's given, and must
//! neither panic nor hang whatever the bytes. `op_json`'s document is
//! built from the form fixtures, so it needs the `fixtures` feature.

use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use lopdf::{LazyPdf, Object, Stream, dictionary};

use super::Limits;
use crate::content::{
    self, Concat, DrawnForms, PageBlocks, PageContent, PropertiesDict, ResCx, SpanAcross,
};
use crate::store::{BlobKind, Src, Store};

/// `data` read lazily: every object the cross-reference table lists, and
/// where each is in the bytes.
pub fn lazy_reader(data: &[u8]) {
    let Ok(pdf) = LazyPdf::open(Arc::<[u8]>::from(data), None, &Limits::DEFAULT.reader) else {
        return;
    };
    let _ = pdf.catalog();
    for id in pdf.ids() {
        let _ = pdf.get(id);
        let _ = pdf.raw_span(id);
    }
}

/// `data` interned and opened as a source: its page table, every page's
/// size, and its conformance claims.
pub fn page_tree(data: &[u8]) {
    let kind = BlobKind::file(Path::new("fuzzed.pdf"), None);
    let Ok(src) = Store::new().open(data.into(), kind) else {
        return;
    };
    for i in 0..src.page_count() {
        let _ = src.page_size(i);
    }
    let _ = src.conformance();
}

/// `data` as the objects of a page's annotations, `n 0 obj … endobj` from
/// 4 up, read as the open reads sheaf's marks: each entry's `/SheafAnnot`
/// and digest, or its geometry from the standard keys, and whether it's
/// ours to rewrite. The page, turned 90°, lists an annotation for each
/// `endobj`, in a file with no cross-reference table, which the reader
/// rebuilds by scanning.
pub fn sheaf_annot(data: &[u8]) {
    let Some(src) = annotated_page(data) else {
        return;
    };
    let (pdf, Some(page)) = (src.pdf(), src.page(0)) else {
        return;
    };
    let annots = crate::file::PageAnnots::new(&page.annots);
    for entry in page.annots.iter() {
        let _ = crate::file::sheaf_mark(pdf, entry, page.geom.rotate, &annots);
        let _ = crate::file::is_ours(pdf, entry, &annots);
        if let Ok((_, lopdf::Object::Dictionary(d))) = pdf.dereference(entry) {
            let _ = crate::annot::staleness(pdf, d, false);
        }
    }
}

/// `data` as a page's content, lexed under the default caps, every token
/// taken: whole, and as two streams split at its middle. The resources
/// name a color space of each size an inline image may be drawn in.
pub fn lexer(data: &[u8]) {
    let res = ResCx::new([
        (b"CS1".to_vec(), 1),
        (b"CS3".to_vec(), 3),
        (b"CS4".to_vec(), 4),
    ]);
    let (first, second) = data.split_at(data.len() / 2);
    let split = Concat::new(&[first, second]);
    for content in [Concat::new(&[data]), split] {
        for token in content.lex(&res, &Limits::DEFAULT) {
            let Ok((range, _)) = token else { break };
            let _ = content.span(range);
        }
    }
}

/// `data` as a page's content, two streams split at its middle, drawn
/// with resources that name a property list, a color space, a form of
/// Acrobat's and one with an artifact in it: its marked ranges, names and
/// blocks, and a splice of each range at the top, of all of them at once
/// widened to touch ([`touching`]), and of its pagination artifacts as a
/// strip cuts them. Where a splice is made, what it keeps lexes as the
/// tokens it kept. Returns how many were made.
pub fn splice(data: &[u8]) -> usize {
    let lim = &Limits::DEFAULT;
    let mut pdf = lopdf::Document::with_version("1.7");
    let (first, second) = data.split_at(data.len() / 2);
    let streams = [first, second].map(|s| pdf.add_object(Stream::new(dictionary! {}, s.to_vec())));
    let piece = dictionary! { "ADBE_CompoundType" => dictionary! { "Private" => "Header" } };
    let acrobat = pdf.add_object(Stream::new(
        dictionary! { "Subtype" => "Form", "PieceInfo" => piece },
        b"BT (h) Tj ET".to_vec(),
    ));
    let nested = pdf.add_object(Stream::new(
        dictionary! { "Subtype" => "Form" },
        b"/Artifact <</Subtype /Footer>> BDC EMC".to_vec(),
    ));
    let resources = Object::Dictionary(dictionary! {
        "Properties" => dictionary! { "P0" => dictionary! {
            "Type" => "Pagination", "Subtype" => "Header", "SheafGen" => Object::string_literal("g1"),
        } },
        "XObject" => dictionary! { "X0" => acrobat, "X1" => nested },
        "ColorSpace" => dictionary! { "CS3" => "DeviceRGB" },
    });
    let Ok(content) = PageContent::read(&pdf, &streams, lim) else {
        return 0;
    };
    let concat = content.concat();
    let res = ResCx::read(&pdf, Some(&resources));
    let _ = content::balance(concat, &res, lim);
    let _ = content::names_used(concat, &res, lim, &[]);
    let props = PropertiesDict::read(&pdf, Some(&resources));
    let mut drops: Vec<Vec<SpanAcross>> = Vec::new();
    if let Ok(found) = content::marked(concat, &props, &res, lim) {
        let top: Vec<SpanAcross> = found
            .iter()
            .filter(|m| m.depth == 0)
            .map(|m| m.span)
            .collect();
        drops.extend(top.iter().map(|&span| vec![span]));
        if top.len() > 1 {
            drops.push(touching(concat, &top));
        }
    }
    let known = DrawnForms::default();
    if let Ok(blocks) = PageBlocks::of(&pdf, &content, Some(&resources), lim, &known)
        && let Ok(cuts) = blocks
            .pagination
            .iter()
            .map(|p| p.cuts(concat, &res, lim))
            .collect::<Result<Vec<_>, _>>()
        && !cuts.is_empty()
    {
        drops.push(cuts.concat());
    }
    let mut made = 0;
    for drop in drops {
        if let Ok(kept) = content::splice(concat, &res, lim, &drop) {
            assert_kept(concat, &res, &kept, &drop);
            made += 1;
        }
    }
    made
}

/// `data` as Op JSON (M10.5 §6.1): an envelope that reads is applied by
/// the user to a small document ([`OpJson`]): its ops in one batch, or
/// one by one when the batch fails, the files the fixture's ops name
/// granted, then committed, undone and redone. Whatever the ops say, each
/// applies or fails with an error, and what's committed keeps the state's
/// rules ([`DocState::check_invariants`]), undone and redone too. Returns
/// how many ops applied.
///
/// [`DocState::check_invariants`]: crate::model::DocState::check_invariants
#[cfg(any(test, feature = "fixtures"))]
pub fn op_json(data: &[u8]) -> usize {
    let Ok(envelope) = serde_json::from_slice::<sheaf_edit::Envelope>(data) else {
        return 0;
    };
    OP_JSON.with(|doc| doc.apply(envelope.ops))
}

#[cfg(any(test, feature = "fixtures"))]
thread_local! {
    static OP_JSON: OpJson = OpJson::new();
}

/// [`op_json`]'s document: four pages, a form with every kind of field on
/// the first two, sheaf's marks of every kind on the third, and on the
/// fourth another tool's comment with a reply; bookmarks, roman labels on
/// the first two pages and a header on the last two. The names the Op
/// JSON fixture's references use are there (`Part I › Ch. 2`, `Appendix`,
/// `a1b2-c3`, `acro-1`, `sheaf:12`). Read once, opened afresh each time,
/// with the files its ops are granted: a stamp's page, an attachment's
/// file and a file of two pages as assets 0, 1 and 2, and one of a page
/// as `logo`.
#[cfg(any(test, feature = "fixtures"))]
struct OpJson {
    store: Arc<Store>,
    primary: Src,
    assets: Vec<crate::store::Blob>,
    logo: crate::store::Blob,
}

#[cfg(any(test, feature = "fixtures"))]
impl OpJson {
    fn new() -> OpJson {
        use crate::forms::fixture::{
            Form, MARK_FILE_BYTES, every_kind_in, every_mark_kind, holding, mark_stamp_pdf,
            outline_tree, write_marks,
        };
        use crate::model::PageId;
        let mut f = every_kind_in(4);
        let pages = f.pages.clone();
        let mut third = f.dict(pages[2]).clone();
        write_marks(&mut f.pdf, &mut third, &holding(&every_mark_kind()));
        *f.dict(pages[2]) = third;
        let rect = || Object::Array([72, 600, 92, 620].map(Object::from).to_vec());
        let comment = f.pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
            "NM" => Object::string_literal("a1b2-c3"), "Contents" => Object::string_literal("Why?"),
        });
        let reply = f.pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(), "IRT" => comment,
            "NM" => Object::string_literal("acro-1"), "Contents" => Object::string_literal("Because."),
        });
        f.set_annots(3, &[comment, reply]);
        let item = |title: &str, page| {
            dictionary! {
                "Title" => Object::string_literal(title),
                "Dest" => vec![Object::Reference(page), "Fit".into()],
            }
        };
        outline_tree(
            &mut f,
            vec![
                (0, item("Part I", pages[0])),
                (1, item("Ch. 1", pages[0])),
                (1, item("Ch. 2", pages[1])),
                (0, item("Appendix", pages[3])),
            ],
        );
        let catalog = f.catalog;
        let labels = vec![
            0.into(),
            dictionary! { "S" => "r" }.into(),
            2.into(),
            dictionary! { "S" => "D" }.into(),
        ];
        f.dict(catalog)
            .set("PageLabels", dictionary! { "Nums" => labels });
        let mut texts: [String; 6] = Default::default();
        texts[0] = "Draft <<1>>".into();
        let hf = crate::stamps::HeaderFooter {
            texts,
            pages: Some(vec![PageId(2), PageId(3)]),
            ..Default::default()
        };
        let ids: Vec<PageId> = (0..4).map(PageId).collect();
        let scope = crate::stamps::to_catalog(&hf, &ids);
        f.dict(catalog).set(crate::stamps::CATALOG_KEY, scope);
        let store = Store::new();
        let kind = BlobKind::file(Path::new("form.pdf"), None);
        let primary = store
            .open(f.bytes().into(), kind)
            .expect("the fixture opens");
        let intern = |bytes: Vec<u8>, kind| store.intern(bytes.into(), kind).expect("a blob");
        let scan = BlobKind::file(Path::new("scan.pdf"), None);
        let assets = vec![
            intern(mark_stamp_pdf(), BlobKind::stamp()),
            intern(MARK_FILE_BYTES.to_vec(), BlobKind::Attachment),
            intern(Form::new(2).bytes(), scan),
        ];
        let logo = intern(
            mark_stamp_pdf(),
            BlobKind::file(Path::new("logo.pdf"), None),
        );
        OpJson {
            store,
            primary,
            assets,
            logo,
        }
    }

    /// `ops` applied, as [`op_json`] says: how many applied.
    fn apply(&self, ops: Vec<sheaf_edit::Op>) -> usize {
        use crate::tx::{Actor, CommitOpts, Grant};
        use in_progress::NoProgress;
        let policy = Box::new(crate::history::FixedCount(4));
        let (store, name) = (self.store.clone(), "form.pdf".into());
        let mut d = crate::open::start(store, self.primary.clone(), name, None, policy);
        d.ready(crate::open::finish(&self.primary, &NoProgress))
            .expect("the fixture's facts");
        let mut tx = d.tx(Actor::User);
        for blob in &self.assets {
            tx.grant(Grant::Blob(blob.clone().into()));
        }
        tx.grant_named("logo", Grant::Blob(self.logo.clone().into()));
        let applied = match tx.apply_batch(ops.clone(), &NoProgress) {
            Ok(done) => done.len(),
            Err(_) => (ops.into_iter())
                .filter(|op| tx.apply(op.clone(), &NoProgress).is_ok())
                .count(),
        };
        let Ok(commit) = d.commit(tx, CommitOpts::default()) else {
            return applied;
        };
        let kept = |d: &crate::model::Document| assert_eq!(d.state().check_invariants(), Ok(()));
        kept(&d);
        if commit.version.is_some() {
            d.undo().expect("its version undone");
            kept(&d);
            d.redo().expect("and redone");
            kept(&d);
        }
        applied
    }
}

/// `spans`, in order and apart, each widened over the white-space either
/// side of it, white-space between two split at its middle: cuts that
/// touch, so whether one needs a space in its place depends on bytes its
/// neighbour takes.
fn touching(content: &Concat, spans: &[SpanAcross]) -> Vec<SpanAcross> {
    let s = content.bytes();
    let space = |c: &&u8| matches!(c, b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ');
    let ranges: Vec<Range<usize>> = spans
        .iter()
        .filter_map(|&span| content.offsets(span))
        .collect();
    let mut cuts: Vec<Range<usize>> = ranges
        .iter()
        .map(|r| {
            let before = s[..r.start].iter().rev().take_while(space).count();
            r.start - before..r.end + s[r.end..].iter().take_while(space).count()
        })
        .collect();
    for i in 1..cuts.len() {
        if cuts[i].start < cuts[i - 1].end {
            let middle = (ranges[i - 1].end + ranges[i].start) / 2;
            (cuts[i - 1].end, cuts[i].start) = (middle, middle);
        }
    }
    cuts.into_iter().map(|r| content.span(r)).collect()
}

/// That `kept`, `content` spliced without `drop`, lexes as the tokens of
/// `content` outside `drop`.
fn assert_kept(content: &Concat, res: &ResCx, kept: &[Cow<[u8]>], drop: &[SpanAcross]) {
    let lim = &Limits::DEFAULT;
    let drop: Vec<_> = drop.iter().filter_map(|&d| content.offsets(d)).collect();
    let before: Vec<&[u8]> = content
        .lex(res, lim)
        .map(|t| t.expect("a splice lexed it"))
        .filter(|(r, _)| !drop.iter().any(|d| d.start <= r.start && r.end <= d.end))
        .map(|(r, _)| &content.bytes()[r])
        .collect();
    let kept: Vec<&[u8]> = kept.iter().map(|s| &**s).collect();
    let kept = Concat::new(&kept);
    let after: Vec<&[u8]> = kept
        .lex(res, lim)
        .map(|t| t.expect("what a splice keeps lexes"))
        .map(|(r, _)| &kept.bytes()[r])
        .collect();
    assert_eq!(before, after, "the tokens a splice kept");
}

/// [`sheaf_annot`]'s file, opened as a source.
fn annotated_page(data: &[u8]) -> Option<Src> {
    let n = data.windows(6).filter(|w| w == b"endobj").count();
    let annots: String = (0..n).map(|i| format!("{} 0 R ", i + 4)).collect();
    let mut file = format!(
        "%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
         2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
         3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [-37 52 575 844] /Rotate 90 \
         /Annots [{annots}] >>\nendobj\n"
    )
    .into_bytes();
    file.extend_from_slice(data);
    file.extend_from_slice(b"\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n");
    let kind = BlobKind::file(Path::new("fuzzed.pdf"), None);
    Store::new().open(file.into(), kind).ok()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::time::{Duration, Instant};

    use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
    use lopdf::xref::XrefType;
    use lopdf::{EncryptionState, EncryptionVersion, Permissions};

    use lopdf::{Document as Pdf, Object, ObjectId};

    use super::*;
    use crate::forms::fixture::{Form, every_kind, every_mark_kind, hybrid_reference};
    use crate::serialize::fixture::encrypted;
    use crate::source::lazy_tests::{plaintext, stream, with_table};

    /// Mutations of each fixture.
    const CASES: usize = 600;

    /// What a mutation may write: the tokens files are made of, and
    /// numbers that are offsets, lengths and counts.
    const TOKENS: &[&[u8]] = &[
        b" 0 R ",
        b" 0 obj ",
        b"endobj",
        b"<<",
        b">>",
        b"[",
        b"]",
        b"(",
        b")",
        b"/Kids [2 0 R] ",
        b"/Parent 3 0 R ",
        b"/Type /Pages ",
        b"/Count 1048576 ",
        b"/MediaBox [0 0 1e30 -7] ",
        b"/Rotate -2147483648 ",
        b"/Length 4 0 R ",
        b"/Type /ObjStm /N 9 /First 0 ",
        b"/Filter /FlateDecode ",
        b"stream\n",
        b"endstream",
        b"xref\n0 3\n",
        b"trailer",
        b"startxref\n0\n",
        b"%%EOF",
        b"4294967296",
        b"-1",
    ];

    /// What a mutation of content may write: its tokens, and the bytes
    /// that end an inline image's data.
    const CONTENT_TOKENS: &[&[u8]] = &[
        b" ",
        b"(",
        b")",
        b"\\",
        b"<",
        b">",
        b"<<",
        b">>",
        b"[",
        b"]",
        b"%",
        b"\n",
        b" BI ",
        b" ID ",
        b" EI ",
        b"/W 4 /H 1 /BPC 8 /CS /CS3 ",
        b"/F /DCT ",
        b"/L 9 ",
        b"~>",
        b"\xff\xd9",
        b"\x80",
        b" BDC ",
        b" EMC ",
        b" q ",
        b" Q ",
        b" BT ",
        b" ET ",
        b" /F1 9 Tf ",
        b" 0 -14 Td ",
        b" (x) Tj ",
        b" /X0 Do ",
        b" /Artifact /P0 BDC ",
        b" /Artifact <</Type /Pagination /Subtype /Footer>> BDC ",
        b"4294967296",
    ];

    /// Mutations of each Op JSON fixture: fewer, as each opens a document
    /// and commits; ten times as many of the ops that apply, which reach
    /// further.
    const OP_CASES: usize = 100;

    /// What a mutation of Op JSON may write: its punctuation and values,
    /// references in their forms, and numbers past what ids, positions and
    /// floats hold.
    const JSON_TOKENS: &[&[u8]] = &[
        b"\"",
        b"{",
        b"}",
        b"[",
        b"]",
        b",",
        b":",
        b"null",
        b"true",
        b"-1",
        b"0",
        b"1e309",
        b"-0.0",
        b"4294967296",
        b"9007199254740993",
        b"\"$1\"",
        b"\"$0\"",
        b"{\"pos\":0}",
        b"{\"pos\":4}",
        b"\"all\"",
        b"{\"nm\":\"sheaf:12\"}",
        b"{\"path\":[]}",
        b"{\"asset\":9}",
        b"\"1-4,2\"",
        b"\"\\u0000\"",
    ];

    fn mutate(rng: &mut fastrand::Rng, data: &mut Vec<u8>, tokens: &[&[u8]]) {
        if data.is_empty() {
            data.extend_from_slice(tokens[rng.usize(..tokens.len())]);
            return;
        }
        let at = rng.usize(..data.len());
        match rng.u8(..5) {
            0 => data[at] = rng.u8(..),
            1 => {
                let end = (at + rng.usize(1..=64)).min(data.len());
                data.drain(at..end);
            }
            2 => {
                let token = tokens[rng.usize(..tokens.len())];
                data.splice(at..at, token.iter().copied());
            }
            3 => {
                let end = (at + rng.usize(1..=256)).min(data.len());
                let copy = data[at..end].to_vec();
                let to = rng.usize(..=data.len());
                data.splice(to..to, copy);
            }
            _ => {
                // A number written over a number.
                let end = at + data[at..].iter().take_while(|c| c.is_ascii_digit()).count();
                let number = rng.u64(..).to_string();
                let number = &number[..rng.usize(1..=number.len())];
                data.splice(at..end, number.bytes());
            }
        }
    }

    /// A change to `json`, an envelope, that keeps it JSON, so its ops
    /// mostly still read and are lowered: one of its ops dropped, copied
    /// elsewhere or swapped with another; or anywhere in it, a number
    /// replaced by another of its kind, a string by another or by one
    /// that names something else or nothing, an element of a list dropped
    /// or copied, a field dropped or made null.
    fn mutate_json(rng: &mut fastrand::Rng, json: &mut serde_json::Value) {
        use serde_json::{Value, json};
        fn count(v: &Value) -> usize {
            1 + match v {
                Value::Array(a) => a.iter().map(count).sum(),
                Value::Object(o) => o.values().map(count).sum(),
                _ => 0,
            }
        }
        /// The `k`th node of `v`, in pre-order.
        fn nth<'a>(v: &'a mut Value, k: &mut usize) -> Option<&'a mut Value> {
            if *k == 0 {
                return Some(v);
            }
            *k -= 1;
            match v {
                Value::Array(a) => a.iter_mut().find_map(|x| nth(x, k)),
                Value::Object(o) => o.values_mut().find_map(|x| nth(x, k)),
                _ => None,
            }
        }
        if let Some(ops) = json["ops"].as_array_mut().filter(|o| o.len() > 1)
            && rng.bool()
        {
            let (i, j) = (rng.usize(..ops.len()), rng.usize(..ops.len()));
            match rng.u8(..3) {
                0 => drop(ops.remove(i)),
                1 => ops.insert(j, ops[i].clone()),
                _ => ops.swap(i, j),
            }
            return;
        }
        let mut k = rng.usize(..count(json));
        let node = nth(json, &mut k).expect("a node of it");
        match node {
            Value::Number(n) if n.is_f64() => {
                let floats = [0.0, -1.0, 0.5, 612.0, 1e30];
                *node = json!(floats[rng.usize(..floats.len())]);
            }
            Value::Number(_) => {
                let ints = [0, 1, 2, 3, 4, 7, 12, 99, 4294967295_u64];
                *node = json!(ints[rng.usize(..ints.len())]);
            }
            Value::String(_) => {
                let strings = ["$1", "$5", "$9", "all", "sheaf:12", "a1b2-c3", "", "1-9"];
                *node = json!(strings[rng.usize(..strings.len())]);
            }
            Value::Array(a) if !a.is_empty() => {
                let i = rng.usize(..a.len());
                if rng.bool() {
                    a.remove(i);
                } else {
                    let copy = a[i].clone();
                    a.insert(rng.usize(..=a.len()), copy);
                }
            }
            Value::Object(o) if !o.is_empty() => {
                let key = o.keys().nth(rng.usize(..o.len())).cloned().expect("a key");
                match rng.bool() {
                    true => o.remove(&key),
                    false => o.insert(key, Value::Null),
                };
            }
            _ => *node = Value::Null,
        }
    }

    const XMP: &[u8] = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" \
        xmlns:a=\"http://www.aiim.org/pdfa/ns/id/\" a:part=\"1\"/>";

    fn fixtures() -> Vec<Vec<u8>> {
        let tree = with_table(&[
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /Metadata 7 0 R >>".to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 3 /MediaBox [0 0 600 800] >>".to_vec(),
            ),
            (
                3,
                b"<< /Type /Pages /Parent 2 0 R /Kids [5 0 R 5 0 R] /Rotate 90 >>".to_vec(),
            ),
            (
                4,
                b"<< /Type /Page /Parent 2 0 R /CropBox [10 10 500 700] /Annots [6 0 R] >>"
                    .to_vec(),
            ),
            (
                5,
                b"<< /Type /Page /Parent 3 0 R /Contents 6 0 R >>".to_vec(),
            ),
            (6, stream("/Length 8", b"0 0 m S\n")),
            (
                7,
                stream(&format!("/Type /Metadata /Length {}", XMP.len()), XMP),
            ),
        ]);
        let mut modern = every_kind();
        let mut streams = Vec::new();
        modern.pdf.save_modern(&mut streams).unwrap();
        let mut classic = every_kind();
        classic.pdf.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
        let std_cf = BTreeMap::from([(
            b"StdCF".to_vec(),
            std::sync::Arc::new(Aes128CryptFilter) as std::sync::Arc<dyn CryptFilter>,
        )]);
        let plain = plaintext();
        // The empty user password: the bodies open it, and decrypt.
        let aes = EncryptionState::try_from(EncryptionVersion::V4 {
            document: &plain,
            encrypt_metadata: true,
            crypt_filters: std_cf,
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: "owner",
            user_password: "",
            permissions: Permissions::all(),
        })
        .unwrap();
        let [aes_table, aes_stream] = encrypted(&plain, &aes, |_| {});
        vec![
            tree,
            classic.bytes(),
            streams,
            hybrid_reference(),
            aes_table,
            aes_stream,
        ]
    }

    /// [`sheaf_annot`]'s fixtures: every kind of mark as a save writes it,
    /// and the same with version 1's payloads, which have no version or
    /// digest. Each is the annotations' objects as lopdf writes them,
    /// numbered from 4 in `/Annots` order, without the appearances and
    /// files they refer to, but for what makes a page stamp and an
    /// attachment marks: the stamp's appearance, after them, which names
    /// its page, and the attachment's file specification, which names its
    /// file. Neither the page nor the file is in it.
    fn mark_fixtures() -> Vec<Vec<u8>> {
        let mut f = Form::new(1);
        let mut page = f.dict(f.pages[0]).clone();
        let marks = crate::forms::fixture::holding(&every_mark_kind());
        crate::forms::fixture::write_marks(&mut f.pdf, &mut page, &marks);
        let entries: Vec<ObjectId> = page
            .get(b"Annots")
            .and_then(Object::as_array)
            .unwrap()
            .iter()
            .map(|e| e.as_reference().unwrap())
            .collect();
        let number: HashMap<ObjectId, ObjectId> =
            (4..).zip(&entries).map(|(n, id)| (*id, (n, 0))).collect();
        // The appearance that draws a form: the page stamp's.
        let draws_a_form = |d: &lopdf::Dictionary| {
            let n = d
                .get(b"AP")
                .and_then(Object::as_dict)
                .ok()?
                .get(b"N")
                .ok()?;
            let n = n.as_reference().ok()?;
            let s = f.pdf.get_object(n).ok()?.as_stream().ok()?;
            let resources = s.dict.get(b"Resources").and_then(Object::as_dict).ok()?;
            resources.has(b"XObject").then(|| s.clone())
        };
        [false, true]
            .map(|v1| {
                let mut out = Pdf::with_version("1.7");
                out.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
                let mut next = 4 + entries.len() as u32;
                for id in &entries {
                    let mut d = f.pdf.get_dictionary(*id).unwrap().clone();
                    let ap = draws_a_form(&d);
                    for key in [b"AP".as_slice(), b"P"] {
                        d.remove(key);
                    }
                    if let Some(stream) = ap {
                        out.objects.insert((next, 0), stream.into());
                        d.set("AP", dictionary! { "N" => (next, 0) });
                        next += 1;
                    }
                    if let Ok(irt) = d.get(b"IRT").and_then(Object::as_reference) {
                        d.set("IRT", number[&irt]);
                    }
                    if v1 && let Ok(json) = d.get(b"SheafAnnot").and_then(Object::as_str) {
                        let mut json: serde_json::Value =
                            serde_json::from_str(&crate::file::text_string(json)).unwrap();
                        let fields = json.as_object_mut().unwrap();
                        fields.remove("v");
                        fields.remove("std");
                        d.set("SheafAnnot", crate::save::text_string(&json.to_string()));
                    }
                    out.objects.insert(number[id], d.into());
                }
                let mut bytes = Vec::new();
                out.save_to(&mut bytes).unwrap();
                let at =
                    |token: &[u8]| bytes.windows(token.len()).position(|w| w == token).unwrap();
                bytes[at(b"4 0 obj")..at(b"xref")].to_vec()
            })
            .to_vec()
    }

    /// [`lexer`]'s fixtures: page content with an inline image found by
    /// each of PDFium's rules, ` EI ` in its data where the data is
    /// counted, and strings, arrays, dictionaries and marked content.
    fn content_fixtures() -> Vec<Vec<u8>> {
        let image = |dict: &str, data: &[u8]| {
            [format!("q BI {dict} ID ").as_bytes(), data, b"\nEI Q\n"].concat()
        };
        vec![
            image("/W 4 /H 1 /BPC 8 /CS /G", b" EI "),
            image("/W 2 /H 1 /BPC 8 /CS /CS3", b"a EI b"),
            image("/IM true /W 10 /H 2", b" EI "),
            image("/W 2 /H 2 /BPC 8 /CS [/I /RGB 1 <000000FFFFFF>]", b"EI E"),
            image("/F /AHx /W 2 /H 2 /BPC 8 /CS /G", b"0E EI 0>"),
            image("/F [/A85 /Fl] /W 9 /H 9", b"9jqo^ EI ~>"),
            image("/F /RL /W 4 /H 1 /BPC 8 /CS /G", b"\x03 EI \x80"),
            image("/F /DCT /W 9 /H 9 /BPC 8 /CS /RGB", b"\xff\xd8 EI \xff\xd9"),
            image("/F /Fl /L 6", b"x EI y"),
            image("/F /CCF /DP <</K -1>>", b"xEI EIy"),
            b"%c\nq /Artifact <</Type /Pagination /Subtype /Header /Attached [/Top]>> BDC\n\
              q BT /F1 12 Tf 72 720 Td [(a\\)(b) c) -250 <4142>] TJ (\\\\) Tj ET Q EMC\n\
              [1 [2 [3]] <</A <</B (c)>>>>] true false null Q { } ) ]"
                .to_vec(),
        ]
    }

    /// [`splice`]'s fixtures, beside the lexer's: pagination artifacts
    /// of each origin, Acrobat's opening with the `Q` that ends the page's
    /// `q`, a range across the two streams, ranges inside text objects,
    /// ranges with white-space between, and forms drawn in and out of
    /// artifacts.
    fn splice_fixtures() -> Vec<Vec<u8>> {
        [
            &b"q /Artifact /P0 BDC q BT /F1 9 Tf (Head) Tj ET Q EMC Q 0 0 9 9 re f"[..],
            b"q 0 g 0 0 9 9 re f 1 1 8 8 re f 2 2 7 7 re f 3 3 6 6 re f 0 0 1 rg 9 9 m 4 4 l h S  \
              /Artifact <</Type /Pagination /Subtype /Header>> BDC Q q 1 0 0 1 9 9 cm /X0 Do Q EMC",
            b"0 g/A <<>> BDC EMC  /B <<>> BDC EMC 1 g 0 0 9 9 re f 1 1 8 8 re f 2 2 7 7 re f",
            b"/Artifact <</Type /Pagination /Subtype /Header>> BDC q /X0 Do Q EMC /X1 Do",
            b"BT /F1 12 Tf 72 700 Td /Artifact <</Subtype /Footer>> BDC (f) Tj EMC \
              0 -14 Td (Body) Tj ET",
            b"/Artifact <</Subtype /Watermark /SheafStamp true>> BDC q 0.5 g \
              BT /SheafHF 9 Tf (wm) Tj ET Q EMC /Span <</MCID 0>> BDC (x) Tj EMC",
            b"1 g /Artifact <</Subtype /Background>> BDC 0 0 m 9 9 l S \
              BI /W 2 /H 1 /BPC 8 /CS /CS3 ID ab EIxyz EI EMC Q",
        ]
        .map(<[u8]>::to_vec)
        .into_iter()
        .chain(content_fixtures())
        .collect()
    }

    /// [`op_json`]'s fixtures: every op, between them every form of every
    /// reference and every kind of mark (`sheaf_edit::fixture`), as one
    /// envelope, and each op as an envelope of its own; then
    /// [`applying`]'s ops, which all apply.
    fn op_json_fixtures() -> Vec<Vec<u8>> {
        let ops = sheaf_edit::fixture::every_op();
        let envelope =
            |ops| serde_json::to_vec(&sheaf_edit::Envelope::new(None, ops)).expect("Op JSON");
        let one = ops.iter().map(|op| envelope(vec![op.clone()]));
        (std::iter::once(envelope(ops.clone())).chain(one))
            .chain(std::iter::once(envelope(applying())))
            .collect()
    }

    /// Ops of every kind, and marks of every kind (the fixture's), that
    /// apply to [`op_json`]'s document one after another, each naming what
    /// it needs as the fixture's ops do: by position, title path, `/NM`,
    /// local and grant. Between them they reach each cascade.
    fn applying() -> Vec<sheaf_edit::Op> {
        use sheaf_edit::{
            Anchor, AnnotRef, BlobSel, BookmarkPatch, BookmarkRef, Delta, Local, MarkKind,
            MarkPatch, NewMark, Op, PageLabelRange, PageRef, PageSel, PageSource, Patch, ScopeSel,
            Sibling, Turn, Vec2,
        };
        let pos = PageRef::Pos;
        let positions = |s: &str| PageSel::Positions(s.parse().unwrap());
        let path =
            |titles: &[&str]| BookmarkRef::Path(titles.iter().map(|t| t.to_string()).collect());
        let nm = |nm: &str| AnnotRef::Nm {
            nm: nm.into(),
            page: None,
        };
        let anchor = |page, x| Anchor {
            page: pos(page),
            rect: crate::annot::Box2 {
                x,
                y: 220.0,
                w: 22.0,
                h: 22.0,
            },
        };
        let file = |file, name: &str| PageSource::File {
            file,
            name: name.into(),
        };
        let every = sheaf_edit::fixture::every_op();
        let spec = every.iter().find_map(|op| match op {
            Op::SetHeaderFooter { spec, .. } => Some(spec.clone()),
            _ => None,
        });
        let style = every.iter().find_map(|op| match op {
            Op::AddConnector { style, .. } => Some(style.clone()),
            _ => None,
        });
        let mut ops = vec![
            Op::AddBookmark {
                parent: None,
                at: Sibling::Last,
                title: "Added".into(),
                target: Some(pos(2)),
                view: None,
                color: None,
                style: Default::default(),
                local: Some(Local(1)),
            },
            Op::AddBookmark {
                parent: Some(BookmarkRef::New(Local(1))),
                at: Sibling::First,
                title: "Under it".into(),
                target: Some(pos(3)),
                view: None,
                color: Some([0, 0, 255]),
                style: Default::default(),
                local: None,
            },
            Op::PatchBookmarks {
                bookmarks: vec![path(&["Part I", "Ch. 2"])],
                patch: BookmarkPatch {
                    title: Some("Renamed".into()),
                    target: Patch::Set(pos(4)),
                    ..Default::default()
                },
            },
            Op::MoveBookmarks {
                bookmarks: vec![path(&["Appendix"])],
                parent: Some(BookmarkRef::New(Local(1))),
                at: Sibling::Last,
                with_pages: true,
            },
            Op::AddConnector {
                a: anchor(1, 500.0),
                b: anchor(3, 100.0),
                label: "1".into(),
                style: style.expect("the fixture's connector"),
                locals: vec![Local(2), Local(3)],
            },
            Op::PatchMarks {
                marks: vec![nm("sheaf:12")],
                patch: MarkPatch {
                    fill: Patch::Set([1, 2, 3]),
                    contents: Some("<<name>> on <<date>>".into()),
                    ..Default::default()
                },
                imported: None,
            },
            Op::MoveMarks {
                marks: vec![AnnotRef::New(Local(2))],
                by: Delta::View(Vec2 { x: 5.0, y: 5.0 }),
                to_page: None,
            },
            Op::MoveMarks {
                marks: vec![nm("sheaf:12")],
                by: Delta::Page(Vec2 { x: 1.0, y: 1.0 }),
                to_page: Some(pos(1)),
            },
            Op::AdoptAnnotations {
                annots: vec![nm("acro-1")],
            },
            Op::DeleteAnnotations {
                annots: vec![AnnotRef::New(Local(3)), nm("sheaf:2")],
            },
            Op::SetOcrText {
                page: pos(1),
                layer: sheaf_edit::fixture::layer(),
            },
            Op::ClearOcrText {
                pages: positions("1-2"),
            },
            Op::SetPageLabels {
                ranges: Some(vec![PageLabelRange {
                    start: pos(3),
                    style: sheaf_edit::LabelStyle::UpperLetters,
                    prefix: "App-".into(),
                    first: 1,
                }]),
            },
            Op::SetHeaderFooter {
                spec: spec.expect("the fixture's header"),
                scope: ScopeSel::Pages(positions("1-3")),
            },
            Op::RemoveHeaderFooter {
                pages: positions("2"),
            },
        ];
        // The fixture's marks of every kind on the first page; a reply's
        // and a status's to the other tool's comment on the fourth; and a
        // page stamp's and an attachment's holding the files granted.
        for op in every {
            let Op::AddMark { mark, .. } = op else {
                continue;
            };
            let (page, reply_to, asset) = match mark.kind {
                MarkKind::Reply {} | MarkKind::Status { .. } => (4, Some(nm("a1b2-c3")), None),
                MarkKind::PageStamp { .. } => (1, None, Some(BlobSel::Asset(0))),
                MarkKind::FileAttachment { .. } => (1, None, Some(BlobSel::Asset(1))),
                _ => (1, None, None),
            };
            ops.push(Op::AddMark {
                page: pos(page),
                mark: NewMark { asset, ..mark },
                reply_to,
                local: None,
                imported: None,
            });
        }
        // Then pages turned, copied, brought in, replaced and moved; and
        // some go, with what's on them, their fields, the bookmarks to
        // them, a label range's start and the header's pages.
        ops.extend([
            Op::RotatePages {
                pages: PageSel::Span {
                    first: pos(1),
                    last: pos(2),
                },
                by: Turn::Quarter,
            },
            Op::DuplicatePages {
                pages: positions("1,3"),
                locals: vec![Local(4), Local(5)],
            },
            Op::InsertPages {
                from: file(BlobSel::Asset(2), "scan.pdf"),
                pages: Some("1-2".parse().unwrap()),
                before: Some(PageRef::New(Local(4))),
                bring: Default::default(),
                locals: vec![Local(6), Local(7)],
            },
            Op::InsertPages {
                from: file(BlobSel::With("logo".into()), "logo.pdf"),
                pages: None,
                before: None,
                bring: Default::default(),
                locals: Vec::new(),
            },
            Op::ReplacePages {
                pages: vec![PageRef::New(Local(6))],
                from: file(BlobSel::Asset(2), "scan.pdf"),
                with: Some("2".parse().unwrap()),
                bring: Default::default(),
            },
            Op::MovePages {
                pages: PageSel::Ids(vec![PageRef::New(Local(5))]),
                before: Some(pos(1)),
            },
            Op::DeleteBookmarks {
                bookmarks: vec![BookmarkRef::New(Local(1))],
            },
            Op::DeletePages {
                pages: positions("2-4"),
            },
        ]);
        ops
    }

    /// The fixtures, as the fuzz targets' first corpus, into the directory
    /// `SHEAF_FUZZ_SEEDS` names: `fuzz.yml` seeds each night's run with
    /// them, so the repository holds no PDF. The target
    /// `SHEAF_FUZZ_TARGET` names takes the marks' fixtures when it's
    /// `sheaf_annot`, the content when it's `lexer` or `splice`, Op JSON
    /// when it's `op_json`, and the files otherwise.
    #[test]
    #[ignore = "fuzz.yml's seeds"]
    fn write_fuzz_seeds() {
        let dir = std::env::var_os("SHEAF_FUZZ_SEEDS").expect("SHEAF_FUZZ_SEEDS names a directory");
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        let named = |seeds: Vec<Vec<u8>>, name: fn(usize) -> String| {
            seeds
                .into_iter()
                .enumerate()
                .map(move |(n, seed)| (name(n), seed))
        };
        let seeds: Vec<(String, Vec<u8>)> = match std::env::var("SHEAF_FUZZ_TARGET").as_deref() {
            Ok("sheaf_annot") => named(mark_fixtures(), |n| format!("marks-{n}")).collect(),
            Ok("lexer") => named(content_fixtures(), |n| format!("content-{n}")).collect(),
            Ok("splice") => named(splice_fixtures(), |n| format!("content-{n}")).collect(),
            Ok("op_json") => named(op_json_fixtures(), |n| format!("ops-{n}.json")).collect(),
            _ => named(fixtures(), |n| format!("fixture-{n}.pdf")).collect(),
        };
        for (name, seed) in seeds {
            std::fs::write(dir.join(name), seed).unwrap();
        }
    }

    /// Unmutated, the marks' fixtures read as marks, every one: version 2's
    /// by their digests, version 1's from the standard keys, a
    /// replacement's struck text from its StrikeOut.
    #[test]
    fn the_mark_fixtures_read_as_marks() {
        for fixture in mark_fixtures() {
            let src = annotated_page(&fixture).unwrap();
            let page = src.page(0).unwrap();
            let annots = crate::file::PageAnnots::new(&page.annots);
            let read = page
                .annots
                .iter()
                .filter_map(|e| crate::file::sheaf_mark(src.pdf(), e, page.geom.rotate, &annots));
            assert_eq!(read.count(), every_mark_kind().len());
        }
    }

    #[test]
    fn fuzz_bodies_survive_mutated_fixtures() {
        let fixtures = fixtures();
        let started = Instant::now();
        let mut rng = fastrand::Rng::with_seed(0x5eaf_f022);
        for (n, fixture) in fixtures.iter().enumerate() {
            // Unmutated, each opens: the mutations start from a file.
            let kind = BlobKind::file(Path::new("fixture.pdf"), None);
            let src = Store::new().open(fixture.as_slice().into(), kind).unwrap();
            assert!(src.page_count() > 0);
            for case in 0..CASES {
                let mut data = fixture.clone();
                for _ in 0..rng.usize(1..=4) {
                    mutate(&mut rng, &mut data, TOKENS);
                }
                let ran = std::panic::catch_unwind(|| {
                    lazy_reader(&data);
                    page_tree(&data);
                });
                // The seed alone can't replay a case of the AES fixtures,
                // whose IVs are drawn afresh each run: the input is kept.
                if let Err(panic) = ran {
                    let kept = std::env::temp_dir().join(format!("sheaf-fuzz-{n}-{case}.pdf"));
                    std::fs::write(&kept, &data).unwrap();
                    eprintln!("fixture {n}, case {case}: the input is {}", kept.display());
                    std::panic::resume_unwind(panic);
                }
            }
        }
        for (n, fixture) in mark_fixtures().iter().enumerate() {
            for case in 0..CASES {
                let mut data = fixture.clone();
                for _ in 0..rng.usize(1..=4) {
                    mutate(&mut rng, &mut data, TOKENS);
                }
                let ran = std::panic::catch_unwind(|| sheaf_annot(&data));
                if let Err(panic) = ran {
                    let kept = std::env::temp_dir().join(format!("sheaf-fuzz-marks-{n}-{case}"));
                    std::fs::write(&kept, &data).unwrap();
                    eprintln!("marks {n}, case {case}: the input is {}", kept.display());
                    std::panic::resume_unwind(panic);
                }
            }
        }
        for (n, fixture) in splice_fixtures().iter().enumerate() {
            // Unmutated, each lexes, and those with marked content splice.
            let res = ResCx::new([(b"CS3".to_vec(), 3)]);
            assert!(crate::content::lex(fixture, &res, &Limits::DEFAULT).all(|t| t.is_ok()));
            let marks = fixture.windows(3).any(|w| w == b"BDC");
            assert_eq!(splice(fixture) > 0, marks, "fixture {n}");
            for case in 0..CASES {
                let mut data = fixture.clone();
                for _ in 0..rng.usize(1..=4) {
                    mutate(&mut rng, &mut data, CONTENT_TOKENS);
                }
                let ran = std::panic::catch_unwind(|| {
                    lexer(&data);
                    splice(&data);
                });
                if let Err(panic) = ran {
                    let kept = std::env::temp_dir().join(format!("sheaf-fuzz-content-{n}-{case}"));
                    std::fs::write(&kept, &data).unwrap();
                    eprintln!("content {n}, case {case}: the input is {}", kept.display());
                    std::panic::resume_unwind(panic);
                }
            }
        }
        // Unmutated, the ops made to apply do, all of them.
        let fixtures = op_json_fixtures();
        assert_eq!(op_json(fixtures.last().unwrap()), applying().len());
        for (n, fixture) in fixtures.iter().enumerate() {
            let json: serde_json::Value = serde_json::from_slice(fixture).unwrap();
            let cases = match n + 1 == fixtures.len() {
                true => 10 * OP_CASES,
                false => OP_CASES,
            };
            for case in 0..cases {
                // Mostly JSON still, which reaches lowering; now and then
                // bytes, which reach the reader.
                let mut data = match rng.u8(..4) {
                    0 => fixture.clone(),
                    _ => {
                        let mut json = json.clone();
                        for _ in 0..rng.usize(1..=3) {
                            mutate_json(&mut rng, &mut json);
                        }
                        serde_json::to_vec(&json).unwrap()
                    }
                };
                for _ in 0..rng.usize(0..=1) {
                    mutate(&mut rng, &mut data, JSON_TOKENS);
                }
                let ran = std::panic::catch_unwind(|| op_json(&data));
                if let Err(panic) = ran {
                    let kept = std::env::temp_dir().join(format!("sheaf-fuzz-ops-{n}-{case}.json"));
                    std::fs::write(&kept, &data).unwrap();
                    eprintln!("ops {n}, case {case}: the input is {}", kept.display());
                    std::panic::resume_unwind(panic);
                }
            }
        }
        let took = started.elapsed();
        assert!(took < Duration::from_secs(60), "{took:?}");
    }
}
