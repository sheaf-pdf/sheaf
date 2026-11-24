//! Files embedded in a PDF: the document-level ones in the catalog's
//! `/EmbeddedFiles` name tree and those on file-attachment annotations.
//! Listed when a file opens; bytes are read on demand.

use std::collections::HashSet;

use crate::pdf::semantics::{Limits, tree};
use crate::pdf::{
    Dictionary, Document as Pdf, LazyPdf, Object, ObjectId, StreamExt, resolve_fn, text,
};
use anyhow::{Context, Result, bail};

use crate::annot::AnnotId;
use crate::{DocState, Source};

/// Where an attachment lives, stable across reloads of the same file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AttachmentRef {
    /// A key in `/Names /EmbeddedFiles`.
    Named(String),
    /// The `n`th file-attachment annotation on page `page` (file order).
    Annot { page: u32, n: u32 },
    /// One of sheaf's, its mark: the file it holds, saved or not yet.
    Mark(AnnotId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub at: AttachmentRef,
    /// The file name (`/UF`, else `/F`), without any folder.
    pub name: String,
    pub description: String,
    /// Uncompressed size, when the file says (`/Params /Size`).
    pub size: Option<u64>,
    /// `/Params /ModDate` and `/CreationDate`, PDF date strings.
    pub modified: String,
    pub created: String,
    /// Portfolio folder path ("Tax/2024"), empty at the top level.
    pub folder: String,
    /// Portfolio metadata (`/CI`) by schema field key, as text.
    pub fields: Vec<(String, String)>,
}

impl Attachment {
    pub fn is_pdf(&self) -> bool {
        self.name.to_ascii_lowercase().ends_with(".pdf")
    }
}

/// Every attachment in the file: named ones in name-tree order, then
/// annotation ones by page. `pages` are the page objects, by index.
pub fn attachments_in<P: Objects + ?Sized>(
    pdf: &P,
    pages: impl IntoIterator<Item = ObjectId>,
) -> Vec<Attachment> {
    let mut out = Vec::new();
    let folders = crate::portfolio::folder_paths(pdf);
    for (key, spec) in name_tree(pdf, b"EmbeddedFiles") {
        if let Some(d) = resolve(pdf, spec).and_then(Object::as_dict) {
            let mut a = describe(pdf, AttachmentRef::Named(key.clone()), d);
            // Portfolio files are keyed "<folder id>name".
            if let Some((id, rest)) = folder_key(&key)
                && let Some(path) = folders.get(&id)
            {
                a.folder = path.clone();
                if a.name.is_empty() {
                    a.name = rest.to_string();
                }
            }
            if a.name.is_empty() {
                a.name = key;
            }
            out.push(a);
        }
    }
    for (page_no, page_id) in pages.into_iter().enumerate() {
        let mut n = 0;
        let entries = page_entries(pdf, page_id);
        for entry in entries {
            let Some(annot) = resolve(pdf, entry).and_then(Object::as_dict) else {
                continue;
            };
            if annot.get(b"Subtype").and_then(Object::as_name) != Some(b"FileAttachment") {
                continue;
            }
            let at = AttachmentRef::Annot {
                page: page_no as u32,
                n,
            };
            n += 1;
            // sheaf's own are listed from the document (`Mark`); one
            // another tool has edited since is that tool's.
            if crate::file::is_ours(pdf, entry) {
                continue;
            }
            let Some(fs) = annot
                .get(b"FS")
                .and_then(|o| resolve(pdf, o))
                .and_then(Object::as_dict)
            else {
                continue;
            };
            let mut a = describe(pdf, at, fs);
            if a.description.is_empty() {
                a.description = annot.get(b"Contents").map(text::of).unwrap_or_default();
            }
            out.push(a);
        }
    }
    out
}

/// The bytes of attachment `at` in `src`, its pages numbered as its page
/// table numbers them, as they're listed; one of sheaf's from its mark in
/// `st`.
pub fn read_attachment(st: &DocState, src: &Source, at: &AttachmentRef) -> Result<Vec<u8>> {
    if let AttachmentRef::Mark(id) = at {
        let mark = st.annot(*id).and_then(|a| a.mark_record());
        let file = mark.context("the attachment isn't in the document")?;
        let file = file.file.as_ref().context("the attachment holds no file")?;
        return Ok(file.bytes().to_vec());
    }
    attachment_bytes(src.pdf(), src.page_ids(), at)
}

