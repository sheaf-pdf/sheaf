//! What sheaf drew on pages before M10.5 slices 16 and 17, and a header
//! as it draws one now. A watermark and a background an older sheaf drew:
//! `/SheafStamp` artifacts whose lists keep the settings they were drawn
//! with (`/SheafSettings`, an image's path among them), drawn with a
//! graphics state, a font and an image of their own names (`/SheafWMgs`,
//! `/SheafWMf`, `/SheafWMim`). A header an older sheaf drew, in a font
//! named `/SheafHF`, its settings in the catalog (`/SheafHeaderFooter`);
//! and a header sheaf draws as a generator's block, which
//! `/SheafGenerators` says is a header. For tests here and in other crates
//! (the `fixtures` feature).

use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};

/// A background an older sheaf drew, as it wrote it: an image, and in its
/// list the settings it was drawn with, the image's path among them.
pub const BACKGROUND: &[u8] = b"/Artifact <</Type/Pagination/Subtype/Background/SheafStamp true\
    /SheafSettings({\"mark\":{\"image\":{\"path\":\"/home/someone/private/logo.png\",\"scale\":0.5}},\
    \"opacity\":0.3,\"rotation\":0.0,\"anchor\":{\"horizontal\":1,\"vertical\":1},\"behind\":true})>> \
    BDC\nq\n/SheafWMgs gs 306.0000 0.0000 0.0000 306.0000 153.000 243.000 cm /SheafWMim Do\nQ\nEMC\n";

/// The `q` an older sheaf put before the page's content, which its
/// watermark's or header's `Q` ends.
pub const Q: &[u8] = b"q\n";

/// A watermark an older sheaf drew after the page's content, as it wrote
/// it: "DRAFT" in red.
pub const WATERMARK: &[u8] = b"Q\n/Artifact <</Type/Pagination/Subtype/Watermark/SheafStamp true\
    /SheafSettings({\"mark\":{\"text\":{\"text\":\"DRAFT\",\"font_size\":72.0,\"color\":[200,30,30]}},\
    \"opacity\":0.3,\"rotation\":45.0,\"anchor\":{\"horizontal\":1,\"vertical\":1},\"behind\":false})>> \
    BDC\nq\n/SheafWMgs gs 0.7071 0.7071 -0.7071 0.7071 200.000 300.000 cm BT 0.784 0.118 0.118 rg \
    /SheafWMf 72.00 Tf 0 0 Td (DRAFT) Tj ET\nQ\nEMC\n";

/// Resources of Helvetica, `font`, as `/F1`, and what an older sheaf drew
/// its watermarks with, made in `pdf`: Helvetica-Bold, a graphics state of
/// 0.3 opacity and a 2×2 image of red, green, blue and black.
pub fn resources(pdf: &mut Pdf, font: ObjectId) -> Dictionary {
    let bold = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold",
    });
    let gs = pdf.add_object(dictionary! { "Type" => "ExtGState", "ca" => 0.3, "CA" => 0.3 });
    let image = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 2,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
        },
        vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0],
    ));
    dictionary! {
        "Font" => dictionary! { "F1" => font, "SheafWMf" => bold },
        "ExtGState" => dictionary! { "SheafWMgs" => gs },
        "XObject" => dictionary! { "SheafWMim" => image },
    }
}

/// A header an older sheaf drew after the page's content, as it wrote it:
/// "Draft" at the top left of a Letter page, in Helvetica named
/// `/SheafHF`, its list inline.
pub const HEADER: &[u8] = b"Q\n/Artifact <</Type/Pagination/Subtype/Header/SheafStamp true>> BDC\n\
    q\nBT\n0 0 0 rg\n/SheafHF 10 Tf\n1 0 0 1 72 748.8 Tm\n(Draft) Tj\nET\nQ\nEMC\n";

/// What an older sheaf kept of [`HEADER`] in the catalog
/// (`/SheafHeaderFooter`): its settings, on the third page.
pub const HEADER_SETTINGS: &str = concat!(
    r#"{"texts":["Draft","","","","",""],"font_size":10.0,"color":[0,0,0],"top":36.0,"#,
    r#""bottom":36.0,"left":72.0,"right":72.0,"first_page_number":1,"date":"","file":"","#,
    r#""bates":null,"page_indexes":[2]}"#,
);

