//! A watermark and a background as an older sheaf drew them, before M10.5
//! slice 16: `/SheafStamp` artifacts whose lists keep the settings they
//! were drawn with (`/SheafSettings`, an image's path among them), drawn
//! with a graphics state, a font and an image of their own names
//! (`/SheafWMgs`, `/SheafWMf`, `/SheafWMim`). For tests here and in other
//! crates (the `fixtures` feature).

use lopdf::{Dictionary, Document as Pdf, ObjectId, Stream, dictionary};

/// A background an older sheaf drew, as it wrote it: an image, and in its
/// list the settings it was drawn with, the image's path among them.
pub const BACKGROUND: &[u8] = b"/Artifact <</Type/Pagination/Subtype/Background/SheafStamp true\
    /SheafSettings({\"mark\":{\"image\":{\"path\":\"/home/someone/private/logo.png\",\"scale\":0.5}},\
    \"opacity\":0.3,\"rotation\":0.0,\"anchor\":{\"horizontal\":1,\"vertical\":1},\"behind\":true})>> \
    BDC\nq\n/SheafWMgs gs 306.0000 0.0000 0.0000 306.0000 153.000 243.000 cm /SheafWMim Do\nQ\nEMC\n";

/// The `q` an older sheaf put before the page's content, which its
/// watermark's `Q` ends.
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
