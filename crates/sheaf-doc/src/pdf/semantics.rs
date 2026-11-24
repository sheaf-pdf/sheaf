//! The common structures through `pdf-semantics` (M18 slice 21): the date
//! a mark is made at ([`date`]), name and number trees read and written
//! ([`tree`]), the page tree's walk ([`pages`]) and what a file's XMP
//! metadata claims it conforms to ([`xmp`]), each under PDFium's rules
//! where the family has rules to choose, and under the caller's caps
//! ([`Limits`]). The walks tell what they read past to a sink of
//! [`Fault`]s; sheaf's own callers want none (`()`) but the claims', which
//! tells a packet it can't read from one that claims nothing.
//!
//! What's read is read from a source's objects through
//! [`Resolve`](super::Resolve), and a packet's stream through
//! [`Streams`](super::Streams): `crate::content::Objects` is any of
//! sheaf's sources as both.

pub use pdf_semantics::{Fault, Limits};

/// The date and time by the clock, written as a PDF date (7.9.4).
pub mod date {
    /// Now, in UT, to the second, every field written:
    /// `D:YYYYMMDDHHmmSSZ`.
    #[must_use]
    pub fn now() -> String {
        pdf_semantics::date::Date::now().write()
    }
}

/// Name and number trees (7.9.6, 7.9.7): each entry in the tree's order,
/// every node read once, under [`Limits::tree_depth`] and
/// [`Limits::tree_nodes`]; and a tree written as one root, sorted.
pub mod tree {
    pub use pdf_semantics::tree::{
        Names, Numbers, WriteError, names, numbers, write_names, write_numbers,
    };
}

/// The page tree, walked as PDFium walks it (chromium/8152): each page in
/// PDFium's order, what it inherits, and a place where PDFium loads no
/// page.
pub mod pages {
    use pdf_objects::Rules;

    pub use pdf_semantics::pages::{
        Attribute, Error, Inheritable, Origin, Page, Pages, Parents, PdfBox, Sourced,
    };

    use super::Limits;
    use crate::pdf::{Dictionary, Resolve};

    /// The pages of the tree `catalog`'s `/Pages` names, read from
    /// `objects` under PDFium's rules and `limits`' `page_depth` and
    /// `pages`.
    ///
    /// # Errors
    ///
    /// An [`Error`]: no pages, a tree past the caps, a node inside itself
    /// or a `/Parent` chain that loops, or an object on the way that
    /// `objects` couldn't read.
    pub fn table<'o, R: Resolve<'o>>(
        objects: R,
        catalog: &'o Dictionary,
        limits: &Limits,
    ) -> Result<Pages<'o>, Error> {
        pdf_semantics::pages::pages(objects, catalog, Rules::Pdfium, limits, &mut ())
    }
}

/// What a file's XMP metadata claims (ISO 19005, ISO 14289): PDF/A's
/// part and level, and PDF/UA's part.
pub mod xmp {
    pub use pdf_semantics::xmp::{ClaimError, Claims, Conformance, Error, PdfA, PdfUa, XmpWhy};

    use super::{Fault, Limits};
    use crate::pdf::fault::Sink;
    use crate::pdf::{Dictionary, Resolve, Streams};

