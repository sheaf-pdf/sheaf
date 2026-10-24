//! Text as words: what content's text-showing operators draw, decoded
//! through each font's `/ToUnicode` or encoding, composed (NFC) and
//! counted by word. The same text drawn in two fonts, with different
//! codes, gives equal bags: what the removal check compares (§11.9). A
//! string whose codes a font doesn't say the text of is its bytes' hash,
//! which only the same bytes match.

use std::collections::{BTreeMap, HashMap};

use lopdf::Object;
use unicode_normalization::UnicodeNormalization;

use super::font::Decoder;
use super::lex::{self, LexError, ResCx, Tok};
use super::page::{Concat, ReadObjects, direct};
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

/// The words `content`, lexed with `res`, draws with `fonts`. `Tf` picks
/// the font, and `Q` restores the one `q` saved. The strings `Tj`, `TJ`,
/// `'` and `"` show are decoded and run together, and a word ends at
/// white-space, at `BT` and `ET`, and where the text moves to a new place
/// (`Td`, `TD`, `Tm`, `T*`, `'`, `"`); `TJ`'s adjustments don't end one.
/// A string with a code its font doesn't say the text of, or shown in no
/// font the resources have, is its bytes' hash, and ends a word.
pub fn text_bag(
    content: &Concat,
    res: &ResCx,
    lim: &Limits,
    fonts: &FontDecoders,
) -> Result<TextBag, LexError> {
    let mut bag = TextBag::default();
    let mut run = String::new();
    let mut font = None;
    let mut saved = Vec::new();
    let depth = lim.content.depth;
    let show =
        |bag: &mut TextBag, run: &mut String, font: Option<&Decoder>, operand: Option<&[u8]>| {
            let mut string = |s: &[u8]| match font.and_then(|f| f.text(s)) {
                Some(text) => run.push_str(&text),
                None => {
                    bag.add(&std::mem::take(run));
                    bag.add_hash(sheaf_util::checksum(s));
                }
            };
            match operand.and_then(|o| lex::object(o, depth)) {
                Some(Object::String(s, _)) => string(&s),
                Some(Object::Array(items)) => {
                    for item in items {
                        if let Object::String(s, _) = item {
                            string(&s);
                        }
                    }
                }
                _ => {}
            }
        };
    lex::steps(content.bytes(), res, lim, |step| {
        let Tok::Op(op) = step.tok else {
            return Ok::<_, LexError>(());
        };
        match op {
            b"q" => saved.push(font),
            b"Q" => font = saved.pop().unwrap_or(font),
            b"Tf" => font = step.name(1).and_then(|n| fonts.get(&n)),
            b"Tj" | b"TJ" => show(&mut bag, &mut run, font, step.arg(0)),
            b"'" | b"\"" => {
                bag.add(&std::mem::take(&mut run));
                show(&mut bag, &mut run, font, step.arg(0));
            }
            b"BT" | b"ET" | b"Td" | b"TD" | b"Tm" | b"T*" => bag.add(&std::mem::take(&mut run)),
            _ => {}
        }
        Ok(())
    })?;
    bag.add(&run);
    Ok(bag)
}

/// How the fonts of some resources read as text, by resource name.
#[derive(Debug, Clone, Default)]
pub struct FontDecoders {
    fonts: HashMap<Vec<u8>, Decoder>,
}

impl FontDecoders {
    /// The fonts of `resources`, a page's or a form's `/Resources` as
    /// written. Each reads its codes as text through, in order:
    /// - its `/ToUnicode` CMap: `bfchar`, `bfrange` with a string or an
    ///   array, and a composite font's codespace ranges, which say how
    ///   long its codes are where its encoding isn't Identity;
    /// - a simple font's `/Differences`, by glyph name, through the Adobe
    ///   Glyph List and `uniXXXX`/`uXXXXX` names;
    /// - its base encoding: WinAnsi, MacRoman or Standard as named, or
    ///   its own: Symbol's or ZapfDingbats' for those, Standard for
    ///   another Type 1 font;
    /// - otherwise none: a string with such a code isn't read as text.
    pub fn read(
        objects: &impl ReadObjects,
        resources: Option<&Object>,
        lim: &Limits,
    ) -> FontDecoders {
        let fonts = resources
            .and_then(|r| direct(objects, r))
            .and_then(|r| r.as_dict().ok())
            .and_then(|r| direct(objects, r.get(b"Font").ok()?))
            .and_then(|f| f.as_dict().ok());
        FontDecoders {
            fonts: fonts
                .into_iter()
                .flat_map(|fonts| fonts.iter())
                .filter_map(|(name, font)| {
                    let font = direct(objects, font)?.as_dict().ok()?;
                    Some((name.clone(), Decoder::of_text(objects, font, lim)))
                })
                .collect(),
        }
    }

    fn get(&self, name: &[u8]) -> Option<&Decoder> {
        self.fonts.get(name)
    }
}
