//! Photos and scans of paper to a clean PDF. `sheaf scan` ([`scan`])
//! takes the image files' pages through the intake ([`crate::intake`])
//! into a document of no pages, then writes it whole; File > Create > From
//! Scans/Photos streams them into an untitled document in the app, with
//! the same options and engine ([`engine`]). Each page is cleaned up
//! (`sheaf_image::clean`), its steps as its [`Profile`] starts them, and
//! can be OCRed so the text is searchable; OCR reads the very image that
//! becomes the page, so the text lines up with it.

use std::path::{Path, PathBuf};

use anyhow::Result;
use in_progress::Progress;
use serde::{Deserialize, Serialize};
use sheaf_doc::materialize::AskAnswers;
use sheaf_image::clean::ScanParams;
use sheaf_ocr::{OcrConfig, OcrEngine};

use crate::images::Paper;
use crate::intake::{Batching, Files, SessionLanding, intake};
use crate::session::{Decided, SaveTo, Session};

/// Where the pages come from, which sets cleanup's steps (M5 proposal 3);
/// each can still be switched:
///
/// | Step | Photo | Scanner |
/// |---|---|---|
/// | Find the page, clear its edges | on | off |
/// | Flatten curl (UVDoc) | off | off |
/// | Even out lighting, white paper | on | off |
/// | Turn upright | on | on |
/// | Straighten | on | on |
/// | Leave out blank pages | off | off |
///
/// Black and white is Sauvola's on the cleaned page in both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    /// Photos of paper: the page is found in the frame and cut out, and
    /// the light evened out.
    #[default]
    Photo,
    /// A scanner's or copier's pages (a scan-to-folder's files included):
    /// the image is the page and its light is even already, and a white
    /// page on a flatbed's white lid has no edge to find.
    Scanner,
}

impl Profile {
    /// The profile's options: Letter at 300 dpi in color, its steps as
    /// the table says. Files have no duplex backs, so neither leaves out
    /// blank pages; a device's duplex scan may (slice 6).
    pub fn options(self) -> ScanOptions {
        match self {
            Profile::Photo => ScanOptions::default(),
            Profile::Scanner => ScanOptions {
                clean: ScanParams {
                    crop: false,
                    flatten: false,
                    ..ScanParams::default()
                },
                ..ScanOptions::default()
            },
        }
    }
}

/// Each profile's last choices, and the profile used last: what Create
/// from Scans remembers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanChoices {
    pub profile: Profile,
    pub photo: ScanOptions,
    pub scanner: ScanOptions,
}

impl Default for ScanChoices {
    fn default() -> Self {
        Self {
            profile: Profile::Photo,
            photo: Profile::Photo.options(),
            scanner: Profile::Scanner.options(),
        }
    }
}

impl ScanChoices {
    pub fn get(&self, profile: Profile) -> &ScanOptions {
        match profile {
            Profile::Photo => &self.photo,
            Profile::Scanner => &self.scanner,
        }
    }

    pub fn set(&mut self, profile: Profile, opts: ScanOptions) {
        match profile {
            Profile::Photo => self.photo = opts,
            Profile::Scanner => self.scanner = opts,
        }
    }
}

/// What a scan does. The default is the photo profile's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ScanOptions {
    #[serde(flatten)]
    pub clean: ScanParams,
    pub page: Paper,
    /// Output resolution.
    pub dpi: u32,
    /// Recognize text with the configured OCR engine.
    pub ocr: bool,
    /// Leave out pages that come out blank (duplex backs, separators).
    /// Only clearly blank ones: a nearly blank page stays.
    pub drop_blanks: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            clean: ScanParams::default(),
            page: Paper::Letter,
            dpi: 300,
            ocr: false,
            drop_blanks: false,
        }
    }
}

impl ScanOptions {
    /// The cleanup parameters, with the pixel size the paper and
    /// resolution allow.
    pub fn params(&self) -> ScanParams {
        let inches = match self.page {
            Paper::Letter => Some((8.5, 11.0)),
            Paper::A4 => Some((8.27, 11.69)),
            Paper::Original => None,
        };
        ScanParams {
            fit: inches.map(|(w, h): (f32, f32)| {
                (
                    (w * self.dpi as f32).round() as u32,
                    (h * self.dpi as f32).round() as u32,
                )
            }),
            ..self.clean.clone()
        }
    }
}