    /// The claims of the packet `catalog`'s `/Metadata` names, its stream
    /// decoded by `objects` under `limits`' `xmp_bytes`: none where the
    /// catalog names no stream. [`Claims::conformance`] reads them.
    /// `sink` hears what's read past: an attribute or a claim's value that
    /// doesn't read, which is as though it weren't written, and a packet
    /// whose first bytes say it isn't UTF-8 ([`Fault::XmpNotUtf8`]), of
    /// which nothing is read, so that it claims nothing unless the caller
    /// tells it apart by that.
    ///
    /// # Errors
    ///
    /// An [`Error`]: the stream, or the way to it, doesn't read; it's past
    /// the cap; or its XML stops making sense, the error holding what was
    /// claimed before there.
    // A packet is read once a file, and its error holds what's read.
    #[allow(clippy::result_large_err)]
    pub fn claims<'o, O: Resolve<'o> + Streams, S: Sink<Fault> + ?Sized>(
        objects: O,
        catalog: &'o Dictionary,
        limits: &Limits,
        sink: &mut S,
    ) -> Result<Option<Claims>, Error> {
        pdf_semantics::xmp::read_from(objects, catalog, limits, sink)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pdf_objects::WithStreams;
    use pdf_semantics::date::{Date, Offset};

    use super::*;
    use crate::pdf::{Dictionary, Object, ObjectId, Unreadable, Why, dictionary, literal};

    #[test]
    fn now_reads_back_as_a_date_in_ut_to_the_second() {
        let now = date::now();
        let read = Date::read(&now).unwrap();
        assert!(read.quirks().is_empty(), "{now}: {:?}", read.quirks());
        assert_eq!(read.offset(), Some(Offset::Utc), "{now}");
        assert!(read.second().is_some(), "{now}");
        assert!(read.year() >= 2026, "{now}");
        // D:, the year, five fields of two digits, Z.
        assert_eq!(now.len(), 2 + 4 + 5 * 2 + 1, "{now}");
        assert!(now.starts_with("D:") && now.ends_with('Z'), "{now}");
    }

    fn id(n: u32) -> ObjectId {
        ObjectId::new(n, 0)
    }

    #[test]
    fn a_name_tree_written_reads_back_sorted_by_its_bytes() {
        let written = tree::write_names([
            (b"b".to_vec(), Object::from(2)),
            (b"a".to_vec(), Object::reference(7)),
            (crate::pdf::text::encoded("é"), Object::from(3)),
        ])
        .unwrap();
        let root = Object::from(written);
        let objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
        let read: Vec<(&[u8], &Object)> = tree::names(&objects, &root, &Limits::DEFAULT, ())
            .map(Result::unwrap)
            .collect();
        let keys: Vec<&[u8]> = read.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, [&b"a"[..], b"b", b"\xFE\xFF\x00\xE9"]);
        assert_eq!(read[0].1, &Object::reference(7));

        let twice = tree::write_names([
            (b"a".to_vec(), Object::from(1)),
            (b"a".to_vec(), Object::from(2)),
        ]);
        assert!(matches!(twice, Err(tree::WriteError::RepeatedName { .. })));
    }

    #[test]
    fn a_number_tree_is_read_through_its_kids() {
        let objects = BTreeMap::from([
            (
                id(2),
                Object::from(dictionary! { "Nums" => vec![Object::from(0), literal("i")] }),
            ),
            (
                id(3),
                Object::from(dictionary! { "Nums" => vec![Object::from(4), literal("A")] }),
            ),
        ]);
        let root = Object::from(dictionary! {
            "Kids" => vec![Object::reference(2), Object::reference(3)],
        });
        let read: Vec<(i64, &Object)> = tree::numbers(&objects, &root, &Limits::DEFAULT, ())
            .map(Result::unwrap)
            .collect();
        assert_eq!(read, [(0, &literal("i")), (4, &literal("A"))]);

        let written = tree::write_numbers([(4, literal("A")), (0, literal("i"))]).unwrap();
        assert_eq!(
            written.get(b"Nums").and_then(Object::as_array).unwrap(),
            &[Object::from(0), literal("i"), Object::from(4), literal("A")]
        );
    }

