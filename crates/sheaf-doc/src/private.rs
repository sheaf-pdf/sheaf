//! sheaf's private data in the files it writes (M10.5 §15): every name it
//! writes, or wrote before M10.5, that only sheaf reads, where each
//! appears and how Strip takes it out ([`KEYS`]); the neutral name a
//! sheaf mark is written with under Strip ([`neutral_nm`]); and what of
//! them a file holds ([`found`]), which a Strip output's post-condition
//! fails on (§11.9). Everything else sheaf writes is standard keys.
//!
//! A stamp library's collection files (`/SheafStampText`,
//! [`crate::stamp_lib`]) are the user's own library, outside the edit API
//! and this table, and keep their format (proposal 29): a stamp placed
//! from one is its page's content and resources, without the page's keys.

use std::collections::BTreeSet;

use lopdf::{Dictionary, Document as Pdf, Object, ObjectId};

use crate::content::{self, ContentFault, PageContent, ResCx, Tok};
use crate::source::Limits;

/// Where a private name appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// An annotation's `/NM`: the start of its value.
    Nm,
    /// An annotation's dictionary: a key.
    Annot,
    /// The catalog: a key.
    Catalog,
    /// A content stream's dictionary: a key.
    Stream,
    /// A marked range's property list, written in the content or named in
    /// the resources' `/Properties`: a key.
    List,
    /// A resource's name in a page's resources, and in the content that
    /// draws with it.
    Resource,
    /// A stamp's `/Name`: its value.
    StampName,
}

/// One of sheaf's private names.
#[derive(Debug, Clone, Copy)]
pub struct Key {
    /// The name, or for [`Place::Nm`] the text a value starts with.
    pub name: &'static [u8],
    /// Whether every name it starts is one (`/SheafFlat1`, `/SheafWMgs`).
    pub prefix: bool,
    pub places: &'static [Place],
    /// What an older sheaf wrote, before M10.5: read, never written.
    pub older: bool,
    /// How Strip takes it out.
    pub strip: &'static str,
}

/// sheaf's private names (§15), those written now first, then an older sheaf's.
pub const KEYS: &[Key] = &[
    Key {
        name: b"sheaf:",
        prefix: true,
        places: &[Place::Nm],
        older: false,
        strip: "a sheaf mark's name, and its StrikeOut's, written neutral (`neutral_nm`)",
    },
    Key {
        name: b"sheaf-anon:",
        prefix: true,
        places: &[Place::Nm],
        older: false,
        strip: "the name a comment without one is given when a reply made in sheaf names it, \
                written neutral",
    },
    Key {
        name: b"SHEF_Annot",
        prefix: false,
        places: &[Place::Annot],
        older: false,
        strip: "the mark's key left out",
    },
    Key {
        name: b"SHEF_Generators",
        prefix: false,
        places: &[Place::Catalog],
        older: false,
        strip: "the catalog's key left out",
    },
    Key {
        name: b"SHEF_Block",
        prefix: false,
        places: &[Place::Stream],
        older: false,
        strip: "a block drawn now isn't marked; a file's block is copied without it \
                (`LegacySel::Streams`)",
    },
    Key {
        name: b"SHEF_Gen",
        prefix: false,
        places: &[Place::List],
        older: false,
        strip: "a generator's list drawn now is written without it; a file's is written again \
                without it (`LegacySel::Resources`, `LegacySel::Marked`)",
    },
    Key {
        name: b"SHEF_OcrBlock",
        prefix: false,
        places: &[Place::List],
        older: false,
        strip: "recognized text drawn now is marked with no list; a file's list is written \
                again without it",
    },
    Key {
        name: b"SheafStamp",
        prefix: false,
        places: &[Place::List, Place::Resource],
        older: true,
        strip: "left out of a list; as a stamp's form's name, the stamp is written anew",
    },
    Key {
        name: b"SheafSettings",
        prefix: false,
        places: &[Place::List],
        older: true,
        strip: "left out of a list (`LegacySel::Marked`)",
    },
    Key {
        name: b"SheafHF",
        prefix: false,
        places: &[Place::Resource],
        older: true,
        strip: "renamed `/Sh…` (`LegacySel::Resources`)",
    },
    Key {
        name: b"SheafHeaderFooter",
        prefix: false,
        places: &[Place::Catalog],
        older: true,
        strip: "the catalog's key, owned and never written (`CATALOG_OWNED`)",
    },
    Key {
        name: b"SheafPart",
        prefix: false,
        places: &[Place::Annot],
        older: true,
        strip: "a mark's companion it marks is written anew with the mark",
    },
    Key {
        name: b"SheafOcrLayer",
        prefix: false,
        places: &[Place::Stream],
        older: true,
        strip: "the stream copied without it (`LegacySel::Streams`)",
    },
    Key {
        name: b"SheafTick",
        prefix: true,
        places: &[Place::StampName],
        older: true,
        strip: "a tick's `/Name`, written `/Tick`",
    },
    Key {
        name: b"SheafOcr",
        prefix: false,
        places: &[Place::Resource],
        older: true,
        strip: "renamed `/Sh…`",
    },
    Key {
        name: b"SheafFlat",
        prefix: true,
        places: &[Place::Resource],
        older: true,
        strip: "renamed `/Sh…`",
    },
    Key {
        name: b"SheafWM",
        prefix: true,
        places: &[Place::Resource],
        older: true,
        strip: "renamed `/Sh…`",
    },
];