/// The bytes of attachment `at` in a file whose page objects, by index,
/// are `pages`: another tool's, or the file's own.
pub fn attachment_bytes<P: Objects + ?Sized>(
    pdf: &P,
    pages: impl IntoIterator<Item = ObjectId>,
    at: &AttachmentRef,
) -> Result<Vec<u8>> {
    let spec = match at {
        AttachmentRef::Named(key) => name_tree(pdf, b"EmbeddedFiles")
            .into_iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, o)| resolve(pdf, o))
            .and_then(Object::as_dict)
            .with_context(|| format!("no embedded file named {key:?}"))?,
        AttachmentRef::Mark(_) => bail!("one of sheaf's is read from its mark"),
        AttachmentRef::Annot { page, n } => {
            let page_id = pages
                .into_iter()
                .nth(*page as usize)
                .with_context(|| format!("no page {}", page + 1))?;
            page_annots(pdf, page_id)
                .into_iter()
                .filter(|a| a.get(b"Subtype").and_then(Object::as_name) == Some(b"FileAttachment"))
                .nth(*n as usize)
                .and_then(|a| a.get(b"FS"))
                .and_then(|o| resolve(pdf, o))
                .and_then(Object::as_dict)
                .context("attachment annotation missing")?
        }
    };
    let ef = spec
        .get(b"EF")
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_dict)
        .context("the attachment has no embedded file (it may be a link to an outside file)")?;
    let stream = [b"UF".as_slice(), b"F"]
        .iter()
        .find_map(|k| ef.get(k))
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_stream)
        .context("embedded file stream missing")?;
    file_bytes(stream)
}

/// An embedded file stream's bytes, decoded.
pub(crate) fn file_bytes(stream: &crate::pdf::Stream) -> Result<Vec<u8>> {
    decoded(stream, None)
}

/// The bytes of the file an attachment mark holds, `stream`, which the
/// mark says is `size` bytes: none past that is decoded, so a small
/// stream can't swell into a large one the mark didn't say.
pub(crate) fn mark_file_bytes(stream: &crate::pdf::Stream, size: u64) -> Result<Vec<u8>> {
    decoded(stream, Some(usize::try_from(size).unwrap_or(usize::MAX)))
}

/// `stream`'s bytes, decoded, at most `limit` of them.
fn decoded(stream: &crate::pdf::Stream, limit: Option<usize>) -> Result<Vec<u8>> {
    let bytes = match limit {
        Some(limit) => stream.decompressed_content_with_limit(limit),
        None => stream.decompressed_content(),
    };
    bytes.map_err(|e| anyhow::anyhow!("can't decode the embedded file: {e}"))
}

