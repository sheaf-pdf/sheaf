//! A page's text as characters in reading order, each with its box: what
//! text selection, Find and Patterns work on, and what text editing will.
//! Built from the engine's characters. A document's text, each page's read
//! from what it draws, recognized text not yet saved included, is a
//! [`TextService`] (M10.5 §10.5).
//!
//! Boxes are in PageSpace (M10.5 §5), like marks. Lines read along x in
//! the [`PageChars::upright`] frame, the way the text itself runs, so a
//! page recognized while turned, or text that runs down the page, still
//! selects along its lines.
//!
//! A position in the text is a *boundary*: 0 is before the first
//! character, `len()` after the last. A selection is the characters
//! between two boundaries.

use std::sync::Arc;

use anyhow::Result;
use sheaf_edit::Rotation;

use crate::annot::{Box2, Pt, caret_box};
use crate::geom::Affine;
use crate::model::{PageId, Snapshot};
use crate::realize::ViewInputs;
use crate::text_layer::overlaps_line;

/// A document's text (M10.5 §10.5): each page's characters read from what
/// it draws under the inputs a caller passes, its realization's, so the
/// screen, the file and the text agree. An inserted, duplicated, cropped
/// or edited page has the text it draws, and so does a page whose source
/// PDFium refuses (from its group's realization); an unsaved header or
/// recognized text is read where the inputs draw it. Boxes are in
/// PageSpace, as the engine gives them (§5).
///
/// It blocks until the engine has read the page: for a thread that may
/// wait on the engine pool (a plugin's, blank detection's), never the
/// pool's own worker, which answers it.
///
/// Only a page's characters, where §10.5 has a document `search` too: the
/// one caller that searches a whole document, the app, can't block, so its
/// walk goes page by page without one.
pub trait TextService: Send + Sync {
    /// Page `page` of `snap`'s characters, read under `o`.
    fn chars(&self, snap: &Snapshot, page: PageId, o: &TextOpts) -> Result<Arc<PageChars>>;
}

/// What a page's text is read under: the view inputs it's realized with
/// (§10.4), so OCR's check and blank detection read the page's content
/// alone and Find reads it as the screen shows it. §10.5's `fields`
/// (fields' display text, M10 slice 14) has no use yet: PDFium reads a
/// page's text from its content and never a widget's appearance, so no
/// field's value is in it.
#[derive(Debug, Clone, PartialEq)]
pub struct TextOpts {
    pub inputs: ViewInputs,
}

/// What Find and Patterns look for on a page.
#[derive(Debug, Clone)]
pub enum Query {
    /// Find's text, compiled by [`Query::literal`].
    Literal(Arc<regex::Regex>),
    /// A pattern, with its capture groups.
    Pattern(Arc<regex::Regex>),
}

impl Query {
    /// Find's `query`: its words in order, escaped, apart by `\s+`, so they
    /// match where the page's are apart by any whitespace (a line break,
    /// two spaces, a no-break space), as PDFium's Find matched them. Case
    /// is folded unless `match_case`. It's compiled once for every page
    /// searched. A query of whitespace alone finds nothing, and only a
    /// query too long to compile fails.
    pub fn literal(query: &str, match_case: bool) -> Result<Query, regex::Error> {
        let words: Vec<String> = query.split_whitespace().map(regex::escape).collect();
        let re = regex::RegexBuilder::new(&words.join(r"\s+"))
            .case_insensitive(!match_case)
            .size_limit(1 << 20)
            .build()?;
        Ok(Query::Literal(Arc::new(re)))
    }
}