    #[test]
    fn the_page_table_is_pdfium_s() {
        let media = vec![
            Object::from(0),
            Object::from(0),
            Object::from(100),
            Object::from(200),
        ];
        let objects = BTreeMap::from([
            (
                id(2),
                Object::from(dictionary! {
                    "Type" => Object::name("Pages"),
                    "Kids" => vec![Object::reference(3), Object::reference(3), Object::reference(4)],
                    "Count" => 3,
                    "MediaBox" => media,
                    "Resources" => Object::reference(9),
                }),
            ),
            (
                id(3),
                Object::from(dictionary! {
                    "Type" => Object::name("Page"),
                    "Parent" => Object::reference(2),
                    "Rotate" => 90,
                }),
            ),
            // A leaf of the wrong type: a place where PDFium loads no page.
            (
                id(4),
                Object::from(dictionary! {
                    "Type" => Object::name("Font"),
                    "Parent" => Object::reference(2),
                }),
            ),
            (id(9), Object::from(Dictionary::new())),
        ]);
        let catalog = dictionary! { "Pages" => Object::reference(2) };
        let table = pages::table(&objects, &catalog, &Limits::DEFAULT).unwrap();
        assert_eq!((table.len(), table.counted()), (3, 3));
        let [first, second, third] = [0, 1, 2].map(|i| table.get(i).unwrap());
        assert_eq!((first.id, first.repeat, second.repeat), (Some(id(3)), 0, 1));
        assert_eq!(second.rotate().value, 90);
        assert_eq!(
            second.media_box().value,
            pages::PdfBox([0.0, 0.0, 100.0, 200.0])
        );
        assert_eq!(
            second.media_box().origin,
            pages::Origin::Ancestor(Some(id(2)))
        );
        assert_eq!(second.parents().collect::<Vec<_>>(), [id(2)]);
        let resources = second.attribute(pages::Inheritable::Resources).unwrap();
        assert_eq!(resources.written, &Object::reference(9));
        assert_eq!((third.id, third.dict), (Some(id(4)), None));
        assert_eq!(third.media_box().value, pages::PdfBox::LETTER);

        let none = dictionary! { "Type" => Object::name("Catalog") };
        let error = pages::table(&objects, &none, &Limits::DEFAULT).unwrap_err();
        assert_eq!(error, pages::Error::NoPages);
    }

    /// A packet claiming PDF/A-2b in attributes and PDF/UA-1 in an
    /// element, behind a catalog's `/Metadata`.
    const PACKET: &[u8] = br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" pdfaid:part="2" pdfaid:conformance="B"/>
<rdf:Description rdf:about="" xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/"><pdfuaid:part>1</pdfuaid:part></rdf:Description>
</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;

    #[test]
    fn claims_are_read_from_the_catalog_s_metadata() {
        let stream = Object::stream(dictionary! {
            "Type" => Object::name("Metadata"),
            "Subtype" => Object::name("XML"),
        });
        let objects = BTreeMap::from([(id(5), stream)]);
        let file = WithStreams::new(&objects, |id: ObjectId, _: usize| match id.number {
            5 => Ok(PACKET.to_vec()),
            _ => Err(Unreadable::new(
                id,
                Why::Damaged,
                format!("{id} R isn't there"),
            )),
        });
        let catalog = dictionary! { "Metadata" => Object::reference(5) };
        let claims = xmp::claims(file, &catalog, &Limits::DEFAULT, &mut ())
            .unwrap()
            .unwrap();
        let conformance = claims.conformance().unwrap();
        assert_eq!(conformance.pdfa.unwrap(), xmp::PdfA::new(2, Some('b')));
        assert_eq!(conformance.pdfa.unwrap().to_string(), "PDF/A-2b");
        assert_eq!(conformance.pdfua.unwrap(), xmp::PdfUa::new(1));
        assert_eq!(conformance.pdfua.unwrap().to_string(), "PDF/UA-1");

        let none = dictionary! { "Type" => Object::name("Catalog") };
        assert_eq!(
            xmp::claims(file, &none, &Limits::DEFAULT, &mut ()).unwrap(),
            None
        );

        let mut small = Limits::DEFAULT;
        small.xmp_bytes = 16;
        let error = xmp::claims(file, &catalog, &small, &mut ()).unwrap_err();
        assert_eq!(error.why, xmp::XmpWhy::TooLong);
    }
}