/// What a scan did, page by page.
#[derive(Debug, Default)]
pub struct ScanReport {
    pub pages: usize,
    /// Pages found in the photo and cut out.
    pub cropped: usize,
    /// Words recognized, when OCR ran.
    pub words: usize,
    /// Blank pages left out.
    pub dropped: usize,
}

/// Clean up an image, with the orientation model when it's there,
/// showing `stage` each step.
pub fn clean_image(
    image: image::RgbImage,
    params: &ScanParams,
    progress: &dyn Progress,
    stage: &mut dyn FnMut(sheaf_image::clean::Stage, &image::RgbImage),
) -> Result<sheaf_image::clean::Cleaned> {
    let orienter = |img: &image::RgbImage| orientation_votes(img);
    let unwarper = |img: &image::RgbImage| sheaf_ocr::rectify::unwarp(img);
    let models = sheaf_image::clean::Models {
        orient: (params.orient
            && (sheaf_ocr::orient::available() || sheaf_ocr::orient::osd_available()))
        .then_some(&orienter as sheaf_image::clean::Orienter),
        unwarp: (params.flatten_curl && sheaf_ocr::rectify::available())
            .then_some(&unwarper as sheaf_image::clean::Unwarper),
    };
    if params.flatten_curl && models.unwarp.is_none() {
        anyhow::bail!(
            "can't flatten curled pages: {}",
            sheaf_ocr::rectify::unavailable_reason().unwrap_or_default()
        );
    }
    sheaf_image::clean::clean_with(image, params, models, progress, stage)
}

/// Votes for which way up a page goes, from the models there are:
/// PaddleOCR's orientation classifier, averaged over the page turned all
/// four ways (on its own it has favourite answers), and Tesseract's
/// orientation detection on the gray page and a black-and-white copy,
/// each weighted by its confidence. On ten phone photos of engineering
/// drawings neither was right alone; together they were.
pub fn orientation_votes(img: &image::RgbImage) -> Option<[f32; 4]> {
    let mut votes = [0f32; 4];
    let mut any = false;
    if sheaf_ocr::orient::available() {
        let s = (1000.0 / img.width().max(img.height()) as f32).min(1.0);
        let small = image::imageops::resize(
            img,
            ((img.width() as f32 * s) as u32).max(1),
            ((img.height() as f32 * s) as u32).max(1),
            image::imageops::FilterType::Triangle,
        );
        if let Some(sc) = sheaf_ocr::orient::scores(&small) {
            votes = sc;
            any = true;
        }
    }
    if sheaf_ocr::orient::osd_available() {
        let gray = image::DynamicImage::ImageRgb8(img.clone()).into_luma8();
        let bw = sheaf_image::clean::sauvola(&gray, (gray.width() / 50).max(15) | 1, 0.25);
        for im in [&gray, &bw] {
            if let Some((k, conf)) = sheaf_ocr::orient::osd(im, 300) {
                votes[k as usize] += (conf / 8.0).clamp(0.0, 1.0);
                any = true;
            }
        }
    }
    any.then_some(votes)
}

/// How the command line lands its pages: 8 at a time, never by the
/// clock, so the same images make the same file (recognized text's block
/// ids aside, which are drawn at random). A Rewrite numbers what an insert
/// brought by its source, each batch's own, so batches cut where a clock
/// says would write the same pages in another order.
pub const BATCHING: Batching = Batching {
    pages: 8,
    wait: None,
};

/// The engine `ocr` sets up, checked and built once, that an intake
/// recognizes its pages with when `opts.ocr` is on; None when it's off.
pub fn engine(opts: &ScanOptions, ocr: &OcrConfig) -> Result<Option<Box<dyn OcrEngine>>> {
    if !opts.ocr {
        return Ok(None);
    }
    sheaf_ocr::check_engine(ocr.engine, ocr)?;
    Ok(Some(sheaf_ocr::build_engine(ocr.engine, ocr)?))
}