/// A match on a page: one rect per line it spans, in PageSpace.
#[derive(Debug, Clone, PartialEq)]
pub struct TextHit {
    pub rects: Vec<Box2>,
    pub text: String,
    /// A pattern's capture groups 1…n (None where a group didn't take
    /// part); empty for a literal search.
    pub groups: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub ch: char,
    /// None for separators the extractor adds: spaces between words, line
    /// breaks.
    pub rect: Option<Box2>,
    /// Which line, from 0, in reading order.
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageChars {
    pub glyphs: Vec<Glyph>,
    /// The frame the text reads upright in.
    pub upright: Rotation,
}

impl PageChars {
    /// From the engine's characters, in its order: each with its box in
    /// PageSpace (None for the separators it adds) and the angle it's
    /// drawn at, in degrees counter-clockwise.
    ///
    /// The text reads upright in the quarter turn most of its boxed
    /// characters are drawn at, so the page's `/Rotate` doesn't matter:
    /// text drawn a quarter turn counter-clockwise, as a `/Rotate 90`
    /// page's upright text is, reads up PageSpace's y. Lines are found in
    /// that frame, by where the characters are, not by the engine's line
    /// breaks, which it sometimes puts mid-line (on a page with
    /// `/Rotate 270`, say): a character starts a new line when it isn't
    /// level with the line so far or goes back. A break the engine left
    /// inside a line reads as a space; one it left out between lines is
    /// added.
    pub fn from_engine(chars: impl IntoIterator<Item = (char, Option<Box2>, f32)>) -> PageChars {
        let chars: Vec<(char, Option<Box2>, f32)> = chars.into_iter().collect();
        let upright = most_drawn(chars.iter().filter(|c| c.1.is_some()).map(|c| c.2));
        let up = Affine::upright(upright);
        let mut glyphs: Vec<Glyph> = Vec::with_capacity(chars.len());
        let mut line = 0;
        // The line so far, and the last character on it, upright.
        let mut current: Option<(Box2, Box2)> = None;
        let mut engine_break = false;
        for (ch, rect, _) in chars {
            match (ch, rect) {
                ('\r', _) => {}
                ('\n', _) => engine_break = true,
                (_, None) => glyphs.push(Glyph {
                    ch,
                    rect: None,
                    line,
                }),
                (_, Some(r)) => {
                    let u = up.apply_box(r);
                    match current {
                        Some((lb, last)) if overlaps_line(lb, u) && u.x >= last.x - last.w => {
                            let spaced = ch.is_whitespace()
                                || glyphs.last().is_none_or(|g| g.ch.is_whitespace());
                            if engine_break && !spaced {
                                glyphs.push(Glyph {
                                    ch: ' ',
                                    rect: None,
                                    line,
                                });
                            }
                            current = Some((lb.union(u), u));
                        }
                        Some(_) => {
                            glyphs.push(Glyph {
                                ch: '\n',
                                rect: None,
                                line,
                            });
                            line += 1;
                            current = Some((u, u));
                        }
                        None => current = Some((u, u)),
                    }
                    engine_break = false;
                    glyphs.push(Glyph {
                        ch,
                        rect: Some(r),
                        line,
                    });
                }
            }
        }
        PageChars { glyphs, upright }
    }

    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// A box in the upright frame (y down; where on the page doesn't
    /// matter, only which way the text runs).
    fn up(&self, b: Box2) -> Box2 {
        Affine::upright(self.upright).apply_box(b)
    }

    fn up_pt(&self, p: Pt) -> Pt {
        Affine::upright(self.upright).apply(p)
    }

    /// The boundary before or after glyph `i`, whichever half of it `p`
    /// (upright) is in.
    fn side(&self, i: usize, p: Pt) -> usize {
        match self.glyphs[i].rect {
            Some(r) => {
                let r = self.up(r);
                if p.x > r.x + r.w / 2.0 { i + 1 } else { i }
            }
            None => i,
        }
    }

    /// The boundary at `p` when it's on a character (within `tolerance`
    /// points of its box); None off the text. A drag that starts here
    /// selects text.
    pub fn boundary_at(&self, p: Pt, tolerance: f32) -> Option<usize> {
        let i = self.glyphs.iter().position(|g| {
            g.rect.is_some_and(|r| {
                Box2 {
                    x: r.x - tolerance,
                    y: r.y - tolerance,
                    w: r.w + 2.0 * tolerance,
                    h: r.h + 2.0 * tolerance,
                }
                .contains(p)
            })
        })?;
        Some(self.side(i, self.up_pt(p)))
    }

