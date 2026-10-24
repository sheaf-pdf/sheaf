//! Kodak patch codes: four bars along a sheet's edge, which scanners and
//! capture software read as "a batch starts here" or "a document starts
//! here". They carry a type and no data, and no barcode library reads
//! them, so sheaf does.
//!
//! Kodak's A-61599 (2006) sets the geometry:
//! - bars parallel to the edge: wide ones 0.20 in, narrow ones and the
//!   spaces 0.08 in, 0.80 in in all;
//! - at least 0.75 in long (2 in for Kodak's Patch-Reader scanners), at
//!   least 0.25 in from the page's edges and other print, within 3.75 in
//!   of the leading edge;
//! - under 20% reflectance on paper over 65%, and never photocopied.
//!
//! Kodak's printed sheets ("Patches for Printing") and Alliance's put the
//! bars along all four edges, about 0.62 in in.
//!
//! The type is the bars' pattern read from the edge inward (W wide, N
//! narrow): 1 WWNN, 2 WNNW, 3 WNWN, 4 NWWN, 6 NNWW, T NWNW. Reading from
//! the edge the bars lie along tells 3 from T and 1 from 6 on a sheet fed
//! turned or upside down.
//!
//! The page is reduced to about 75 dpi and split into dark and light at
//! its Otsu level. Lines about 0.04 in apart cross a band along each edge,
//! 4.7 in deep (A-61599's 3.75 in plus the bars) or half the page. A line
//! reads a patch where it crosses light wider than a space, four bars and
//! three spaces of a patch's widths, then light again; lines that agree
//! on the type and on where the bars start, for at least 0.6 in along the
//! edge (under A-61599's 0.75, for a skewed or clipped sheet), are a
//! patch. Bars
//! across the leading edge (Kodak's i280 layout) aren't read. The M5
//! bench's version read 204 of 204 synthetic pages and 60 of 60 renders of
//! Kodak's sheets, and nothing on 554 pages without, in about 5 ms a page.

use std::ops::RangeInclusive;

use image::GrayImage;

#[cfg(test)]
mod tests;

/// A patch code's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PatchCode {
    One,
    Two,
    Three,
    Four,
    Six,
    T,
}

impl PatchCode {
    pub const ALL: [PatchCode; 6] = [
        PatchCode::One,
        PatchCode::Two,
        PatchCode::Three,
        PatchCode::Four,
        PatchCode::Six,
        PatchCode::T,
    ];

    /// The bars from the edge inward, true for a wide one.
    pub fn bars(self) -> [bool; 4] {
        match self {
            PatchCode::One => [true, true, false, false],
            PatchCode::Two => [true, false, false, true],
            PatchCode::Three => [true, false, true, false],
            PatchCode::Four => [false, true, true, false],
            PatchCode::Six => [false, false, true, true],
            PatchCode::T => [false, true, false, true],
        }
    }

    /// The type of `bars`, from the edge inward: every pattern of two wide
    /// bars is one, any other none.
    fn of(bars: [bool; 4]) -> Option<PatchCode> {
        PatchCode::ALL.into_iter().find(|c| c.bars() == bars)
    }
}

/// An edge of the page image, as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// A patch code found along an edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patch {
    pub code: PatchCode,
    pub edge: Edge,
    /// How far the first bar is from the edge, in inches (the lines'
    /// mean).
    pub inset: f32,
    /// How far along the edge the lines that read it reach, in inches.
    pub length: f32,
}

/// The resolution the bars are read at.
const WORK_DPI: f32 = 75.0;
/// How deep the band along each edge is, in inches, at most half the
/// page.
const BAND: f32 = 4.7;
/// How far apart the lines across a band are, in inches, at most (a whole
/// number of pixels: 0.04 in at 75 dpi).
const STEP: f32 = 0.05;
/// The light on each side of the bars, at least, in inches: wider than a
/// space (so a run of more than four bars isn't read from its middle),
/// under A-61599's 0.25 in from other print.
const QUIET: f32 = 0.18;
/// The four bars and three spaces in all, in inches (0.80 printed).
const SPAN: RangeInclusive<f32> = 0.62..=1.0;
/// A bar, in inches (0.08 or 0.20 printed), wide over [`WIDE`].
const BAR: RangeInclusive<f32> = 0.03..=0.32;
const WIDE: f32 = 0.14;
/// A space, in inches (0.08 printed).
const SPACE: RangeInclusive<f32> = 0.025..=0.14;
/// How far a line's bars may start from the last agreeing line's, in
/// inches: enough for a skewed sheet's bars to drift along a long edge.
const DRIFT: f32 = 0.08;
/// How far apart agreeing lines may be, in lines.
const GAP: usize = 3;
/// A patch: lines that agree for at least this far along the edge, in
/// inches, and at least this many of them.
const LENGTH: f32 = 0.6;
const LINES: usize = 8;

