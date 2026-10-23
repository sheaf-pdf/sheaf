//! The `text`, `patterns` and `ocr` interfaces: a page's text as the file
//! will draw it, and its content recognized now, in PageSpace.

use std::sync::Arc;
use std::time::Instant;

use sheaf_doc::{PageChars, Query, TextHit, TextOpts, ViewInputs};

use super::api::{self, types as w};
use super::convert::{box_from_wit, box_to_wit};
use crate::runtime::HostState;

fn matches_to_wit(hits: Vec<TextHit>) -> Vec<w::TextMatch> {
    hits.into_iter()
        .map(|h| w::TextMatch {
            text: h.text,
            rects: h.rects.into_iter().map(box_to_wit).collect(),
            groups: h.groups,
        })
        .collect()
}

/// Page `page` of the call's document, its text as the file will draw it
/// now: recognized text and a header, saved or not, with it, and no
/// annotation's or field's text, which PDFium never reads as a page's
/// text (M10.5 §14.3; [`ViewInputs::plugin_text`]). Read: what it draws
/// so.
fn drawn_text(host: &mut HostState, page: u64) -> Result<Arc<PageChars>, String> {
    let inputs = ViewInputs::plugin_text();
    let (d, snap, id) = host.drawn(page, &inputs)?;
    let chars = d.text.chars(&snap, id, &TextOpts { inputs });
    chars.map_err(|e| format!("{e:#}"))
}

impl api::text::Host for HostState {
    fn page_text(&mut self, page: u64) -> Result<String, String> {
        let chars = drawn_text(self, page)?;
        Ok(chars.text(0, chars.len()))
    }

    fn pattern_search(&mut self, page: u64, regex: String) -> Result<Vec<w::TextMatch>, String> {
        self.doc().ok_or("no document is open")?;
        let re = regex::RegexBuilder::new(&regex)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| e.to_string())?;
        let chars = drawn_text(self, page)?;
        Ok(matches_to_wit(chars.find(&Query::Pattern(Arc::new(re)))))
    }

    fn search(
        &mut self,
        page: u64,
        query: String,
        match_case: bool,
    ) -> Result<Vec<w::TextMatch>, String> {
        let query = Query::literal(&query, match_case).map_err(|e| e.to_string())?;
        let chars = drawn_text(self, page)?;
        Ok(matches_to_wit(chars.find(&query)))
    }

    fn words(&mut self, page: u64, rect: Option<w::Box2>) -> Result<Vec<w::TextMatch>, String> {
        static WORD: std::sync::LazyLock<Arc<regex::Regex>> =
            std::sync::LazyLock::new(|| Arc::new(regex::Regex::new(r"\S+").unwrap()));
        let chars = drawn_text(self, page)?;
        let words = matches_to_wit(chars.find(&Query::Pattern(WORD.clone())));
        let within = |m: &w::TextMatch| match rect {
            None => true,
            Some(r) => m.rects.first().is_some_and(|b| {
                let (cx, cy) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
                cx >= r.x && cx <= r.x + r.w && cy >= r.y && cy <= r.y + r.h
            }),
        };
        Ok(words.into_iter().filter(within).collect())
    }

    fn text_boxes(&mut self, page: u64, rect: w::Box2) -> Result<Vec<w::Box2>, String> {
        let chars = drawn_text(self, page)?;
        let boxes = chars.rects_in(box_from_wit(rect)).into_iter();
        Ok(boxes.map(box_to_wit).collect())
    }
}

impl api::patterns::Host for HostState {
    fn presets(&mut self) -> Vec<api::patterns::Preset> {
        let presets = self
            .call
            .as_ref()
            .map(|c| &c.presets[..])
            .unwrap_or_default();
        (presets.iter())
            .map(|p| api::patterns::Preset {
                name: p.name.clone(),
                regex: p.regex.clone(),
            })
            .collect()
    }
}

/// An OCR layer, its words in PageSpace as recognized.
fn layer_to_wit(l: &sheaf_doc::TextLayer) -> api::ocr::TextLayer {
    use api::ocr as wo;
    wo::TextLayer {
        words: (l.words.iter())
            .map(|w| wo::OcrWord {
                text: w.text.clone(),
                rect: box_to_wit(w.rect),
                confidence: w.confidence,
            })
            .collect(),
        lines: (l.lines.iter())
            .map(|r| (r.start as u32, r.end as u32))
            .collect(),
        engine: l.engine.clone(),
    }
}

impl api::ocr::Host for HostState {
    /// Page `page` of the call's document recognized now with the user's
    /// engine, from its content alone. Read: what that content draws.
    fn recognize(
        &mut self,
        page: u64,
        options: api::ocr::OcrOptions,
    ) -> Result<api::ocr::TextLayer, String> {
        let (d, snap, id) = self.drawn(page, &ViewInputs::content_only())?;
        let ocr = d.ocr.clone().ok_or("OCR isn't available")?;
        let started = Instant::now();
        let layer = ocr.recognize(&snap, id, options.language, options.dpi);
        // The engine's time isn't the plugin's.
        self.deadline += started.elapsed();
        layer
            .map(|l| layer_to_wit(&l))
            .map_err(|e| format!("{e:#}"))
    }
}