/// A file specification's name, description and parameters.
fn describe<P: Objects + ?Sized>(pdf: &P, at: AttachmentRef, fs: &Dictionary) -> Attachment {
    let text = |d: &Dictionary, k: &[u8]| {
        d.get(k)
            .and_then(|o| resolve(pdf, o))
            .map(text::of)
            .unwrap_or_default()
    };
    let name = [b"UF".as_slice(), b"F"]
        .iter()
        .map(|k| text(fs, k))
        .find(|s| !s.is_empty())
        .unwrap_or_default();
    // Some writers put a path in /F; show the file's own name.
    let name = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_string();
    let params = fs
        .get(b"EF")
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_dict)
        .and_then(|ef| [b"UF".as_slice(), b"F"].iter().find_map(|k| ef.get(k)))
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_stream)
        .and_then(|s| s.dict.get(b"Params"))
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_dict);
    let size = params
        .and_then(|p| p.get(b"Size"))
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_integer)
        .and_then(|n| u64::try_from(n).ok());
    let fields = fs
        .get(b"CI")
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_dict)
        .map(|ci| {
            ci.iter()
                .filter(|(k, _)| *k != b"Type")
                .map(|(k, v)| {
                    (
                        String::from_utf8_lossy(k).into_owned(),
                        crate::portfolio::field_text(pdf, v),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    Attachment {
        at,
        name,
        description: text(fs, b"Desc"),
        size,
        modified: params.map(|p| text(p, b"ModDate")).unwrap_or_default(),
        created: params.map(|p| text(p, b"CreationDate")).unwrap_or_default(),
        folder: String::new(),
        fields,
    }
}

/// One of the catalog's `/Names` trees, flattened: (key, value) in tree
/// order, each key read as a text string. `/EmbeddedFiles` maps names to
/// file specs, `/Dests` to destinations. Walked by [`tree_names`].
pub(crate) fn name_tree<'a, P: Objects + ?Sized>(
    pdf: &'a P,
    tree: &[u8],
) -> Vec<(String, &'a Object)> {
    let root = pdf
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Names"))
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_dict)
        .and_then(|n| n.get(tree));
    (root.into_iter())
        .flat_map(|root| tree_names(pdf, root))
        .map(|(key, value)| (text::decode(key), value))
        .collect()
}

/// The entries of the name tree at `root`, each key's bytes and its value
/// as written, in the tree's order: the family's walk
/// ([`tree::names`]), under its default caps (32 levels, 100,000 nodes).
/// Each node and each array of one is read once, however often it's
/// listed, so a loop ends; a key is followed through its references; a
/// node is a dictionary, never a stream. Passed, the walk going on: an
/// object `pdf` gives none for (not there, damaged, past its caps, a loop
/// of references), a kid that's no dictionary, a key that's no string, an
/// odd last key. `pdf` follows a chain of references itself, so the
/// family's walk sees one hop of any and never fails over it; should it
/// fail, what it gave before is kept and it ends there.
pub(crate) fn tree_names<'a, P: Objects + ?Sized>(
    pdf: &'a P,
    root: &'a Object,
) -> impl Iterator<Item = (&'a [u8], &'a Object)> + 'a {
    let objects = resolve_fn(move |id: ObjectId| pdf.object(id));
    tree::names(objects, root, &Limits::DEFAULT, ()).map_while(Result::ok)
}

/// The entries of the number tree at `root`, as [`tree_names`] gives a
/// name tree's: each key, an integer, and its value.
pub(crate) fn tree_numbers<'a, P: Objects + ?Sized>(
    pdf: &'a P,
    root: &'a Object,
) -> impl Iterator<Item = (i64, &'a Object)> + 'a {
    let objects = resolve_fn(move |id: ObjectId| pdf.object(id));
    tree::numbers(objects, root, &Limits::DEFAULT, ()).map_while(Result::ok)
}

fn page_annots<P: Objects + ?Sized>(pdf: &P, page: ObjectId) -> Vec<&Dictionary> {
    page_entries(pdf, page)
        .iter()
        .filter_map(|o| resolve(pdf, o).and_then(Object::as_dict))
        .collect()
}

/// A page's `/Annots`, as written.
fn page_entries<P: Objects + ?Sized>(pdf: &P, page: ObjectId) -> &[Object] {
    pdf.get_dictionary(page)
        .ok()
        .and_then(|p| p.get(b"Annots"))
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_array)
        .unwrap_or(&[])
}

/// A portfolio key "<12>Report.pdf" as (12, "Report.pdf").
fn folder_key(key: &str) -> Option<(i64, &str)> {
    let rest = key.strip_prefix('<')?;
    let (id, name) = rest.split_once('>')?;
    Some((id.parse().ok()?, name))
}

/// What objects are read from: a source's [`LazyPdf`], or a document
/// loaded whole, so the readers serve both. Its reads match `crate::pdf`'s,
/// by their names, so a reader's body reads the same over either. Every file
/// a document reads, and every PDF sheaf builds in memory, is read as a
/// source; what's loaded whole is a Rewrite's graph (`materialize`'s
/// records of it and `keep_only`, until a Rewrite needn't load the file
/// whole), the form check's file and its revisions (`forms::check`), a
/// stamp collection, the user's own file (`stamp_lib`), and tests' files.
pub trait Objects {
    /// Object `id`, its references followed.
    fn get_object(&self, id: ObjectId) -> crate::pdf::Result<&Object>;

    fn get_dictionary(&self, id: ObjectId) -> crate::pdf::Result<&Dictionary>;

    /// `object`, or what its chain of references ends at, with the last
    /// reference's id.
    fn dereference<'a>(
        &'a self,
        object: &'a Object,
    ) -> crate::pdf::Result<(Option<ObjectId>, &'a Object)>;

    fn catalog(&self) -> crate::pdf::Result<&Dictionary>;

    /// Object `id`, its references followed; None when it can't be read.
    fn object(&self, id: ObjectId) -> Option<&Object> {
        self.get_object(id).ok()
    }

    fn dictionary(&self, id: ObjectId) -> Option<&Dictionary> {
        self.get_dictionary(id).ok()
    }
}