/// `page` of `pdf` with [`HEADER`] drawn on it as an older sheaf drew it:
/// its `/Contents` `[q, …, header]`, Helvetica named `/SheafHF` in its
/// resources, which are its own dictionary.
pub fn legacy_header(pdf: &mut Pdf, page: &mut Dictionary) {
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let q = pdf.add_object(Stream::new(dictionary! {}, Q.to_vec()));
    let header = pdf.add_object(Stream::new(dictionary! {}, HEADER.to_vec()));
    bind(pdf, page, b"Font", b"SheafHF", font.into());
    let mut contents = vec![Object::Reference(q)];
    contents.extend(
        crate::content::contents(page)
            .into_iter()
            .map(Object::Reference),
    );
    contents.push(header.into());
    page.set("Contents", contents);
}

/// The id of the header and footer [`sheaf_header`] draws.
pub const SHEAF_HEADER_ID: u64 = 0x1f;

/// A header sheaf drew as generator `g1f`'s block, a stream of its own:
/// "Draft" at the top left of a Letter page, in Helvetica, its list
/// `/ShHead` and its font `/ShHelv`.
pub const SHEAF_HEADER: &[u8] = b"/Artifact /ShHead BDC\nq\nBT\n0 0 0 rg\n/ShHelv 10 Tf\n\
    1 0 0 1 72 748.8 Tm\n(Draft) Tj\nET\nQ\nEMC\n";

/// `/SheafGenerators` saying [`SHEAF_HEADER`]'s generator is a header and
/// footer on the pages its blocks are on.
pub const SHEAF_HEADER_JSON: &str = concat!(
    r#"{"v":1,"generators":[{"id":"g1f","kind":"header-footer","z":"over","scope":"pages","#,
    r#""replaces":["header","footer"],"spec":{"texts":["Draft","","","","",""],"#,
    r#""font":"helvetica","size":10.0,"color":[0,0,0],"margins":{"top":36.0,"bottom":36.0,"#,
    r#""left":72.0,"right":72.0},"first_number":1},"frozen":{"date":"","file":""}}]}"#,
);

/// `page` of `pdf` with [`SHEAF_HEADER`] drawn on it, after its content,
/// its list and font named in its resources (a dictionary of its own, or
/// the object they are); the catalog's `/SheafGenerators` must say what it
/// is ([`SHEAF_HEADER_JSON`]).
pub fn sheaf_header(pdf: &mut Pdf, page: &mut Dictionary) {
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let list = dictionary! {
        "Type" => "Pagination", "Subtype" => "Header",
        "SheafGen" => Object::string_literal("g1f"),
    };
    let marker = dictionary! {
        "SheafBlock" => dictionary! { "K" => "Gen", "G" => Object::string_literal("g1f") },
    };
    let block = pdf.add_object(Stream::new(marker, SHEAF_HEADER.to_vec()));
    bind(pdf, page, b"Font", b"ShHelv", font.into());
    bind(pdf, page, b"Properties", b"ShHead", list.into());
    let mut contents: Vec<Object> = (crate::content::contents(page).into_iter())
        .map(Object::Reference)
        .collect();
    contents.push(block.into());
    page.set("Contents", contents);
}

/// `name` bound to `value` in `category` of `page`'s resources, those of
/// its own or the object they are.
fn bind(pdf: &mut Pdf, page: &mut Dictionary, category: &[u8], name: &[u8], value: Object) {
    if let Ok(&Object::Reference(r)) = page.get(b"Resources") {
        let resources = pdf.get_object_mut(r).and_then(Object::as_dict_mut);
        return bind_in(resources.expect("resources"), category, name, value);
    }
    if !matches!(page.get(b"Resources"), Ok(Object::Dictionary(_))) {
        page.set("Resources", Dictionary::new());
    }
    let resources = page.get_mut(b"Resources").and_then(Object::as_dict_mut);
    bind_in(resources.expect("resources"), category, name, value);
}

/// `name` bound to `value` in `category` of `resources`.
fn bind_in(resources: &mut Dictionary, category: &[u8], name: &[u8], value: Object) {
    if !resources.has(category) {
        resources.set(category, Dictionary::new());
    }
    let names = resources.get_mut(category).and_then(Object::as_dict_mut);
    names.expect("a category").set(name, value);
}
