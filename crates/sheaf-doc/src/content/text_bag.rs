//! Text as words: what content's text-showing operators draw, decoded
//! through each font's `/ToUnicode` or encoding, or a Type0 font's
//! predefined CMap's or collection's
//! ([`Foreign::code_text`](super::Foreign::code_text)), composed (NFC)
//! and counted by word. The same text drawn in two fonts, with different
//! codes, gives equal bags: what the removal check compares (§11.9). A
//! string whose codes a font doesn't say the text of is its bytes' hash,
//! which only the same bytes match.

use std::collections::BTreeMap;
use std::ops::ControlFlow;

use lopdf::Object;
use unicode_normalization::UnicodeNormalization;

use super::Fonts;
use super::interp::{Forms, InterpError, Kind, Ops, Opts, Wants, interpret};
use super::page::{Concat, ReadObjects};
use crate::source::Limits;

/// Words, each with how many times it's drawn, and what isn't words, by
/// hash: strings drawn in codes a font doesn't say the text of, and what
/// the removal check compares whole (an image, an embedded file).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextBag {
    words: BTreeMap<String, usize>,
    hashes: BTreeMap<u128, usize>,
}

impl TextBag {
    /// Counts `text`'s words: composed (NFC), and split at white-space.
    pub fn add(&mut self, text: &str) {
        let text: String = text.nfc().collect();
        for word in text.split_whitespace() {
            *self.words.entry(word.to_string()).or_default() += 1;
        }
    }

    /// Counts what isn't words, by its hash.
    pub fn add_hash(&mut self, hash: u128) {
        *self.hashes.entry(hash).or_default() += 1;
    }

    /// Counts `other`'s words and hashes too.
    pub fn extend(&mut self, other: &TextBag) {
        for (word, n) in &other.words {
            *self.words.entry(word.clone()).or_default() += n;
        }
        for (&hash, n) in &other.hashes {
            *self.hashes.entry(hash).or_default() += n;
        }
    }

    /// Each word, in order, with how many times it's drawn.
    pub fn words(&self) -> impl Iterator<Item = (&str, usize)> {
        self.words.iter().map(|(w, &n)| (w.as_str(), n))
    }

    /// Each hash, in order, with how many times it's counted.
    pub fn hashes(&self) -> impl Iterator<Item = (u128, usize)> + '_ {
        self.hashes.iter().map(|(&h, &n)| (h, n))
    }

    /// How many words and hashes it holds, each as many times as it's
    /// counted.
    pub fn len(&self) -> usize {
        self.words.values().chain(self.hashes.values()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.hashes.is_empty()
    }

    /// How many of its words and hashes `other` hasn't got, counting each
    /// as often as it has it more than `other`: none when it's within
    /// `other`.
    pub fn missing_from(&self, other: &TextBag) -> usize {
        let short = |n: usize, has: Option<&usize>| n.saturating_sub(has.copied().unwrap_or(0));
        let words = (self.words.iter()).map(|(w, &n)| short(n, other.words.get(w)));
        let hashes = (self.hashes.iter()).map(|(h, &n)| short(n, other.hashes.get(h)));
        words.chain(hashes).sum()
    }
}

/// The words `content`, drawn with `resources` (a page's or a form's
/// `/Resources` as written), draws in `fonts`, a source's (read for text,
/// [`Fonts::for_text`]), as the content interpreter reads it
/// ([`interpret`]), each form it draws judged on its own
/// ([`Forms::Report`]). `Tf` picks the font, or an ExtGState's
/// `/Font`, and `Q` restores the one `q` saved. The strings `Tj`, `TJ`,
/// `'` and `"` show are decoded and run together, and a word ends at
/// white-space, at `BT` and `ET`, and where the text moves to a new place
/// (`Td`, `TD`, `Tm`, `T*`, `'`, `"`); `TJ`'s adjustments don't end one.
/// A string with a code its font doesn't say the text of, or shown in no
/// font the resources have, is its bytes' hash, and ends a word.
pub fn text_bag(
    objects: &impl ReadObjects,
    content: &Concat,
    resources: Option<&Object>,
    lim: &Limits,
    fonts: &Fonts,
) -> Result<TextBag, InterpError> {
    let mut bag = TextBag::default();
    let mut run = String::new();
    let opts = Opts {
        forms: Forms::Report,
        wants: WANTS,
    };
    interpret(objects, content, resources, lim, fonts, opts, |event| {
        match event.kind {
            // One that ends a word.
            Kind::Op => flush(&mut bag, &mut run),
            Kind::Text(show) => {
                let font = show.font.filter(|_| !show.stock);
                match font.and_then(|f| f.text(show.string)) {
                    Some(text) => run.push_str(&text),
                    None => {
                        flush(&mut bag, &mut run);
                        bag.add_hash(sheaf_util::checksum(show.string));
                    }
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    })?;
    flush(&mut bag, &mut run);
    Ok(bag)
}

/// What [`text_bag`] reads of content: the strings shown, and the
/// operators that end a word.
pub(crate) const WANTS: Wants = Wants {
    ops: Ops::of(&[b"BT", b"ET", b"Td", b"TD", b"Tm", b"T*", b"'", b"\""]),
    text: true,
    ..Wants::NONE
};

/// The words of `run`, text run together, counted in `bag`, and `run`
/// emptied; an empty one has none.
fn flush(bag: &mut TextBag, run: &mut String) {
    if !run.is_empty() {
        bag.add(&std::mem::take(run));
    }
}