impl Objects for Pdf {
    fn get_object(&self, id: ObjectId) -> crate::pdf::Result<&Object> {
        Pdf::get_object(self, id)
    }

    fn get_dictionary(&self, id: ObjectId) -> crate::pdf::Result<&Dictionary> {
        Pdf::get_dictionary(self, id)
    }

    fn dereference<'a>(
        &'a self,
        object: &'a Object,
    ) -> crate::pdf::Result<(Option<ObjectId>, &'a Object)> {
        Pdf::dereference(self, object)
    }

    fn catalog(&self) -> crate::pdf::Result<&Dictionary> {
        Pdf::catalog(self)
    }
}

impl Objects for LazyPdf {
    fn get_object(&self, id: ObjectId) -> crate::pdf::Result<&Object> {
        LazyPdf::get_object(self, id)
    }

    fn get_dictionary(&self, id: ObjectId) -> crate::pdf::Result<&Dictionary> {
        LazyPdf::get_dictionary(self, id)
    }

    fn dereference<'a>(
        &'a self,
        object: &'a Object,
    ) -> crate::pdf::Result<(Option<ObjectId>, &'a Object)> {
        LazyPdf::dereference(self, object)
    }

    fn catalog(&self) -> crate::pdf::Result<&Dictionary> {
        LazyPdf::catalog(self)
    }
}

pub(crate) fn resolve<'a, P: Objects + ?Sized>(pdf: &'a P, o: &'a Object) -> Option<&'a Object> {
    match o.as_reference() {
        Some(id) => pdf.object(id),
        None => Some(o),
    }
}

