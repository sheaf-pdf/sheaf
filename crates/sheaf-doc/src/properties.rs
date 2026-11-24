//! File > Properties: what a file says about itself. Read from its source
//! when the dialog opens (the font list walks every page's resources), not
//! when the file does.

use std::collections::HashSet;

use crate::pdf::{Dictionary, LazyPdf, Object, ObjectId, text};

use crate::attachments::resolve;
use crate::source::{PageFacts, Source};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Properties {
    /// The header's version, or the catalog's `/Version` when newer.
    pub version: String,
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    /// The program that made the original document, and the one that
    /// made the PDF.
    pub creator: String,
    pub producer: String,
    /// PDF date strings.
    pub created: String,
    pub modified: String,
    pub pages: u32,
    /// `/MarkInfo /Marked`: a tagged (accessible) PDF.
    pub tagged: bool,
    pub security: Option<Security>,
    pub fonts: Vec<Font>,
    /// The font walk stopped early (a huge file).
    pub fonts_truncated: bool,
    /// What the file's form is: its class, fields, scripts, usage rights
    /// and signatures.
    pub form: crate::forms::FormFacts,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Security {
    /// "Password (AES-256)" and the like.
    pub method: String,
    /// What the file allows, as (what, allowed).
    pub permissions: Vec<(&'static str, bool)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    /// `/BaseFont`, without a subset tag.
    pub name: String,
    /// `/Subtype`: Type1, TrueType, Type0, Type3…
    pub kind: String,
    pub embedding: Embedding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Embedding {
    Embedded,
    Subset,
    NotEmbedded,
}

/// Pages walked for fonts before giving up.
const FONT_PAGE_LIMIT: usize = 5_000;

/// The properties of `src`: its strings decrypted as it's read, its
/// security from the `/Encrypt` it keeps. Fast Web View isn't one: the
/// dialog reads it from the file it shows, the saved record's when a save
/// wrote it ([`crate::pdf::XrefFacts::linearized`]).
pub fn read(src: &Source) -> Properties {
    let pdf = src.pdf();
    let info = pdf
        .trailer()
        .get(b"Info")
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_dict);
    let text = |k: &[u8]| {
        info.and_then(|d| d.get(k))
            .and_then(|o| resolve(pdf, o))
            .map(text::of)
            .unwrap_or_default()
    };
    let catalog = pdf.catalog().ok();
    let mut version = pdf.version().to_string();
    if let Some(v) = catalog
        .and_then(|c| c.get(b"Version"))
        .and_then(Object::as_name)
    {
        let v = String::from_utf8_lossy(v).into_owned();
        if v > version {
            version = v;
        }
    }
    let tagged = catalog
        .and_then(|c| c.get(b"MarkInfo"))
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_dict)
        .and_then(|m| m.get(b"Marked"))
        .and_then(Object::as_bool)
        .unwrap_or(false);
    let (fonts, fonts_truncated) = fonts_in(pdf, src.pages());
    Properties {
        version,
        title: text(b"Title"),
        author: text(b"Author"),
        subject: text(b"Subject"),
        keywords: text(b"Keywords"),
        creator: text(b"Creator"),
        producer: text(b"Producer"),
        created: text(b"CreationDate"),
        modified: text(b"ModDate"),
        pages: src.page_count(),
        tagged,
        security: pdf.is_encrypted().then(|| security_of(pdf)),
        fonts,
        fonts_truncated,
        form: src.form().facts.clone(),
    }
}

fn security_of(pdf: &LazyPdf) -> Security {
    let enc = pdf
        .trailer()
        .get(b"Encrypt")
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_dict);
    let int = |k: &[u8]| enc.and_then(|d| d.get(k)).and_then(Object::as_integer);
    let filter = enc
        .and_then(|d| d.get(b"Filter"))
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .unwrap_or_default();
    let cipher = match int(b"V") {
        Some(5) => "AES-256".to_string(),
        Some(4) => {
            let aes = enc
                .and_then(|d| d.get(b"CF"))
                .and_then(Object::as_dict)
                .and_then(|cf| cf.get(b"StdCF"))
                .and_then(Object::as_dict)
                .and_then(|s| s.get(b"CFM"))
                .and_then(Object::as_name)
                == Some(b"AESV2");
            if aes { "AES-128" } else { "RC4 128-bit" }.to_string()
        }
        Some(2) => format!("RC4 {}-bit", int(b"Length").unwrap_or(40)),
        Some(1) => "RC4 40-bit".to_string(),
        _ => "unknown cipher".to_string(),
    };
    let method = if filter == "Standard" || filter.is_empty() {
        format!("Password ({cipher})")
    } else {
        format!("{filter} ({cipher})")
    };
    // `/P`: bit 3 print, 4 modify, 5 copy, 6 annotate, 9 fill forms,
    // 10 accessibility, 11 assemble, 12 high-quality print (1-based).
    let p = int(b"P").unwrap_or(-1) as i32;
    let bit = |n: u32| p & (1 << (n - 1)) != 0;
    Security {
        method,
        permissions: vec![
            ("Printing", bit(3)),
            ("High-quality printing", bit(3) && bit(12)),
            ("Changing the document", bit(4)),
            ("Assembling pages", bit(4) || bit(11)),
            ("Copying text and images", bit(5)),
            ("Text access for accessibility", bit(5) || bit(10)),
            ("Commenting", bit(6)),
            ("Filling form fields", bit(6) || bit(9)),
        ],
    }
}