/// The keys Strip takes out of property lists.
pub const LIST_KEYS: [&[u8]; count(Place::List)] = names(Place::List);

/// The keys Strip takes out of content streams' dictionaries.
pub const STREAM_KEYS: [&[u8]; count(Place::Stream)] = names(Place::Stream);

/// How many of [`KEYS`] appear in `place`.
const fn count(place: Place) -> usize {
    let (mut n, mut i) = (0, 0);
    while i < KEYS.len() {
        if in_place(&KEYS[i], place) {
            n += 1;
        }
        i += 1;
    }
    n
}

/// The names of [`KEYS`] that appear in `place`, in order: `N` of them.
const fn names<const N: usize>(place: Place) -> [&'static [u8]; N] {
    let mut out: [&[u8]; N] = [b""; N];
    let (mut n, mut i) = (0, 0);
    while i < KEYS.len() {
        if in_place(&KEYS[i], place) {
            out[n] = KEYS[i].name;
            n += 1;
        }
        i += 1;
    }
    out
}

const fn in_place(key: &Key, place: Place) -> bool {
    let mut i = 0;
    while i < key.places.len() {
        if key.places[i] as u8 == place as u8 {
            return true;
        }
        i += 1;
    }
    false
}

impl Key {
    /// Whether it is `text`, or starts it.
    fn names(&self, text: &[u8]) -> bool {
        match self.prefix {
            true => text.starts_with(self.name),
            false => text == self.name,
        }
    }
}

/// Whether `name`, a name or a dictionary's key anywhere, is one of
/// sheaf's private names ([`KEYS`] but the `/NM` values).
pub fn is_private(name: &[u8]) -> bool {
    (KEYS.iter()).any(|k| k.places != [Place::Nm] && k.names(name))
}

/// Whether `name` is a name an older sheaf gave a resource, which Strip
/// renames.
pub fn is_resource_name(name: &[u8]) -> bool {
    (KEYS.iter()).any(|k| in_place(k, Place::Resource) && k.names(name))
}

/// Whether `nm`, an annotation's `/NM`, is a name sheaf gave it.
pub fn is_sheaf_nm(nm: &[u8]) -> bool {
    (KEYS.iter()).any(|k| k.places == [Place::Nm] && k.names(nm))
}

/// The neutral name a sheaf mark named `nm` is written with under Strip:
/// a UUID's form, its 32 hex digits a digest of `nm`, as other tools name
/// their comments. The same mark is named alike in every export, and two
/// marks never alike, as their sheaf names never are; nothing of sheaf's
/// name is in it.
pub fn neutral_nm(nm: &str) -> String {
    let hex = sheaf_util::checksum_hex(nm.as_bytes());
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// Whether annotation dictionary `dict` holds sheaf's private data: a key
/// of [`KEYS`], a name sheaf gave it, or an older sheaf's tick's `/Name`.
pub(crate) fn in_annot(dict: &Dictionary) -> bool {
    let nm = dict.get(b"NM").ok().and_then(|nm| nm.as_str().ok());
    let name = dict.get(b"Name").ok().and_then(|n| n.as_name().ok());
    (dict.iter()).any(|(key, _)| is_private(key))
        || nm.is_some_and(is_sheaf_nm)
        || name.is_some_and(is_private)
}

/// `dict`, an annotation's dictionary as written, without sheaf's private
/// data: its keys of [`KEYS`] left out, the name sheaf gave it neutral
/// ([`neutral_nm`]), and an older sheaf's tick's `/Name` a tick's, as
/// sheaf names one now.
pub(crate) fn strip_annot(dict: &mut Dictionary) {
    let keys: Vec<Vec<u8>> = (dict.iter())
        .filter(|(key, _)| is_private(key))
        .map(|(key, _)| key.clone())
        .collect();
    for key in keys {
        dict.remove(&key);
    }
    if let Ok(nm) = dict.get(b"NM").and_then(Object::as_str)
        && is_sheaf_nm(nm)
    {
        let neutral = neutral_nm(&String::from_utf8_lossy(nm));
        dict.set("NM", Object::string_literal(neutral));
    }
    if dict
        .get(b"Name")
        .and_then(Object::as_name)
        .is_ok_and(is_private)
    {
        dict.set("Name", Object::Name(b"Tick".to_vec()));
    }
}

/// A private name a file holds: where, and what. `at` is the object whose
/// dictionary or content holds it, a page for its content.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Found {
    pub at: ObjectId,
    pub name: Vec<u8>,
}