/// `o` resolved, unless it's a reference `seen` already has: a tree's walk
/// reads each node it reaches by reference once, however often it's
/// listed, and so ends.
pub(crate) fn once<'a, P: Objects + ?Sized>(
    pdf: &'a P,
    seen: &mut HashSet<ObjectId>,
    o: &'a Object,
) -> Option<&'a Object> {
    if let Some(id) = o.as_reference()
        && !seen.insert(id)
    {
        return None;
    }
    resolve(pdf, o)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::pdf::{Stream, dictionary, literal};

    /// A one-page file with a named attachment, a compressed one and a
    /// file-attachment annotation.
    pub(crate) fn sample() -> Pdf {
        let mut pdf = Pdf::with_version("1.7");
        let plain = pdf.add_object(Stream::new(
            dictionary! { "Type" => Object::name("EmbeddedFile"), "Params" => dictionary! { "Size" => 5, "ModDate" => literal("D:20240102030405Z") } },
            b"hello".to_vec(),
        ));
        let mut packed = Stream::new(
            dictionary! { "Type" => Object::name("EmbeddedFile") },
            b"%PDF-1.4 tiny".to_vec(),
        );
        packed.compress();
        let packed = pdf.add_object(packed);
        let spec_a = pdf.add_object(dictionary! {
            "Type" => Object::name("Filespec"),
            "F" => literal("notes.txt"),
            "Desc" => literal("Client notes"),
            "EF" => dictionary! { "F" => plain },
        });
        let spec_b = dictionary! {
            "Type" => Object::name("Filespec"),
            "UF" => literal("sub/dir/w2.pdf"),
            "EF" => dictionary! { "F" => packed },
        };
        let annot_stream = pdf.add_object(Stream::new(dictionary! {}, b"annot bytes".to_vec()));
        let annot = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"),
            "Subtype" => Object::name("FileAttachment"),
            "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "Contents" => literal("Support"),
            "FS" => dictionary! { "Type" => Object::name("Filespec"), "F" => literal("support.xlsx"), "EF" => dictionary! { "F" => annot_stream } },
        });
        let pages_id = pdf.new_object_id();
        let page = pdf.add_object(dictionary! {
            "Type" => Object::name("Page"),
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Annots" => vec![annot.into()],
        });
        pdf.objects.insert(
            pages_id,
            dictionary! { "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1 }.into(),
        );
        let catalog = pdf.add_object(dictionary! {
            "Type" => Object::name("Catalog"),
            "Pages" => pages_id,
            "Names" => dictionary! {
                "EmbeddedFiles" => dictionary! {
                    "Kids" => vec![dictionary! {
                        "Names" => vec![
                            literal("notes.txt"), spec_a.into(),
                            literal("w2"), spec_b.into(),
                        ],
                    }.into()],
                },
            },
        });
        pdf.trailer.set("Root", catalog);
        pdf
    }

    #[test]
    fn lists_named_and_annotation_attachments() {
        let pdf = sample();
        let list = attachments_in(&pdf, pdf.page_iter());
        let names: Vec<&str> = list.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["notes.txt", "w2.pdf", "support.xlsx"]);
        assert_eq!(list[0].description, "Client notes");
        assert_eq!(list[0].size, Some(5));
        assert_eq!(list[0].modified, "D:20240102030405Z");
        assert!(list[1].is_pdf());
        assert_eq!(list[2].description, "Support");
        assert_eq!(list[2].at, AttachmentRef::Annot { page: 0, n: 0 });
    }

    #[test]
    fn reads_bytes_plain_and_compressed() {
        let pdf = sample();
        let list = attachments_in(&pdf, pdf.page_iter());
        let bytes = |at: &AttachmentRef| attachment_bytes(&pdf, pdf.page_iter(), at);
        assert_eq!(bytes(&list[0].at).unwrap(), b"hello");
        assert_eq!(bytes(&list[1].at).unwrap(), b"%PDF-1.4 tiny");
        assert_eq!(bytes(&list[2].at).unwrap(), b"annot bytes");
        assert!(bytes(&AttachmentRef::Named("nope".into())).is_err());
        assert!(bytes(&AttachmentRef::Annot { page: 1, n: 0 }).is_err());
    }

    /// Read from a source, as a file opens, the sample lists as it does
    /// loaded, and each attachment reads the same bytes.
    #[test]
    fn a_source_lists_and_reads_as_the_loaded_file() {
        let mut pdf = sample();
        let src = crate::open::tests::lazily(&mut pdf);
        let list = attachments_in(&pdf, pdf.page_iter());
        assert_eq!(attachments_in(src.pdf(), src.page_ids()), list);
        let st = DocState::builder().build();
        for a in &list {
            assert_eq!(
                read_attachment(&st, &src, &a.at).unwrap(),
                attachment_bytes(&pdf, pdf.page_iter(), &a.at).unwrap(),
                "{}",
                a.name
            );
        }
    }

    /// An attachment's name, key and description are text strings, read
    /// as PDFium reads one: 0x90 and 0x85 are PDFDocEncoding's ’ and –,
    /// not Latin-1's control characters. The file by such a key is found.
    #[test]
    fn a_name_and_description_read_as_pdfdocencoding() {
        let mut pdf = Pdf::with_version("1.7");
        let file = pdf.add_object(Stream::new(dictionary! {}, b"minutes".to_vec()));
        let spec = pdf.add_object(dictionary! {
            "Type" => Object::name("Filespec"),
            "UF" => literal(b"Ann\x90s \x85 minutes.txt"),
            "Desc" => literal(b"Ann\x90s notes \x85 May"),
            "EF" => dictionary! { "F" => file },
        });
        // An annotation's description is its `/Contents` where its file
        // specification has none.
        let sheet = pdf.add_object(Stream::new(dictionary! {}, b"sheet".to_vec()));
        let annot = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"),
            "Subtype" => Object::name("FileAttachment"),
            "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "Contents" => literal(b"Bob\x90s \x85 sheet"),
            "FS" => dictionary! { "F" => literal("sheet.csv"), "EF" => dictionary! { "F" => sheet } },
        });
        let pages_id = pdf.new_object_id();
        let page = pdf.add_object(dictionary! {
            "Type" => Object::name("Page"),
            "Parent" => pages_id,
            "Annots" => vec![annot.into()],
        });
        pdf.objects.insert(
            pages_id,
            dictionary! { "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1 }.into(),
        );
        let catalog = pdf.add_object(dictionary! {
            "Type" => Object::name("Catalog"),
            "Pages" => pages_id,
            "Names" => dictionary! {
                "EmbeddedFiles" => dictionary! {
                    "Names" => vec![literal(b"Ann\x90s \x85 key"), spec.into()],
                },
            },
        });
        pdf.trailer.set("Root", catalog);
        let list = attachments_in(&pdf, pdf.page_iter());
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].at, AttachmentRef::Named("Ann’s – key".into()));
        assert_eq!(list[0].name, "Ann’s – minutes.txt");
        assert_eq!(list[0].description, "Ann’s notes – May");
        assert_eq!(list[1].description, "Bob’s – sheet");
        let bytes = attachment_bytes(&pdf, pdf.page_iter(), &list[0].at).unwrap();
        assert_eq!(bytes, b"minutes");
    }

    #[test]
    fn portfolio_keys_split() {
        assert_eq!(folder_key("<3>a.pdf"), Some((3, "a.pdf")));
        assert_eq!(folder_key("a.pdf"), None);
    }
}