/// Clean up `inputs` and write them to `out` as a PDF, a page each (a
/// TIFF's pages each a page). `ocr` is the engine setup to use when
/// `opts.ocr` is on.
pub fn scan(
    inputs: &[PathBuf],
    opts: &ScanOptions,
    ocr: &OcrConfig,
    out: &Path,
    progress: &dyn Progress,
) -> Result<ScanReport> {
    let mut files = Files::new(inputs)?;
    let mut engine = engine(opts, ocr)?;
    // The pages made in memory, which a document of no pages takes in, the
    // recognized text its own (M10.5 §3.9).
    let mut session = Session::new_empty();
    let mut landing = SessionLanding::new(&mut session, "Scan");
    let report = intake(
        &mut files,
        opts,
        engine.as_deref_mut().map(|e| e as &mut dyn OcrEngine),
        &mut landing,
        BATCHING,
        progress,
    )?;
    landing.finish();
    progress.check()?;
    if report.pages == 0 {
        anyhow::bail!("every page came out blank");
    }
    let to = SaveTo::File {
        path: out.to_path_buf(),
        append: false,
    };
    // A file of its own: nothing to ask.
    let writing = progress.child("writing", 0.0);
    session
        .save(to, &AskAnswers::default(), &*writing)
        .decided()?;
    Ok(report)
}

/// Whether a cleaned page is blank enough to leave out.
pub fn is_blank_page(image: &image::DynamicImage) -> bool {
    matches!(
        sheaf_image::blank::judge_image(image, sheaf_image::blank::Sensitivity::Normal),
        sheaf_image::blank::Verdict::Blank { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letter_at_300_dpi_fits_2550_by_3300() {
        let opts = ScanOptions::default();
        assert_eq!(opts.params().fit, Some((2550, 3300)));
        let opts: ScanOptions =
            serde_json::from_str(r#"{"page":"original","mode":"bw","ocr":true}"#).unwrap();
        assert_eq!(opts.params().fit, None);
        assert_eq!(opts.clean.mode, sheaf_image::clean::Mode::Bw);
        assert!(opts.ocr && opts.clean.deskew);
    }

    #[test]
    fn the_scanner_profile_neither_crops_nor_evens_the_light() {
        assert_eq!(Profile::Photo.options(), ScanOptions::default());
        let s = Profile::Scanner.options();
        let c = &s.clean;
        assert!(!c.crop && !c.flatten && !c.flatten_curl);
        assert!(c.orient && c.deskew);
        assert!(!s.drop_blanks && !s.ocr);
        assert_eq!(
            (c.mode, s.page, s.dpi),
            (sheaf_image::clean::Mode::Color, Paper::Letter, 300)
        );
        // The rest is the photo profile's.
        let mut p = s.clone();
        (p.clean.crop, p.clean.flatten) = (true, true);
        assert_eq!(p, ScanOptions::default());
    }

    #[test]
    fn each_profile_keeps_its_own_choices() {
        let mut choices = ScanChoices::default();
        assert_eq!(choices.get(Profile::Photo), &Profile::Photo.options());
        assert_eq!(choices.get(Profile::Scanner), &Profile::Scanner.options());
        let mut bw = Profile::Scanner.options();
        bw.clean.mode = sheaf_image::clean::Mode::Bw;
        bw.dpi = 200;
        choices.set(Profile::Scanner, bw.clone());
        choices.profile = Profile::Scanner;
        assert_eq!(choices.get(Profile::Scanner), &bw);
        assert_eq!(choices.get(Profile::Photo), &Profile::Photo.options());
        let json = serde_json::to_string(&choices).unwrap();
        assert_eq!(serde_json::from_str::<ScanChoices>(&json).unwrap(), choices);
        // Nothing remembered is each profile's own defaults.
        assert_eq!(
            serde_json::from_str::<ScanChoices>("{}").unwrap(),
            ScanChoices::default()
        );
    }
}