/// sheaf's private names `pdf` holds ([`KEYS`]): a dictionary's key or a
/// name in any object, an annotation's `/NM` that sheaf gave it, and a
/// name or a property list's key in the content of a page, a form or a
/// tiling pattern. Content that can't be read whole is read a stream at a
/// time, and what can't be read fails closed: content that can't be lexed,
/// and a stream whose data can't be decoded (lopdf hands back what of a
/// broken deflated stream inflates) are searched for `/SHEF_` and `/Sheaf`
/// as bytes,
/// and a stream that decodes past the content limit is taken to hold
/// some.
pub fn found(pdf: &Pdf) -> Vec<Found> {
    let mut found = BTreeSet::new();
    walk(pdf, &mut |at, said| {
        let hit = match said {
            Said::Name(name) => is_private(name),
            Said::Nm(nm) => is_sheaf_nm(nm),
            Said::Unread => true,
        };
        if hit {
            let name = match said {
                Said::Name(n) | Said::Nm(n) => n.to_vec(),
                Said::Unread => b"/Sheaf".to_vec(),
            };
            found.insert(Found { at, name });
        }
    });
    found.into_iter().collect()
}

/// Every name `pdf` holds that starts `SHEF_` or `Sheaf`, wherever
/// [`found`] looks: what §15's test holds to [`KEYS`].
#[cfg(test)]
pub(crate) fn sheaf_names(pdf: &Pdf) -> BTreeSet<Vec<u8>> {
    let mut names = BTreeSet::new();
    walk(pdf, &mut |_, said| match said {
        Said::Name(name) if name.starts_with(b"SHEF_") || name.starts_with(b"Sheaf") => {
            names.insert(name.to_vec());
        }
        Said::Unread => {
            names.insert(b"Sheaf?".to_vec());
        }
        _ => {}
    });
    names
}