/// The patch codes along `page`'s edges, `dpi` its resolution: every
/// stretch of lines that agree, so a sheet with bars on all four edges
/// has four or more.
pub fn patches(page: &GrayImage, dpi: f32) -> Vec<Patch> {
    let factor = (dpi / WORK_DPI).round().max(1.0) as u32;
    let work = reduced(page, factor);
    let dpi = dpi / factor as f32;
    let inches = |px: usize| px as f32 / dpi;
    let level = otsu_level(&work);
    let (w, h) = (work.width() as usize, work.height() as usize);
    let raw = work.as_raw();
    let step = ((STEP * dpi) as usize).max(1);
    let mut found = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        let (along, depth) = match edge {
            Edge::Top | Edge::Bottom => (w, h),
            Edge::Left | Edge::Right => (h, w),
        };
        let band = ((BAND * dpi) as usize).min(depth / 2);
        // `d` pixels in from the edge, `a` along it.
        let dark = |a: usize, d: usize| {
            let (x, y) = match edge {
                Edge::Top => (a, d),
                Edge::Bottom => (a, h - 1 - d),
                Edge::Left => (d, a),
                Edge::Right => (w - 1 - d, a),
            };
            raw[y * w + x] <= level
        };
        let mut reads = Vec::new();
        for a in (0..along).step_by(step) {
            runs.clear();
            for d in 0..band {
                let dark = dark(a, d);
                match runs.last_mut() {
                    Some(run) if run.dark == dark => run.len += 1,
                    _ => runs.push(Run {
                        dark,
                        start: d,
                        len: 1,
                    }),
                }
            }
            for window in runs.windows(9) {
                if let Some(code) = read(window, dpi) {
                    reads.push(Read {
                        along: a,
                        inset: window[1].start,
                        code,
                    });
                }
            }
        }
        let drift = (DRIFT * dpi) as usize;
        for s in agreeing(reads, step, drift) {
            if inches(s.length) >= LENGTH && s.lines >= LINES {
                found.push(Patch {
                    code: s.code,
                    edge,
                    inset: inches(s.insets) / s.lines as f32,
                    length: inches(s.length),
                });
            }
        }
    }
    found
}

/// The patch code on `page`, `dpi` its resolution: of [`patches`], the
/// type read along the most length.
pub fn patch(page: &GrayImage, dpi: f32) -> Option<PatchCode> {
    strongest(&patches(page, dpi))
}

/// Of `found`, the type read along the most length; the first of
/// [`PatchCode::ALL`] on a tie.
fn strongest(found: &[Patch]) -> Option<PatchCode> {
    let mut best: Option<(PatchCode, f32)> = None;
    for code in PatchCode::ALL {
        let length: f32 = found
            .iter()
            .filter(|p| p.code == code)
            .map(|p| p.length)
            .sum();
        if length > 0.0 && best.is_none_or(|(_, most)| length > most) {
            best = Some((code, length));
        }
    }
    best.map(|(code, _)| code)
}

/// A run of dark or light pixels along a line, from the edge inward.
struct Run {
    dark: bool,
    start: usize,
    len: usize,
}

/// A line's patch: `along` the edge and the first bar's `inset`, in work
/// pixels.
struct Read {
    along: usize,
    inset: usize,
    code: PatchCode,
}

