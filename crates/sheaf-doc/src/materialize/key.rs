//! What the emitter names an object by (M10.5 §11.3): an [`ObjKey`] says
//! what the object is, from the definition, never which number it's
//! written at. A save numbers keys through its [`SaveRecord`]'s map, so a
//! key keeps its number from one save to the next.
//!
//! Only the keys slice 9's emitter writes are here; the other name trees,
//! the generators' blocks, filtered streams and page resources come with
//! what writes them. The legacy OCR layers and header and footer are
//! [`ObjKey::Block`]s by their streams' digest until then.
//!
//! [`SaveRecord`]: super::SaveRecord

use lopdf::ObjectId;

use crate::annot::AnnotId;
use crate::content::ResId;
use crate::forms::FieldId;
use crate::model::{OutlineId, PageId};
use crate::store::BlobId;

/// An object of a written file, by what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ObjKey {
    Catalog,
    /// The nth node of the page tree, in the order the tree is walked:
    /// 0 is the root.
    PagesNode(u32),
    /// The outline's root, the catalog's `/Outlines`.
    OutlinesRoot,
    AcroForm,
    /// The trailer's `/Info`.
    Info,
    /// A page's dictionary. A page of the primary's that's its page's copy
    /// 0 is written at its own number.
    Page(PageId),
    /// An object of the source in blob `blob`, as `copy` of it: written at
    /// its own number when it's the primary's and `copy` is 0, otherwise
    /// copied. For an object a page's annotations reach, `copy` is the
    /// page's [`PageDef::copy`](crate::PageDef::copy), which counts the
    /// copies of a page across every insert of its bytes (for a derived
    /// copy's page, which is written as the page it was made from, the
    /// copy of that page it is); what its content and resources reach is
    /// copy 0, shared by every copy of the page. For one a form instance
    /// reaches, it's the instance's [`FormInst::copy`](crate::FormInst),
    /// fixed as it was made.
    Base {
        blob: BlobId,
        obj: ObjectId,
        copy: u32,
    },
    /// An annotation: a mark, another tool's comment or link, or a widget.
    /// A merged widget's key names the same number as its field's.
    Annot(AnnotId),
    /// An object a mark writes beside its dictionary.
    AnnotPart(AnnotId, Part),
    /// A field's dictionary: for a field merged with its one widget
    /// (ISO 32000-1 12.7.3.1), the one object both are.
    Field(FieldId),
    Bookmark(OutlineId),
    /// The catalog's `/PageLabels`.
    Labels,
    /// A resource content draws with, by its digest: identical ones are
    /// one object.
    Res(ResId),
    Shared(Shared),
    /// An object a resource reaches, by the resource's digest and its
    /// place among the resource's objects (1 the first after the resource
    /// itself): a font's descendant, descriptor and program.
    ResPart(ResId, u32),
    /// A stream of content sheaf draws on a page, by its digest: an OCR
    /// layer or a header and footer, in their legacy shape until slices 17
    /// and 18. Identical ones are one object.
    Block(u128),
    /// A name tree of the catalog's `/Names` a save builds: the document
    /// scripts of forms that came in from other files beside the primary's.
    Names(NameTree),
}

/// What a mark writes beside its dictionary, and another tool's comment
/// its pop-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    /// Its appearance stream, `/AP /N`.
    Ap,
    /// A replacement's StrikeOut, the companion of its Caret.
    Companion,
    /// An attachment's file specification.
    FileSpec,
    /// The companion's appearance stream.
    CompanionAp,
    /// Another tool's comment's pop-up window (`/Popup`), which goes with
    /// it.
    Popup,
}

/// A name tree of the catalog's `/Names`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NameTree {
    /// `/JavaScript`: the document scripts.
    JavaScript,
}

/// An object content shares across pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Shared {
    /// The `q` that isolates a page's own content from what's drawn over
    /// it.
    QOpen,
}

impl ObjKey {
    /// Whether its object is one the state derives: a root object, a page,
    /// an annotation and what it writes beside it, a field or a bookmark.
    /// An Append writes such a key only when it's in scope (M10.5 §7.2);
    /// every other is a resource or a block, by its digest, or an object of
    /// a source, which a file has once.
    pub fn of_state(&self) -> bool {
        match self {
            ObjKey::Catalog
            | ObjKey::PagesNode(_)
            | ObjKey::OutlinesRoot
            | ObjKey::AcroForm
            | ObjKey::Info
            | ObjKey::Page(_)
            | ObjKey::Annot(_)
            | ObjKey::AnnotPart(..)
            | ObjKey::Field(_)
            | ObjKey::Bookmark(_)
            | ObjKey::Labels
            | ObjKey::Names(_) => true,
            ObjKey::Base { .. }
            | ObjKey::Res(_)
            | ObjKey::Shared(_)
            | ObjKey::ResPart(..)
            | ObjKey::Block(_) => false,
        }
    }