    /// The boundary nearest `p`, wherever it is: on the line it's level
    /// with (or the nearest line), at the character nearest along it.
    /// Above the text is the start; below it, the end.
    pub fn boundary_near(&self, p: Pt) -> usize {
        let p = self.up_pt(p);
        // Each line's extent across (top, bottom) in the upright frame.
        let mut lines: Vec<(u32, f32, f32)> = Vec::new();
        for g in &self.glyphs {
            let Some(r) = g.rect else { continue };
            let r = self.up(r);
            match lines.last_mut() {
                Some((l, top, bottom)) if *l == g.line => {
                    *top = top.min(r.y);
                    *bottom = bottom.max(r.bottom());
                }
                _ => lines.push((g.line, r.y, r.bottom())),
            }
        }
        let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
            return 0;
        };
        if p.y < first.1 {
            return 0;
        }
        if p.y > last.2 {
            return self.len();
        }
        let dist = |top: f32, bottom: f32| {
            if p.y < top {
                top - p.y
            } else if p.y > bottom {
                p.y - bottom
            } else {
                0.0
            }
        };
        let line = lines
            .iter()
            .min_by(|a, b| dist(a.1, a.2).total_cmp(&dist(b.1, b.2)))
            .map(|l| l.0)
            .unwrap_or(0);
        // The nearest character along that line.
        let along = |r: Box2| {
            if p.x < r.x {
                r.x - p.x
            } else if p.x > r.right() {
                p.x - r.right()
            } else {
                0.0
            }
        };
        let best = self
            .glyphs
            .iter()
            .enumerate()
            .filter(|(_, g)| g.line == line)
            .filter_map(|(i, g)| g.rect.map(|r| (i, self.up(r))))
            .min_by(|a, b| along(a.1).total_cmp(&along(b.1)));
        match best {
            Some((i, _)) => self.side(i, p),
            None => 0,
        }
    }

    /// Where a caret at boundary `at` goes (see [`caret_box`]): after the
    /// character before it, or before the one after when it starts a line
    /// or follows a space. None away from any character.
    pub fn caret_at(&self, at: usize) -> Option<Box2> {
        let before = at
            .checked_sub(1)
            .and_then(|i| self.glyphs.get(i))
            .and_then(|g| g.rect)
            .map(|r| (self.up(r), true));
        let after = self
            .glyphs
            .get(at)
            .and_then(|g| g.rect)
            .map(|r| (self.up(r), false));
        let (r, after_it) = before.or(after)?;
        let x = if after_it { r.right() } else { r.x };
        let b = caret_box(x, r.bottom(), r.h);
        // Back from the upright frame.
        Some(Affine::upright(self.upright).inverse().apply_box(b))
    }

    /// The word around boundary `at`: the run of characters that aren't
    /// whitespace, as two boundaries. Empty on whitespace.
    pub fn word_at(&self, at: usize) -> (usize, usize) {
        let word = |i: usize| !self.glyphs[i].ch.is_whitespace();
        // The character under the pointer: after the boundary, or before
        // it at a word's end.
        let i = if at < self.len() && word(at) {
            at
        } else if at > 0 && word(at - 1) {
            at - 1
        } else {
            return (at, at);
        };
        let mut a = i;
        while a > 0 && word(a - 1) {
            a -= 1;
        }
        let mut b = i + 1;
        while b < self.len() && word(b) {
            b += 1;
        }
        (a, b)
    }