/// Fonts used by these pages (and the forms they draw), once each: each
/// page's resources as the page table has them, its own or inherited.
fn fonts_in(pdf: &LazyPdf, pages: &[PageFacts]) -> (Vec<Font>, bool) {
    let mut seen_fonts: HashSet<ObjectId> = HashSet::new();
    let mut seen_res: HashSet<ObjectId> = HashSet::new();
    let mut fonts: Vec<Font> = Vec::new();
    let mut truncated = false;
    for (n, page) in pages.iter().enumerate() {
        if n >= FONT_PAGE_LIMIT {
            truncated = true;
            break;
        }
        if let Some(res) = page
            .resources
            .as_deref()
            .and_then(|o| resolve(pdf, o))
            .and_then(Object::as_dict)
        {
            walk_resources(pdf, res, &mut seen_fonts, &mut seen_res, &mut fonts, 0);
        }
    }
    fonts.sort_by(|a, b| a.name.cmp(&b.name).then(a.kind.cmp(&b.kind)));
    fonts.dedup();
    (fonts, truncated)
}

fn walk_resources(
    pdf: &LazyPdf,
    res: &Dictionary,
    seen_fonts: &mut HashSet<ObjectId>,
    seen_res: &mut HashSet<ObjectId>,
    out: &mut Vec<Font>,
    depth: u8,
) {
    if depth > 8 {
        return;
    }
    if let Some(fonts) = res
        .get(b"Font")
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_dict)
    {
        for (_, f) in fonts.iter() {
            if let Some(id) = f.as_reference()
                && !seen_fonts.insert(id)
            {
                continue;
            }
            if let Some(d) = resolve(pdf, f).and_then(Object::as_dict) {
                out.push(describe_font(pdf, d));
            }
        }
    }
    // Form XObjects (and their own resources) draw text too.
    if let Some(xobjects) = res
        .get(b"XObject")
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_dict)
    {
        for (_, x) in xobjects.iter() {
            let Some(id) = x.as_reference() else { continue };
            if !seen_res.insert(id) {
                continue;
            }
            let Some(s) = pdf.get_object(id).ok().and_then(Object::as_stream) else {
                continue;
            };
            if s.dict.get(b"Subtype").and_then(Object::as_name) != Some(b"Form") {
                continue;
            }
            if let Some(r) = s
                .dict
                .get(b"Resources")
                .and_then(|o| resolve(pdf, o))
                .and_then(Object::as_dict)
            {
                walk_resources(pdf, r, seen_fonts, seen_res, out, depth + 1);
            }
        }
    }
}