    /// Whether it's one of the file's root objects, which the state owns
    /// wherever they're numbered: the catalog, the page tree's nodes, the
    /// outline's root, the form, the page labels and the document
    /// information.
    pub fn is_root(&self) -> bool {
        matches!(
            self,
            ObjKey::Catalog
                | ObjKey::PagesNode(_)
                | ObjKey::OutlinesRoot
                | ObjKey::AcroForm
                | ObjKey::Labels
                | ObjKey::Info
        )
    }

    /// The key as bytes, the same in every build and session: a tag for
    /// the kind, then its fields, big-endian. What an AES IV is keyed by
    /// (§11.7). A tag is never reused: a new kind takes the next.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(35);
        match *self {
            ObjKey::Catalog => out.push(0),
            ObjKey::PagesNode(n) => {
                out.push(1);
                out.extend(n.to_be_bytes());
            }
            ObjKey::OutlinesRoot => out.push(2),
            ObjKey::AcroForm => out.push(3),
            ObjKey::Info => out.push(4),
            ObjKey::Page(id) => {
                out.push(5);
                out.extend(id.0.to_be_bytes());
            }
            ObjKey::Base { blob, obj, copy } => {
                out.push(6);
                out.extend(blob.hash.to_be_bytes());
                out.extend(blob.len.to_be_bytes());
                out.extend(obj.0.to_be_bytes());
                out.extend(obj.1.to_be_bytes());
                out.extend(copy.to_be_bytes());
            }
            ObjKey::Annot(id) => {
                out.push(7);
                out.extend(id.0.to_be_bytes());
            }
            ObjKey::AnnotPart(id, part) => {
                out.push(8);
                out.extend(id.0.to_be_bytes());
                out.push(match part {
                    Part::Ap => 0,
                    Part::Companion => 1,
                    Part::FileSpec => 2,
                    Part::CompanionAp => 3,
                    Part::Popup => 4,
                });
            }
            ObjKey::Field(id) => {
                out.push(9);
                out.extend(id.0.to_be_bytes());
            }
            ObjKey::Bookmark(id) => {
                out.push(10);
                out.extend(id.0.to_be_bytes());
            }
            ObjKey::Labels => out.push(11),
            ObjKey::Res(id) => {
                out.push(12);
                out.extend(id.0.to_be_bytes());
            }
            ObjKey::Shared(Shared::QOpen) => out.extend([13, 0]),
            ObjKey::ResPart(id, part) => {
                out.push(14);
                out.extend(id.0.to_be_bytes());
                out.extend(part.to_be_bytes());
            }
            ObjKey::Block(digest) => {
                out.push(15);
                out.extend(digest.to_be_bytes());
            }
            ObjKey::Names(NameTree::JavaScript) => out.extend([16, 0]),
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::*;
    use crate::history::FixedCount;
    use crate::model::FormInst;
    use crate::store::{BlobKind, Store};
    use crate::tx::fixture as fx;

    /// Every kind of key, two of the kinds that carry an id twice.
    fn every_key() -> Vec<ObjKey> {
        let blob = BlobId { hash: 7, len: 9 };
        vec![
            ObjKey::Catalog,
            ObjKey::PagesNode(0),
            ObjKey::PagesNode(1),
            ObjKey::OutlinesRoot,
            ObjKey::AcroForm,
            ObjKey::Info,
            ObjKey::Page(PageId(1)),
            ObjKey::Page(PageId(2)),
            ObjKey::Base {
                blob,
                obj: (4, 0),
                copy: 0,
            },
            ObjKey::Base {
                blob,
                obj: (4, 0),
                copy: 1,
            },
            ObjKey::Base {
                blob,
                obj: (4, 1),
                copy: 0,
            },
            ObjKey::Annot(AnnotId(1)),
            ObjKey::AnnotPart(AnnotId(1), Part::Ap),
            ObjKey::AnnotPart(AnnotId(1), Part::Companion),
            ObjKey::AnnotPart(AnnotId(1), Part::FileSpec),
            ObjKey::AnnotPart(AnnotId(1), Part::CompanionAp),
            ObjKey::AnnotPart(AnnotId(1), Part::Popup),
            ObjKey::Field(FieldId(1)),
            ObjKey::Bookmark(OutlineId(1)),
            ObjKey::Labels,
            ObjKey::Res(ResId(1)),
            ObjKey::Shared(Shared::QOpen),
            ObjKey::ResPart(ResId(1), 1),
            ObjKey::ResPart(ResId(1), 2),
            ObjKey::Block(1),
            ObjKey::Names(NameTree::JavaScript),
        ]
    }

    /// Two keys never share bytes, even keys of different kinds with the
    /// same number in them, and no key's bytes start another's: an IV
    /// keyed by one is never another's.
    #[test]
    fn every_key_has_bytes_of_its_own() {
        let keys = every_key();
        for (i, a) in keys.iter().enumerate() {
            for b in &keys[i + 1..] {
                let (x, y) = (a.bytes(), b.bytes());
                assert_ne!(x, y, "{a:?} and {b:?}");
                assert!(!y.starts_with(&x) && !x.starts_with(&y), "{a:?} and {b:?}");
            }
        }
    }

    /// The bytes are fixed: a build that wrote others would key another
    /// IV for the same object.
    #[test]
    fn a_key_s_bytes_are_fixed() {
        assert_eq!(ObjKey::Catalog.bytes(), [0]);
        assert_eq!(ObjKey::PagesNode(258).bytes(), [1, 0, 0, 1, 2]);
        assert_eq!(ObjKey::Page(PageId(3)).bytes(), [5, 0, 0, 0, 0, 0, 0, 0, 3]);
        let base = ObjKey::Base {
            blob: BlobId { hash: 1, len: 2 },
            obj: (3, 4),
            copy: 5,
        };
        let mut want = vec![6];
        want.extend(1u128.to_be_bytes());
        want.extend(2u64.to_be_bytes());
        want.extend([0, 0, 0, 3, 0, 4, 0, 0, 0, 5]);
        assert_eq!(base.bytes(), want);
        assert_eq!(ObjKey::Shared(Shared::QOpen).bytes(), [13, 0]);
        let mut want = vec![14];
        want.extend(9u128.to_be_bytes());
        want.extend([0, 0, 0, 2]);
        assert_eq!(ObjKey::ResPart(ResId(9), 2).bytes(), want);
        let mut want = vec![15];
        want.extend(9u128.to_be_bytes());
        assert_eq!(ObjKey::Block(9).bytes(), want);
        assert_eq!(ObjKey::Names(NameTree::JavaScript).bytes(), [16, 0]);
        assert_eq!(
            ObjKey::AnnotPart(AnnotId(2), Part::Popup).bytes(),
            [8, 0, 0, 0, 0, 0, 0, 0, 2, 4]
        );
    }

    /// Each form instance of one file's bytes has a copy of its own, fixed
    /// as it's made, whichever edit brought it: the primary's own instance
    /// 0, a duplicate's and two more inserts of the same file the next
    /// ones, and another file's first instance 0 of its own.
    #[test]
    fn each_insert_of_one_file_is_an_instance_of_its_own() {
        let store = Store::new();
        let bytes = crate::forms::fixture::every_kind().bytes();
        let open = |bytes: &[u8], name: &str| {
            (store.open(bytes.into(), BlobKind::file(Path::new(name), None))).unwrap()
        };
        let primary = open(&bytes, "form.pdf");
        let src = primary.clone();
        let mut d = crate::open::start(
            store.clone(),
            primary,
            "form.pdf".into(),
            None,
            Box::new(FixedCount(10)),
        );
        d.ready(crate::open::finish(&src, &in_progress::NoProgress))
            .unwrap();
        let first = d.page(0).unwrap();
        let duplicate = sheaf_edit::Op::DuplicatePages {
            pages: fx::ids(&[first]),
            locals: Vec::new(),
        };
        d.commit_ops(vec![duplicate], "duplicate").unwrap();
        let other = crate::forms::fixture::every_fact().bytes();
        let mut inserted = Vec::new();
        for (bytes, name) in [
            (&bytes, "again.pdf"),
            (&bytes, "more.pdf"),
            (&other, "o.pdf"),
        ] {
            let src = open(bytes, name);
            inserted.push(src.id());
            let insert = fx::insert(&src, [0], name, None);
            d.commit_ops(vec![insert], "insert").unwrap();
        }
        let inst = |src, copy| FormInst { src, copy };
        let own = d.primary().id();
        assert_eq!(inserted[..2], [own, own]);
        let held: BTreeSet<FormInst> = (d.state().fields()).map(|(_, n)| n.inst).collect();
        assert_eq!(
            held,
            BTreeSet::from([
                inst(own, 0),
                inst(own, 1),
                inst(own, 2),
                inst(own, 3),
                inst(inserted[2], 0),
            ])
        );
        let names: Vec<(FormInst, &str)> = (d.state().form().names.iter())
            .map(|(inst, name)| (*inst, &**name))
            .collect();
        let mut want = vec![
            (inst(own, 0), "form.pdf"),
            (inst(own, 1), "form.pdf"),
            (inst(own, 2), "again.pdf"),
            (inst(own, 3), "more.pdf"),
            (inst(inserted[2], 0), "o.pdf"),
        ];
        want.sort();
        assert_eq!(names, want);
    }
}
