//! Print what PDFium sees on each page of a PDF: how many real characters
//! of text it has and, optionally, where a query matches, in PageSpace
//! (points, y up). For checking an OCR'd file without opening the app.
//!
//! Usage: cargo run -p sheaf-ocr --example pdftext -- file.pdf [query]

use std::path::Path;

use sheaf_doc::{PageChars, Query};
use sheaf_render::PageIndex;
use sheaf_render::Renderer;
use sheaf_render::pdfium::PdfiumEngine;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: pdftext file.pdf [query]");
        std::process::exit(2);
    };
    let query = args.next();
    let engine = PdfiumEngine::open_bytes(sheaf_util::read_file(Path::new(&path))?, None)?;
    for i in 0..engine.page_count() {
        let n = engine.page_text_len(PageIndex(i))?;
        print!("page {}: {n} chars", i + 1);
        if let Some(q) = &query {
            let read = engine.page_chars(PageIndex(i))?;
            let chars = PageChars::from_engine(read.into_iter().map(|c| (c.ch, c.rect, c.angle)));
            let hits = chars.find(&Query::literal(q, false).unwrap());
            print!(", {} hit(s) for {q:?}", hits.len());
            for h in hits.iter().take(3) {
                if let Some(r) = h.rects.first() {
                    print!(" [{:.0},{:.0} {:.0}×{:.0}]", r.x, r.y, r.w, r.h);
                }
            }
        }
        println!();
    }
    Ok(())
}