fn describe_font(pdf: &LazyPdf, d: &Dictionary) -> Font {
    let name_of = |d: &Dictionary, k: &[u8]| {
        d.get(k)
            .and_then(|o| resolve(pdf, o))
            .and_then(Object::as_name)
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .unwrap_or_default()
    };
    let kind = name_of(d, b"Subtype");
    let base = name_of(d, b"BaseFont");
    // Type0 fonts keep their descriptor on the descendant.
    let desc_owner = if kind == "Type0" {
        d.get(b"DescendantFonts")
            .and_then(|o| resolve(pdf, o))
            .and_then(Object::as_array)
            .and_then(|a| a.first())
            .and_then(|o| resolve(pdf, o))
            .and_then(Object::as_dict)
            .unwrap_or(d)
    } else {
        d
    };
    let has_file = desc_owner
        .get(b"FontDescriptor")
        .and_then(|o| resolve(pdf, o))
        .and_then(Object::as_dict)
        .is_some_and(|fd| {
            [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"]
                .iter()
                .any(|k| fd.has(k))
        });
    let subset = base.len() > 7
        && base.as_bytes()[6] == b'+'
        && base[..6].bytes().all(|c| c.is_ascii_uppercase());
    let embedding = if kind == "Type3" || (has_file && !subset) {
        Embedding::Embedded
    } else if has_file || subset {
        Embedding::Subset
    } else {
        Embedding::NotEmbedded
    };
    let name = if subset { base[7..].to_string() } else { base };
    Font {
        name: if name.is_empty() {
            "(unnamed)".into()
        } else {
            name
        },
        kind,
        embedding,
    }
}

#[cfg(test)]
mod tests {
    use crate::pdf::test_crypt::{CipherKind, TestEncryption};
    use crate::pdf::{Document as Pdf, Permissions, Stream, dictionary, literal};

    use super::*;
    use crate::store::Src;

    /// Two pages: the first with its own fonts, one in a form it draws;
    /// the second with its parent's.
    fn sample() -> Pdf {
        let mut pdf = Pdf::with_version("1.4");
        let file = pdf.add_object(Stream::new(dictionary! {}, vec![0u8; 4]));
        let desc = pdf.add_object(dictionary! {
            "Type" => Object::name("FontDescriptor"),
            "FontFile2" => file,
        });
        let embedded = pdf.add_object(dictionary! {
            "Type" => Object::name("Font"),
            "Subtype" => Object::name("TrueType"),
            "BaseFont" => Object::name("ABCDEF+Arial-Bold"),
            "FontDescriptor" => desc,
        });
        let helv = pdf.add_object(dictionary! {
            "Type" => Object::name("Font"),
            "Subtype" => Object::name("Type1"),
            "BaseFont" => Object::name("Helvetica"),
        });
        let courier = pdf.add_object(dictionary! {
            "Type" => Object::name("Font"),
            "Subtype" => Object::name("Type1"),
            "BaseFont" => Object::name("Courier"),
        });
        let form = pdf.add_object(Stream::new(
            dictionary! {
                "Subtype" => Object::name("Form"),
                "Resources" => dictionary! { "Font" => dictionary! { "F9" => helv } },
            },
            Vec::new(),
        ));
        let pages_id = pdf.new_object_id();
        let page = pdf.add_object(dictionary! {
            "Type" => Object::name("Page"),
            "Parent" => pages_id,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => embedded }, "XObject" => dictionary! { "X1" => form } },
        });
        let plain =
            pdf.add_object(dictionary! { "Type" => Object::name("Page"), "Parent" => pages_id });
        pdf.objects.insert(
            pages_id,
            dictionary! {
                "Type" => Object::name("Pages"), "Kids" => vec![page.into(), plain.into()], "Count" => 2,
                "Resources" => dictionary! { "Font" => dictionary! { "F2" => courier } },
            }
            .into(),
        );
        let info = pdf.add_object(dictionary! {
            "Title" => literal("Engagement letter"),
            "Producer" => literal("sheaf test"),
        });
        let catalog = pdf.add_object(dictionary! {
            "Type" => Object::name("Catalog"),
            "Pages" => pages_id,
            "Version" => Object::name("1.7"),
            "MarkInfo" => dictionary! { "Marked" => true },
        });
        pdf.trailer.set("Root", catalog);
        pdf.trailer.set("Info", info);
        pdf
    }

    /// `pdf`, written, as a source.
    fn source(pdf: &Pdf) -> Src {
        let mut bytes = Vec::new();
        pdf.clone().save_to(&mut bytes).unwrap();
        crate::forms::fixture::source(bytes)
    }

    #[test]
    fn reads_info_version_and_tags() {
        let p = read(&source(&sample()));
        assert_eq!(p.title, "Engagement letter");
        assert_eq!(p.producer, "sheaf test");
        assert_eq!(p.version, "1.7");
        assert_eq!(p.pages, 2);
        assert!(p.tagged);
        assert!(p.security.is_none());
    }

    #[test]
    fn lists_fonts_including_forms_and_inherited_ones() {
        let p = read(&source(&sample()));
        let font = |name: &str, kind: &str, embedding| Font {
            name: name.into(),
            kind: kind.into(),
            embedding,
        };
        assert_eq!(
            p.fonts,
            [
                font("Arial-Bold", "TrueType", Embedding::Subset),
                font("Courier", "Type1", Embedding::NotEmbedded),
                font("Helvetica", "Type1", Embedding::NotEmbedded),
            ]
        );
        assert!(!p.fonts_truncated);
    }

    #[test]
    fn an_encrypted_file_reads_decrypted_with_its_permissions() {
        let mut pdf = sample();
        // Print and copy allowed; nothing else.
        let state = TestEncryption::new(CipherKind::Aes256, "", "owner")
            .with_permissions(Permissions::PRINT | Permissions::COPY);
        pdf.encrypt(&state).unwrap();
        let p = read(&source(&pdf));
        // Its strings decrypt as they're read; /Encrypt is kept.
        assert_eq!(p.title, "Engagement letter");
        assert_eq!(p.fonts.len(), 3);
        let s = p.security.expect("its security");
        assert_eq!(s.method, "Password (AES-256)");
        let allowed: Vec<&str> = s
            .permissions
            .iter()
            .filter(|(_, ok)| *ok)
            .map(|(w, _)| *w)
            .collect();
        assert_eq!(
            allowed,
            [
                "Printing",
                "Copying text and images",
                "Text access for accessibility"
            ]
        );
    }
}