    /// The words (runs of characters that aren't whitespace), each with
    /// its boundaries and box.
    pub fn words(&self) -> Vec<(String, (usize, usize), Box2)> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < self.len() {
            if self.glyphs[at].ch.is_whitespace() {
                at += 1;
                continue;
            }
            // `at` starts a word here, so it runs on from it.
            let (a, b) = self.word_at(at);
            if let Some(r) = self.rects(a, b).into_iter().reduce(Box2::union) {
                out.push((self.text(a, b), (a, b), r));
            }
            at = b.max(at + 1);
        }
        out
    }

    /// The characters between two boundaries, in either order.
    pub fn text(&self, a: usize, b: usize) -> String {
        let (a, b) = (a.min(b).min(self.len()), a.max(b).min(self.len()));
        self.glyphs[a..b].iter().map(|g| g.ch).collect()
    }

    /// The boxes of the characters between two boundaries, one per line.
    pub fn rects(&self, a: usize, b: usize) -> Vec<Box2> {
        let (a, b) = (a.min(b).min(self.len()), a.max(b).min(self.len()));
        merge_lines(self.glyphs[a..b].iter())
    }

    /// The boxes of the characters whose centre is inside `rect`, one per
    /// line: what a highlight dragged over text covers.
    pub fn rects_in(&self, rect: Box2) -> Vec<Box2> {
        merge_lines(self.glyphs.iter().filter(|g| {
            g.rect.is_some_and(|r| {
                rect.contains(Pt {
                    x: r.x + r.w / 2.0,
                    y: r.y + r.h / 2.0,
                })
            })
        }))
    }

    /// How many of the characters are drawn and aren't whitespace, as the
    /// engine counts a page's text: none is an image-only page, a
    /// candidate for OCR.
    pub fn text_len(&self) -> usize {
        (self.glyphs.iter())
            .filter(|g| g.rect.is_some() && !g.ch.is_whitespace())
            .count()
    }

    /// Every match of `q` in reading order, over the text as
    /// [`PageChars::text`] reads it: words apart by a space, lines by a
    /// line break. A literal's words match where the page's are apart by
    /// any whitespace, as PDFium's Find did, so a phrase found across a
    /// line break has a box on each line; its matches don't overlap. A
    /// pattern's are its own. A match of nothing, or of separators alone,
    /// has no box and isn't one.
    pub fn find(&self, q: &Query) -> Vec<TextHit> {
        let (Query::Literal(re) | Query::Pattern(re)) = q;
        (self.pattern(re).into_iter())
            .filter_map(|(a, b, groups)| {
                let rects = self.rects(a, b);
                (!rects.is_empty()).then(|| TextHit {
                    rects,
                    text: self.text(a, b),
                    groups,
                })
            })
            .collect()
    }

    /// Where `re` matches something, as boundaries, with its groups.
    fn pattern(&self, re: &regex::Regex) -> Vec<Span> {
        let mut text = String::with_capacity(self.len());
        // Where each character starts in `text`.
        let mut starts = Vec::with_capacity(self.len());
        for g in &self.glyphs {
            starts.push(text.len());
            text.push(g.ch);
        }
        (re.captures_iter(&text))
            .filter_map(|caps| {
                let m = caps.get(0).filter(|m| !m.is_empty())?;
                let a = starts.partition_point(|&b| b < m.start());
                let b = starts.partition_point(|&b| b < m.end());
                let groups = (caps.iter().skip(1))
                    .map(|g| g.map(|g| g.as_str().to_string()))
                    .collect();
                Some((a, b, groups))
            })
            .collect()
    }
}

/// A match: its boundaries, and a pattern's groups.
type Span = (usize, usize, Vec<Option<String>>);

/// The quarter turn most of `angles` (degrees counter-clockwise) are
/// nearest, as a `/Rotate` that shows them upright: R0 for none, and the
/// lesser turn on a tie.
fn most_drawn(angles: impl Iterator<Item = f32>) -> Rotation {
    let mut counts = [0usize; 4];
    for a in angles {
        counts[((a / 90.0).round() as i32).rem_euclid(4) as usize] += 1;
    }
    let most = (1..4).fold(0, |most, q| if counts[q] > counts[most] { q } else { most });
    Rotation::from_degrees(most as i32 * 90)
}