/// What [`walk`] finds.
#[derive(Clone, Copy)]
enum Said<'a> {
    /// A name, or a dictionary's key.
    Name(&'a [u8]),
    /// An annotation's `/NM`.
    Nm(&'a [u8]),
    /// Content that can't be read, holding `/SHEF_` or `/Sheaf`.
    Unread,
}

/// Each name and dictionary key of `pdf`'s objects, each annotation's
/// `/NM`, and each name and inline property list's key in the content of
/// its pages, forms and tiling patterns, handed to `f` with the object
/// that holds it.
fn walk(pdf: &Pdf, f: &mut dyn FnMut(ObjectId, Said)) {
    let lim = Limits::DEFAULT;
    for (&id, obj) in &pdf.objects {
        object(id, obj, f);
        let Ok(dict) = obj.as_dict().or_else(|_| obj.as_stream().map(|s| &s.dict)) else {
            continue;
        };
        if let Ok(Object::String(nm, _)) = dict.get(b"NM") {
            f(id, Said::Nm(nm));
        }
        let is = |key: &[u8], value: &[u8]| {
            dict.get(key)
                .and_then(Object::as_name)
                .is_ok_and(|n| n == value)
        };
        let streams = match obj {
            _ if is(b"Type", b"Page") => content::contents(dict),
            Object::Stream(_) if is(b"Subtype", b"Form") || dict.has(b"PatternType") => vec![id],
            _ => continue,
        };
        let resources = dict.get(b"Resources").ok();
        let res = ResCx::read(pdf, resources);
        if let Ok(read) = PageContent::read(pdf, &streams, &lim) {
            lexed(id, read.concat().bytes(), (&res, &lim), f);
            continue;
        }
        // A stream that can't be decoded, or content past the limit, stops
        // a read of them all: each is read alone, and one that can't be is
        // searched as written, one past the limit can't be told.
        for &stream in &streams {
            match PageContent::read(pdf, &[stream], &lim) {
                Ok(read) => lexed(id, read.concat().bytes(), (&res, &lim), f),
                Err(ContentFault::TooLarge(_)) => f(id, Said::Unread),
                Err(_) => {
                    let raw = pdf.get_object(stream).and_then(Object::as_stream);
                    if raw.is_ok_and(|raw| says_sheaf(&raw.content)) {
                        f(id, Said::Unread);
                    }
                }
            }
        }
    }
}

/// Each name and inline property list's key in content `bytes`, `at`'s,
/// handed to `f`; content that can't be lexed is searched for `/SHEF_` and
/// `/Sheaf` as bytes.
fn lexed(
    at: ObjectId,
    bytes: &[u8],
    (res, lim): (&ResCx, &Limits),
    f: &mut dyn FnMut(ObjectId, Said),
) {
    for token in content::lex(bytes, res, lim) {
        let operand = match token {
            Ok((_, Tok::Operand(operand))) => operand,
            Ok(_) => continue,
            Err(_) => {
                if says_sheaf(bytes) {
                    f(at, Said::Unread);
                }
                break;
            }
        };
        if let Some(name) = content::name(operand) {
            f(at, Said::Name(&name));
        } else if operand.starts_with(b"<<")
            && let Some(list) = content::object(operand, lim.content.depth)
        {
            object(at, &list, f);
        }
    }
}

/// Whether `bytes` hold `/SHEF_` (a name sheaf writes) or `/Sheaf` (an older
/// sheaf's).
fn says_sheaf(bytes: &[u8]) -> bool {
    bytes.windows(6).any(|w| w == b"/SHEF_" || w == b"/Sheaf")
}

/// Each name and dictionary key in `obj`, at any depth, handed to `f`
/// as `at`'s.
fn object(at: ObjectId, obj: &Object, f: &mut dyn FnMut(ObjectId, Said)) {
    match obj {
        Object::Name(name) => f(at, Said::Name(name)),
        Object::Array(items) => items.iter().for_each(|o| object(at, o, f)),
        Object::Dictionary(dict) => entries(at, dict, f),
        Object::Stream(stream) => entries(at, &stream.dict, f),
        _ => {}
    }
}

fn entries(at: ObjectId, dict: &Dictionary, f: &mut dyn FnMut(ObjectId, Said)) {
    for (key, value) in dict.iter() {
        f(at, Said::Name(key));
        object(at, value, f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Stream, dictionary};

    /// The table derives what Strip takes out of lists and streams, and
    /// matches names exactly or by prefix as it says.
    #[test]
    fn the_table_says_what_is_private() {
        assert_eq!(
            LIST_KEYS,
            [
                &b"SHEF_Gen"[..],
                b"SHEF_OcrBlock",
                b"SheafStamp",
                b"SheafSettings"
            ]
        );
        assert_eq!(STREAM_KEYS, [&b"SHEF_Block"[..], b"SheafOcrLayer"]);
        for name in [
            &b"SHEF_Annot"[..],
            b"SheafOcr",
            b"SheafOcrLayer",
            b"SheafFlat1",
            b"SheafWMgs",
            b"SheafTick12",
            b"SheafHeaderFooter",
        ] {
            assert!(is_private(name), "{}", String::from_utf8_lossy(name));
        }
        for name in [&b"SheafStampText"[..], b"SheafOcrX", b"Sheaf", b"ShABCDEF"] {
            assert!(!is_private(name), "{}", String::from_utf8_lossy(name));
        }
        assert!(is_resource_name(b"SheafWMim") && is_resource_name(b"SheafHF"));
        assert!(!is_resource_name(b"SHEF_Gen"));
        assert!(is_sheaf_nm(b"sheaf:12") && is_sheaf_nm(b"sheaf-anon:ab"));
        assert!(!is_sheaf_nm(b"sheafy") && !is_sheaf_nm(b"a1b2-c3"));
        // Every name sheaf writes is a second-class name (ISO 32000-2 Annex
        // E): its prefix, `SHEF_`, then the rest.
        for key in KEYS.iter().filter(|k| !k.older && k.places != [Place::Nm]) {
            let name = String::from_utf8_lossy(key.name);
            assert!(key.name.starts_with(b"SHEF_"), "{name}");
            assert_eq!(
                crate::arlington::Class::of(key.name),
                crate::arlington::Class::Second,
                "{name}"
            );
        }
    }

    /// A neutral name is a UUID's form, the same for the same name, and
    /// another for another.
    #[test]
    fn a_neutral_name_says_nothing_of_sheaf() {
        let one = neutral_nm("sheaf:12");
        assert_eq!(one, neutral_nm("sheaf:12"));
        assert_ne!(one, neutral_nm("sheaf:12:strike"));
        let parts: Vec<usize> = one.split('-').map(str::len).collect();
        assert_eq!(parts, [8, 4, 4, 4, 12]);
        assert!(!one.contains("sheaf") && !is_sheaf_nm(one.as_bytes()));
    }

    /// What's found: keys and names anywhere, a sheaf mark's `/NM`, and in
    /// a page's content a name and an inline list's key; a stream not
    /// content isn't lexed.
    #[test]
    fn found_reads_dictionaries_names_and_content() {
        let mut pdf = Pdf::with_version("1.7");
        let content = pdf.add_object(Stream::new(
            dictionary! { "SheafOcrLayer" => true },
            b"/Artifact <</SheafSettings (x) /Type /Pagination>> BDC /SheafHF 9 Tf (Sheaf) Tj EMC"
                .to_vec(),
        ));
        let image = pdf.add_object(Stream::new(
            dictionary! { "Subtype" => "Image" },
            b"/SheafWMim Do".to_vec(),
        ));
        let page = pdf.add_object(dictionary! { "Type" => "Page", "Contents" => content });
        let annot = pdf.add_object(dictionary! {
            "NM" => Object::string_literal("sheaf:7"), "SHEF_Annot" => Object::string_literal("{}"),
        });
        let at = |at, name: &[u8]| Found {
            at,
            name: name.to_vec(),
        };
        assert_eq!(
            found(&pdf),
            [
                at(content, b"SheafOcrLayer"),
                at(page, b"SheafHF"),
                at(page, b"SheafSettings"),
                at(annot, b"SHEF_Annot"),
                at(annot, b"sheaf:7"),
            ]
        );
        let names = sheaf_names(&pdf);
        assert!(names.contains(b"SheafHF".as_slice()));
        assert!(
            !names.contains(b"SheafWMim".as_slice()),
            "{image:?} isn't content"
        );
    }

    /// Content that can't be lexed is found to hold private data when its
    /// bytes say `/SHEF_` or `/Sheaf`, and not otherwise.
    #[test]
    fn content_that_cant_be_read_fails_closed() {
        let unread = |data: &[u8]| {
            let mut pdf = Pdf::with_version("1.7");
            let content = pdf.add_object(Stream::new(dictionary! {}, data.to_vec()));
            let page = pdf.add_object(dictionary! { "Type" => "Page", "Contents" => content });
            (page, found(&pdf))
        };
        let want = |at| Found {
            at,
            name: b"/Sheaf".to_vec(),
        };
        let (page, said) = unread(b"BT (never ends /SheafHF 9 Tf");
        assert_eq!(said, [want(page)]);
        let (page, said) = unread(b"BT (never ends /SHEF_Gen 9 Tf");
        assert_eq!(said, [want(page)]);
        assert_eq!(unread(b"BT (never ends /F1 9 Tf").1, []);
    }

    /// A page with a stream whose data can't be decoded is read a stream
    /// at a time: an older sheaf's list in another of its streams is found,
    /// and the stream that can't be is found to hold private data when its
    /// bytes say `/Sheaf`, and not otherwise.
    #[test]
    fn a_stream_that_cant_be_decoded_fails_closed() {
        let page_of = |undecoded: &[u8]| {
            let mut pdf = Pdf::with_version("1.7");
            let older = pdf.add_object(Stream::new(
                dictionary! {},
                b"/Artifact <</SheafSettings (x)>> BDC 0 0 9 9 re f EMC".to_vec(),
            ));
            let filter = dictionary! { "Filter" => "JBIG2Decode" };
            let undecoded = pdf.add_object(Stream::new(filter, undecoded.to_vec()));
            let contents = vec![older.into(), undecoded.into()];
            let page = pdf.add_object(dictionary! { "Type" => "Page", "Contents" => contents });
            assert!(PageContent::read(&pdf, &[older, undecoded], &Limits::DEFAULT).is_err());
            (page, found(&pdf))
        };
        let at = |at, name: &[u8]| Found {
            at,
            name: name.to_vec(),
        };
        let (page, said) = page_of(b"/F1 9 Tf");
        assert_eq!(said, [at(page, b"SheafSettings")]);
        let (page, said) = page_of(b"/SheafHF 9 Tf");
        assert_eq!(said, [at(page, b"/Sheaf"), at(page, b"SheafSettings")]);
    }
}
