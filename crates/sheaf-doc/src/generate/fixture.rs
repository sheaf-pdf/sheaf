//! Headers drawn on pages, as sheaf draws one now and as Acrobat does: a
//! header sheaf draws as a generator's block, which `/SHEF_Generators`
//! says is a header; and one Acrobat drew, a form its `/PieceInfo` says is
//! a header. For tests here and in other crates (the `fixtures` feature).

use lopdf::{Dictionary, Document as Pdf, Object, Stream, dictionary};

/// A header Acrobat drew after the page's content, as it writes it: its
/// form, `/HeaderFm`, drawn in an artifact.
pub const ACROBAT_HEADER: &[u8] = b"/Artifact <</Type /Pagination /Subtype /Header>> BDC\n\
    q 1 0 0 1 72 740 cm /HeaderFm Do Q\nEMC\n";

/// `page` of `pdf` with [`ACROBAT_HEADER`] drawn on it, after its content:
/// "Draft" in Helvetica, in a form whose `/PieceInfo` says Acrobat drew a
/// header in it, named in its resources (a dictionary of its own, or the
/// object they are).
pub fn acrobat_header(pdf: &mut Pdf, page: &mut Dictionary) {
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let piece = dictionary! { "ADBE_CompoundType" => dictionary! {
        "DocSettings" => dictionary! {}, "Private" => "Header",
    } };
    let form = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "PieceInfo" => piece,
            "BBox" => vec![0.into(), 0.into(), 300.into(), 20.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        },
        b"BT /F1 10 Tf 0 4 Td (Draft) Tj ET".to_vec(),
    ));
    let header = pdf.add_object(Stream::new(dictionary! {}, ACROBAT_HEADER.to_vec()));
    bind(pdf, page, b"XObject", b"HeaderFm", form.into());
    let mut contents: Vec<Object> = (crate::content::contents(page).into_iter())
        .map(Object::Reference)
        .collect();
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

/// `/SHEF_Generators` saying [`SHEAF_HEADER`]'s generator is a header and
/// footer on the pages its blocks are on.
pub const SHEAF_HEADER_JSON: &str = concat!(
    r#"{"v":1,"generators":[{"id":"g1f","kind":"header-footer","z":"over","scope":"pages","#,
    r#""replaces":["header","footer"],"spec":{"texts":["Draft","","","","",""],"#,
    r#""font":"helvetica","size":10.0,"color":[0,0,0],"margins":{"top":36.0,"bottom":36.0,"#,
    r#""left":72.0,"right":72.0},"first_number":1},"frozen":{"date":"","file":""}}]}"#,
);

/// `page` of `pdf` with [`SHEAF_HEADER`] drawn on it, after its content,
/// its list and font named in its resources (a dictionary of its own, or
/// the object they are); the catalog's `/SHEF_Generators` must say what it
/// is ([`SHEAF_HEADER_JSON`]).
pub fn sheaf_header(pdf: &mut Pdf, page: &mut Dictionary) {
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let list = dictionary! {
        "Type" => "Pagination", "Subtype" => "Header",
        "SHEF_Gen" => Object::string_literal("g1f"),
    };
    let marker = dictionary! {
        "SHEF_Block" => dictionary! { "K" => "Gen", "G" => Object::string_literal("g1f") },
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