fn merge_lines<'a>(glyphs: impl Iterator<Item = &'a Glyph>) -> Vec<Box2> {
    let mut out: Vec<(u32, Box2)> = Vec::new();
    for g in glyphs {
        let Some(r) = g.rect else { continue };
        match out.last_mut() {
            Some((l, b)) if *l == g.line => *b = b.union(r),
            _ => out.push((g.line, r)),
        }
    }
    out.into_iter().map(|(_, b)| b).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Geom;

    /// A character as the engine gives it.
    type Drawn = (char, Option<Box2>, f32);

    /// A Letter page turned `r`.
    fn letter(r: Rotation) -> Geom {
        Geom::read(None, None, r.degrees() as i64)
    }

    /// A point given as it shows on an upright Letter page, in PageSpace.
    fn shown(x: f32, y: f32) -> Pt {
        letter(Rotation::R0).from_view().apply(Pt { x, y })
    }

    /// A PageSpace box as it shows on an upright Letter page.
    fn as_shown(b: Box2) -> Box2 {
        letter(Rotation::R0).to_view().apply_box(b)
    }

    /// `ch` drawn `angle` degrees counter-clockwise, its box showing as
    /// `shows` on a Letter page the file turns `page`: in PageSpace, as
    /// the engine gives it.
    fn on(page: Rotation, angle: f32, ch: char, shows: Box2) -> Drawn {
        (ch, Some(letter(page).from_view().apply_box(shows)), angle)
    }

    /// A 10-by-12-point character at (`x`, `y`) as shown, on an upright
    /// page.
    fn g(ch: char, x: f32, y: f32) -> Drawn {
        let shows = Box2 {
            x,
            y,
            w: 10.0,
            h: 12.0,
        };
        on(Rotation::R0, 0.0, ch, shows)
    }

    /// A space or line break the engine adds.
    fn added(ch: char) -> Drawn {
        (ch, None, 0.0)
    }

    /// "ab cd\nef": 10-point-wide characters, lines 20 points apart.
    fn sample() -> PageChars {
        PageChars::from_engine([
            g('a', 100.0, 100.0),
            g('b', 110.0, 100.0),
            added(' '),
            g('c', 130.0, 100.0),
            g('d', 140.0, 100.0),
            added('\r'),
            added('\n'),
            g('e', 100.0, 120.0),
            g('f', 110.0, 120.0),
        ])
    }

    #[test]
    fn engine_text_reads_in_order_with_line_breaks() {
        let c = sample();
        assert_eq!(c.text(0, c.len()), "ab cd\nef");
        assert_eq!(c.glyphs.iter().filter(|g| g.line == 1).count(), 2);
        // Stored in PageSpace: 'a' is 100 to 112 down the page.
        let a = c.glyphs[0].rect.unwrap();
        assert_eq!((a.x, a.y, a.h), (100.0, 680.0, 12.0));
    }

    #[test]
    fn lines_follow_the_characters_not_the_engines_breaks() {
        let c = PageChars::from_engine([
            g('a', 100.0, 100.0),
            added('\r'),
            added('\n'),
            g('b', 110.0, 100.0),
            // No break from the engine, but a new line.
            g('c', 100.0, 120.0),
        ]);
        assert_eq!(c.text(0, c.len()), "a b\nc");
        assert_eq!(c.rects(0, c.len()).len(), 2);
    }

    #[test]
    fn a_point_on_a_character_is_the_nearer_boundary() {
        let c = sample();
        assert_eq!(c.boundary_at(shown(102.0, 105.0), 0.0), Some(0));
        assert_eq!(c.boundary_at(shown(108.0, 105.0), 0.0), Some(1));
        assert_eq!(c.boundary_at(shown(300.0, 105.0), 0.0), None);
    }

    #[test]
    fn dragging_off_the_text_still_finds_a_boundary() {
        let c = sample();
        // Right of the first line: its end, before the break.
        assert_eq!(c.boundary_near(shown(400.0, 104.0)), 5);
        // Left of the second line: its start.
        assert_eq!(c.boundary_near(shown(10.0, 125.0)), 6);
        // Above and below everything.
        assert_eq!(c.boundary_near(shown(120.0, 10.0)), 0);
        assert_eq!(c.boundary_near(shown(120.0, 700.0)), c.len());
        // Selecting across the break takes it along.
        assert_eq!(c.text(3, 7), "cd\ne");
        assert_eq!(c.rects(3, 7).len(), 2);
    }

    #[test]
    fn carets_sit_between_characters() {
        let c = sample();
        // After "ab": at b's right edge, straddling the bottom of the text.
        let b = as_shown(c.caret_at(2).unwrap());
        assert_eq!(b.x + b.w / 2.0, 120.0);
        assert!(b.y < 112.0 && b.bottom() > 112.0);
        // At the start of the second line: before "e".
        let e = as_shown(c.caret_at(6).unwrap());
        assert_eq!(e.x + e.w / 2.0, 100.0);
        assert!(e.y > 120.0);
    }

    /// Whatever the page's `/Rotate`, text drawn to read upright as the
    /// page shows reads along its lines there: the turn it's drawn at is
    /// the frame it reads in.
    #[test]
    fn each_rotate_with_its_content_reads_upright() {
        for r in [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270] {
            let page = letter(r);
            let up = |ch, x, y| {
                let shows = Box2 {
                    x,
                    y,
                    w: 10.0,
                    h: 12.0,
                };
                on(r, r.degrees() as f32, ch, shows)
            };
            let c = PageChars::from_engine([
                up('a', 100.0, 100.0),
                up('b', 110.0, 100.0),
                up('c', 100.0, 120.0),
            ]);
            assert_eq!(c.upright, r);
            assert_eq!(c.text(0, c.len()), "ab\nc", "{r:?}");
            assert_eq!(c.rects(0, c.len()).len(), 2, "{r:?}");
            // The right half of 'a' as shown is the boundary after it.
            let on_a = page.from_view().apply(Pt { x: 108.0, y: 105.0 });
            assert_eq!(c.boundary_at(on_a, 0.0), Some(1), "{r:?}");
            // Left of the second line as shown: its start.
            let left = page.from_view().apply(Pt { x: 10.0, y: 125.0 });
            assert_eq!(c.boundary_near(left), 3, "{r:?}");
            // The caret after "ab" is at b's far edge as shown.
            let caret = page.to_view().apply_box(c.caret_at(2).unwrap());
            assert!(
                (caret.x + caret.w / 2.0 - 120.0).abs() < 1e-3,
                "{r:?}: {caret:?}"
            );
        }
    }

    /// Text that runs down an upright page, drawn a quarter turn
    /// clockwise, reads a line at a time as it does turned: down the page,
    /// then the next line, to its left. (Grouped as the page shows, it
    /// read a character a line.)
    #[test]
    fn text_running_down_the_page_reads_as_one_line() {
        let down = |ch, x, y| {
            let shows = Box2 {
                x,
                y,
                w: 12.0,
                h: 10.0,
            };
            on(Rotation::R0, 270.0, ch, shows)
        };
        let c = PageChars::from_engine([
            down('a', 100.0, 100.0),
            down('b', 100.0, 110.0),
            down('c', 100.0, 120.0),
            added('\r'),
            added('\n'),
            down('d', 80.0, 100.0),
            down('e', 80.0, 110.0),
        ]);
        assert_eq!(c.upright, Rotation::R270);
        assert_eq!(c.text(0, c.len()), "abc\nde");
        assert_eq!(c.rects(0, 3).len(), 1);
        // The lower half of 'a' as shown is the boundary after it.
        assert_eq!(c.boundary_at(shown(105.0, 108.0), 0.0), Some(1));
        assert_eq!(c.boundary_at(shown(105.0, 102.0), 0.0), Some(0));
    }

    /// A page's turn is the one most of its characters are drawn at: a
    /// note up the margin doesn't turn the page's text.
    #[test]
    fn the_most_drawn_turn_reads_upright() {
        let mut chars = vec![
            g('a', 100.0, 100.0),
            g('b', 110.0, 100.0),
            g('c', 120.0, 100.0),
        ];
        chars.push(on(
            Rotation::R0,
            89.6,
            'n',
            Box2 {
                x: 20.0,
                y: 300.0,
                w: 12.0,
                h: 10.0,
            },
        ));
        assert_eq!(PageChars::from_engine(chars.clone()).upright, Rotation::R0);
        // Angles near a quarter turn round to it, past a full turn too.
        let turned = chars.iter().map(|&(ch, r, _)| (ch, r, 359.7));
        assert_eq!(PageChars::from_engine(turned).upright, Rotation::R0);
        let up = chars.iter().map(|&(ch, r, _)| (ch, r, 90.4));
        assert_eq!(PageChars::from_engine(up).upright, Rotation::R90);
        assert_eq!(PageChars::from_engine([]).upright, Rotation::R0);
    }

    #[test]
    fn words_come_with_their_boxes() {
        let c = sample();
        let words: Vec<(String, f32)> = c.words().into_iter().map(|(w, _, r)| (w, r.x)).collect();
        assert_eq!(
            words,
            [
                ("ab".into(), 100.0),
                ("cd".into(), 130.0),
                ("ef".into(), 100.0)
            ]
        );
    }

    #[test]
    fn a_double_click_takes_the_word() {
        let c = sample();
        assert_eq!(c.word_at(4), (3, 5));
        assert_eq!(c.word_at(5), (3, 5));
        assert_eq!(c.word_at(0), (0, 2));
        assert_eq!(c.text(3, 5), "cd");
    }

    /// `text` as the engine reads it on an upright page, from (`x`, `y`)
    /// as shown, 10 points a character, each space one it added.
    fn written(text: &str, x: f32, y: f32) -> Vec<Drawn> {
        (text.chars().enumerate())
            .map(|(i, ch)| match ch {
                ' ' => added(' '),
                _ => g(ch, x + 10.0 * i as f32, y),
            })
            .collect()
    }

    /// "Hello World" over "second line", lines 20 points apart.
    fn two_lines() -> PageChars {
        let mut chars = written("Hello World", 100.0, 100.0);
        chars.extend([added('\r'), added('\n')]);
        chars.extend(written("second line", 100.0, 120.0));
        PageChars::from_engine(chars)
    }

    fn literal(query: &str, match_case: bool) -> Query {
        Query::literal(query, match_case).unwrap()
    }

    fn pattern(re: &str) -> Query {
        Query::Pattern(Arc::new(regex::Regex::new(re).unwrap()))
    }

    #[test]
    fn a_literal_spans_words_on_a_line() {
        let c = two_lines();
        let hits = c.find(&literal("lo wor", false));
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].text.as_str(), hits[0].rects.len()), ("lo Wor", 1));
        // From the second "l", 130 across, to the right of "r", 190.
        let r = as_shown(hits[0].rects[0]);
        assert_eq!((r.x, r.right(), r.y, r.h), (130.0, 190.0, 100.0, 12.0));
        assert!(hits[0].groups.is_empty());
        assert!(c.find(&literal("lo wor", true)).is_empty());
        assert!(c.find(&literal("", false)).is_empty());
        assert!(c.find(&literal(" ", false)).is_empty(), "nothing drawn");
    }

    /// A literal's words match where the page's are apart by any
    /// whitespace, as PDFium's Find's did: across a line break, with a box
    /// on each line, and over two spaces or a no-break space; but not
    /// where the page has none.
    #[test]
    fn a_literal_s_words_match_across_lines_and_spaces() {
        let c = two_lines();
        let hits = c.find(&literal("world second", false));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "World\nsecond");
        let rects: Vec<(f32, f32, f32)> = (hits[0].rects.iter())
            .map(|&r| as_shown(r))
            .map(|r| (r.x, r.right(), r.y))
            .collect();
        assert_eq!(rects, [(160.0, 210.0, 100.0), (100.0, 160.0, 120.0)]);
        assert_eq!(c.find(&literal(" world\tsecond ", false)), hits);
        let spaced = PageChars::from_engine(written("total  42\u{a0}due", 0.0, 0.0));
        let hits = spaced.find(&literal("Total 42 due", false));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "total  42\u{a0}due");
        assert!(spaced.find(&literal("total42", false)).is_empty());
        assert_eq!(spaced.find(&literal("42 du", true)).len(), 1);
    }

    #[test]
    fn repeated_matches_follow_one_another() {
        let c = PageChars::from_engine(written("aaaa", 0.0, 0.0));
        let hits = c.find(&literal("aa", true));
        assert_eq!(hits.len(), 2);
        let second = as_shown(hits[1].rects[0]);
        assert_eq!((second.x, second.w), (20.0, 20.0));
    }

    /// A pattern keeps its groups, and characters of more than one byte
    /// before a match don't shift it; one that matches nothing isn't a
    /// hit.
    #[test]
    fn a_pattern_maps_its_bytes_to_characters() {
        let mut chars = written("SSN 123-45-6789", 10.0, 10.0);
        chars.extend([added('\r'), added('\n')]);
        chars.extend(written("état", 10.0, 30.0));
        let c = PageChars::from_engine(chars);
        let hits = c.find(&pattern(r"\b(\d{3})-\d{2}-(\d{4})\b"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "123-45-6789");
        let groups = [Some("123".to_string()), Some("6789".to_string())];
        assert_eq!(hits[0].groups, groups);
        let r = as_shown(hits[0].rects[0]);
        assert_eq!((r.x, r.w), (50.0, 110.0));
        let hits = c.find(&pattern("tat"));
        assert_eq!(hits.len(), 1);
        assert_eq!(as_shown(hits[0].rects[0]).x, 20.0);
        assert!(c.find(&pattern("x*")).is_empty());
        let hits = c.find(&pattern(r"SSN (\d+)|(z)"));
        assert_eq!(hits[0].groups, [Some("123".to_string()), None]);
    }

    /// On a page whose text the file turns with it, a match's box is its
    /// characters' in PageSpace, across the page as it's drawn.
    #[test]
    fn hits_on_turned_text_are_in_page_space() {
        let page = Rotation::R90;
        let up = |ch, x| {
            let shows = Box2 {
                x,
                y: 100.0,
                w: 10.0,
                h: 12.0,
            };
            on(page, 90.0, ch, shows)
        };
        let c = PageChars::from_engine([up('H', 100.0), up('i', 110.0), up('!', 120.0)]);
        let hits = c.find(&literal("i!", true));
        assert_eq!(hits.len(), 1);
        let i = c.glyphs[1].rect.unwrap();
        let bang = c.glyphs[2].rect.unwrap();
        assert_eq!(hits[0].rects, [i.union(bang)]);
        // It runs up PageSpace: as tall as two characters are wide.
        assert_eq!((hits[0].rects[0].w, hits[0].rects[0].h), (12.0, 20.0));
    }

    /// Spaces and line breaks don't count, drawn or added, nor does a
    /// character with no box.
    #[test]
    fn text_len_counts_what_s_drawn_but_whitespace() {
        assert_eq!(sample().text_len(), 6);
        let mut chars = written("a b", 0.0, 0.0);
        chars.extend([g(' ', 40.0, 0.0), ('c', None, 0.0), g('d', 50.0, 0.0)]);
        assert_eq!(PageChars::from_engine(chars).text_len(), 3);
        assert_eq!(PageChars::from_engine([]).text_len(), 0);
    }
}