/// The walk of a name or number tree ([`tree_names`], [`tree_numbers`]),
/// over a document held whole and over the same file read as a source.
#[cfg(test)]
mod tree_tests {
    use super::*;
    use crate::pdf::{Stream, dictionary, literal};

    fn id(n: u32) -> ObjectId {
        ObjectId::new(n, 0)
    }

    fn r(n: u32) -> Object {
        Object::from(id(n))
    }

    /// A file whose catalog holds `tree` as `/Tree`, with `objects` by
    /// number (from 10, past what the file's own take).
    fn file(objects: Vec<(u32, Object)>, tree: Object) -> Pdf {
        let mut pdf = Pdf::with_version("1.7");
        let catalog =
            pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Tree" => tree });
        pdf.trailer.set("Root", catalog);
        for (n, o) in objects {
            assert!(n >= 10);
            pdf.objects.insert(id(n), o);
        }
        pdf
    }

    fn root<P: Objects + ?Sized>(pdf: &P) -> &Object {
        pdf.catalog().unwrap().get(b"Tree").unwrap()
    }

    /// Each entry as `key=value`, the value an integer or `?`.
    fn shown(key: String, value: &Object) -> String {
        let value = value.as_integer().map_or("?".into(), |n| n.to_string());
        format!("{key}={value}")
    }

    /// What `tree_names` gives of the file's `/Tree`, the same from the
    /// document and from its source.
    fn names(mut pdf: Pdf) -> Vec<String> {
        fn of<P: Objects + ?Sized>(pdf: &P) -> Vec<String> {
            (tree_names(pdf, root(pdf)))
                .map(|(k, v)| shown(String::from_utf8_lossy(k).into_owned(), v))
                .collect()
        }
        let whole = of(&pdf);
        let src = crate::open::tests::lazily(&mut pdf);
        assert_eq!(of(src.pdf()), whole, "read as a source");
        whole
    }

    /// What `tree_numbers` gives, as [`names`].
    fn numbers(mut pdf: Pdf) -> Vec<String> {
        fn of<P: Objects + ?Sized>(pdf: &P) -> Vec<String> {
            (tree_numbers(pdf, root(pdf)))
                .map(|(k, v)| shown(k.to_string(), v))
                .collect()
        }
        let whole = of(&pdf);
        let src = crate::open::tests::lazily(&mut pdf);
        assert_eq!(of(src.pdf()), whole, "read as a source");
        whole
    }

    /// A leaf holding `/Names [(key) value]`.
    fn leaf(key: &str, value: i64) -> Object {
        Object::from(dictionary! { "Names" => vec![literal(key), value.into()] })
    }

    /// A chain of nodes, each one level below the last, each with a key
    /// of its depth: the nodes 32 levels below the root are read, and the
    /// one 33 below isn't.
    #[test]
    fn a_tree_is_read_to_32_levels_below_its_root() {
        let objects = (0..=33u32)
            .map(|depth| {
                let mut node = dictionary! {
                    "Names" => vec![literal(format!("d{depth:02}")), i64::from(depth).into()],
                };
                if depth < 33 {
                    node.set("Kids", vec![r(10 + depth + 1)]);
                }
                (10 + depth, Object::from(node))
            })
            .collect();
        let read = names(file(objects, r(10)));
        let want: Vec<String> = (0..=32).map(|d| format!("d{d:02}={d}")).collect();
        assert_eq!(read, want);
    }