/// The patch code nine runs of a line make, `dpi` its resolution: light,
/// four bars and three spaces of a patch's widths, light.
fn read(runs: &[Run], dpi: f32) -> Option<PatchCode> {
    let inches = |run: &Run| run.len as f32 / dpi;
    if !runs[1].dark || inches(&runs[0]) < QUIET || inches(&runs[8]) < QUIET {
        return None;
    }
    let span: f32 = runs[1..8].iter().map(inches).sum();
    let mut spaces = runs[2..7].iter().step_by(2).map(inches);
    if !SPAN.contains(&span) || !spaces.all(|s| SPACE.contains(&s)) {
        return None;
    }
    let mut bars = [false; 4];
    for (wide, bar) in bars.iter_mut().zip(runs[1..8].iter().step_by(2)) {
        let width = inches(bar);
        if !BAR.contains(&width) {
            return None;
        }
        *wide = width > WIDE;
    }
    PatchCode::of(bars)
}

/// Lines along an edge that agree on a patch.
struct Stretch {
    code: PatchCode,
    lines: usize,
    /// The lines' insets added up, and how far along the edge they reach,
    /// in work pixels.
    insets: usize,
    length: usize,
}

/// `reads` along one edge, gathered into stretches of the same type whose
/// lines are at most [`GAP`] lines apart (`step` a line's spacing) and
/// whose bars start within `drift` of the last line's, in work pixels.
fn agreeing(mut reads: Vec<Read>, step: usize, drift: usize) -> Vec<Stretch> {
    reads.sort_by_key(|r| (r.code as u8, r.along));
    let mut taken = vec![false; reads.len()];
    let mut stretches = Vec::new();
    for first in 0..reads.len() {
        if taken[first] {
            continue;
        }
        taken[first] = true;
        let (code, from) = (reads[first].code, reads[first].along);
        let (mut last, mut lines, mut insets) = (first, 1, reads[first].inset);
        for next in first + 1..reads.len() {
            let (r, l) = (&reads[next], &reads[last]);
            if r.code != code || r.along > l.along + GAP * step {
                break;
            }
            if taken[next] || r.along == l.along || r.inset.abs_diff(l.inset) > drift {
                continue;
            }
            taken[next] = true;
            (last, lines, insets) = (next, lines + 1, insets + r.inset);
        }
        stretches.push(Stretch {
            code,
            lines,
            insets,
            length: reads[last].along - from,
        });
    }
    stretches
}

/// `page` reduced `factor` times, each pixel the mean of a box of
/// `factor` × `factor`; a part box at the right or the bottom is left out.
fn reduced(page: &GrayImage, factor: u32) -> GrayImage {
    let (w, h) = (page.width() / factor, page.height() / factor);
    if factor == 1 || w == 0 || h == 0 {
        return image::imageops::crop_imm(page, 0, 0, w, h).to_image();
    }
    let (f, width) = (factor as usize, page.width() as usize);
    let n = factor * factor;
    let mut sums = vec![0u32; w as usize];
    let mut out = Vec::with_capacity(w as usize * h as usize);
    for band in page.as_raw().chunks_exact(width * f) {
        sums.fill(0);
        for row in band.chunks_exact(width) {
            for (sum, px) in sums.iter_mut().zip(row.chunks_exact(f)) {
                *sum += px.iter().map(|&v| u32::from(v)).sum::<u32>();
            }
        }
        out.extend(sums.iter().map(|&s| ((s + n / 2) / n) as u8));
    }
    GrayImage::from_raw(w, h, out).expect("a reduced page")
}

/// `image`'s Otsu level: the gray that splits it into dark (at or under
/// it) and light with the most variance between the two. imageproc's
/// `otsu_level` gives the same level but counts in u32, which overflows on
/// a page of more than about 16.8 M pixels of one gray.
fn otsu_level(image: &GrayImage) -> u8 {
    let mut histogram = [0u64; 256];
    for &v in image.as_raw() {
        histogram[usize::from(v)] += 1;
    }
    let total = image.as_raw().len() as u64;
    let sum: f64 = (0u64..)
        .zip(histogram)
        .map(|(gray, count)| (gray * count) as f64)
        .sum();
    let (mut weight, mut below) = (0u64, 0.0);
    let (mut most, mut level) = (0.0, 0u8);
    for (gray, count) in (0u8..=255).zip(histogram) {
        weight += count;
        if weight == 0 {
            continue;
        }
        let above = total - weight;
        if above == 0 {
            break;
        }
        below += (u64::from(gray) * count) as f64;
        let means = below / weight as f64 - (sum - below) / above as f64;
        let variance = weight as f64 * above as f64 * means.powi(2);
        if variance > most {
            (most, level) = (variance, gray);
        }
    }
    level
}