    /// A node the tree lists twice, and an array of entries two nodes
    /// share, are each read once.
    #[test]
    fn a_node_listed_twice_and_an_array_two_share_are_read_once() {
        let objects = vec![
            (10, leaf("a", 1)),
            (11, Object::from(vec![literal("b"), 2.into()])),
            (12, Object::from(dictionary! { "Names" => r(11) })),
            (13, Object::from(dictionary! { "Names" => r(11) })),
        ];
        let tree = Object::from(dictionary! { "Kids" => vec![r(10), r(10), r(12), r(13)] });
        assert_eq!(names(file(objects, tree)), ["a=1", "b=2"]);
    }

    /// A node among its own descendants: the walk ends, each node read
    /// once.
    #[test]
    fn a_loop_ends() {
        let objects = vec![
            (
                10,
                Object::from(
                    dictionary! { "Names" => vec![literal("a"), 1.into()], "Kids" => vec![r(11)] },
                ),
            ),
            (
                11,
                Object::from(
                    dictionary! { "Names" => vec![literal("b"), 2.into()], "Kids" => vec![r(10), r(11)] },
                ),
            ),
        ];
        assert_eq!(names(file(objects, r(10))), ["a=1", "b=2"]);
    }

    /// A key that's no string (a name, an integer) is passed and the walk
    /// goes on; a key behind a reference is followed; an odd last key is
    /// passed. In a number tree, a key that's no integer is passed.
    #[test]
    fn an_odd_last_key_and_a_key_that_s_no_string_are_passed() {
        let tree = Object::from(dictionary! {
            "Names" => vec![
                literal("a"), 1.into(),
                Object::name("b"), 2.into(),
                3.into(), 3.into(),
                literal("c"), 4.into(),
                r(10), 5.into(),
                literal("d"),
            ],
        });
        let objects = vec![(10, literal("e"))];
        assert_eq!(names(file(objects, tree)), ["a=1", "c=4", "e=5"]);

        let tree = Object::from(dictionary! {
            "Nums" => vec![
                0.into(), 1.into(),
                literal("x"), 2.into(),
                Object::from(2.0), 3.into(),
                r(10), 4.into(),
                7.into(),
            ],
        });
        let objects = vec![(10, Object::from(4))];
        assert_eq!(numbers(file(objects, tree)), ["0=1", "4=4"]);
    }

    /// A stream holding a node's entries in its dictionary is no node,
    /// as a kid or as the root.
    #[test]
    fn a_stream_isn_t_a_node() {
        let stream = |key: &str| {
            let dict = dictionary! { "Names" => vec![literal(key), 9.into()] };
            Object::from(Stream::new(dict, b"x".to_vec()))
        };
        let objects = vec![(10, stream("s")), (11, leaf("a", 1))];
        let tree = Object::from(dictionary! { "Kids" => vec![r(10), r(11)] });
        assert_eq!(names(file(objects, tree)), ["a=1"]);

        let objects = vec![(10, stream("s"))];
        assert!(names(file(objects, r(10))).is_empty());
    }

    /// An object that's a reference to itself, listed among the kids,
    /// names nothing `pdf` can read: it's passed, as any kid that doesn't
    /// read is, and the walk goes on to the kids after it. So is a kid at
    /// the end of a chain of 40 references, which `pdf` follows: the
    /// family's walk sees one hop of it.
    #[test]
    fn a_reference_to_itself_among_the_kids_is_passed() {
        let objects = vec![(10, leaf("a", 1)), (11, r(11)), (12, leaf("b", 2))];
        let tree = Object::from(dictionary! { "Kids" => vec![r(10), r(11), r(12)] });
        assert_eq!(names(file(objects, tree)), ["a=1", "b=2"]);

        let mut objects: Vec<(u32, Object)> = (20..60).map(|n| (n, r(n + 1))).collect();
        objects.push((60, leaf("c", 3)));
        let tree = Object::from(dictionary! { "Kids" => vec![r(20), r(10)] });
        objects.push((10, leaf("a", 1)));
        assert_eq!(names(file(objects, tree)), ["c=3", "a=1"]);
    }

    /// A tree that isn't there, or a root that's null or no dictionary,
    /// gives no entries.
    #[test]
    fn no_tree_is_no_entries() {
        assert!(names(file(Vec::new(), r(10))).is_empty());
        assert!(names(file(Vec::new(), Object::null())).is_empty());
        assert!(names(file(Vec::new(), 7.into())).is_empty());
    }
}
