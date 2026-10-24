//! `sheaf scan`: photos of paper, or a scanner's pages, to a clean PDF.

use std::path::PathBuf;

use anyhow::bail;
use clap::{Args, ValueEnum};
use sheaf_image::clean::Mode;
use sheaf_ocr::{EngineKind, OcrConfig};
use sheaf_ops::scan::{Profile, ScanOptions};

use crate::images::PaperArg;

#[derive(Args)]
#[command(
    about = "Clean up photos of paper, or a scanner's pages, into a PDF",
    long_about = "Clean up photos of paper, or a scanner's or copier's pages, into a PDF, a \
                  page each. Photos (the default): find the page in the photo and \
                  straighten it, even out shadows and make the paper white, turn it \
                  upright and level the text. A scanner's or copier's pages (--profile \
                  scanner, a scan-to-folder's files too): the image is the page and its \
                  light is even already, so they're only turned upright and levelled. A \
                  step's flag turns it on or off whatever the profile."
)]
pub struct ScanArgs {
    /// The images, in page order. A multi-page TIFF gives all its pages.
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
    /// The PDF to write.
    #[arg(short, long)]
    output: PathBuf,
    /// Where the pages come from, which decides each step below unless its
    /// flag is given.
    #[arg(long, value_enum, default_value_t = ProfileArg::Photo)]
    profile: ProfileArg,
    /// Color, gray, or black and white (default: color).
    #[arg(long, value_enum)]
    mode: Option<ModeArg>,
    /// Page size (default: letter). `original` keeps the cleaned image's
    /// size at --dpi.
    #[arg(long, value_enum)]
    page: Option<PaperArg>,
    /// Output resolution (default: 300).
    #[arg(long, value_parser = clap::value_parser!(u32).range(72..=1200))]
    dpi: Option<u32>,
    /// Recognize the text, so the PDF can be searched and copied from.
    #[arg(long)]
    ocr: bool,
    /// OCR engine (default: tesseract).
    #[arg(long, value_enum)]
    engine: Option<EngineArg>,
    /// OCR language, e.g. "eng" or "eng+deu" (tesseract) or a PaddleOCR set.
    #[arg(long)]
    lang: Option<String>,
    /// Find the page in the photo and cut it out (the photo profile's
    /// default).
    #[arg(long, overrides_with = "no_crop")]
    crop: bool,
    /// Keep the whole image: don't look for the page's edges (the scanner
    /// profile's default).
    #[arg(long, overrides_with = "crop")]
    no_crop: bool,
    /// Flatten curled or bent pages (book pages, folded letters) with the
    /// UVDoc model. Slower; can make flat pages wobble slightly. Needs
    /// scripts/fetch-ocr-data.sh --uvdoc.
    #[arg(long)]
    flatten_curl: bool,
    /// Even out shadows and color casts and make the paper white (the
    /// photo profile's default).
    #[arg(long, overrides_with = "no_flatten")]
    flatten: bool,
    /// Leave lighting and colors as they are (the scanner profile's
    /// default).
    #[arg(long, overrides_with = "flatten")]
    no_flatten: bool,
    /// Don't turn pages upright.
    #[arg(long)]
    no_orient: bool,
    /// Don't level the text.
    #[arg(long)]
    no_deskew: bool,
    /// Leave out pages that come out blank (duplex backs, separators).
    #[arg(long)]
    drop_blanks: bool,
    /// Black-and-white threshold strength (Sauvola k, 0.05–0.6): higher
    /// drops more faint marks (default: 0.25).
    #[arg(long)]
    threshold: Option<f32>,
    /// Replace the output if it's already there.
    #[arg(long)]
    force: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum ProfileArg {
    /// Photos of paper: find the page, even out the light.
    Photo,
    /// A scanner's or copier's pages: the image is the page, its light
    /// even.
    Scanner,
}

impl From<ProfileArg> for Profile {
    fn from(p: ProfileArg) -> Profile {
        match p {
            ProfileArg::Photo => Profile::Photo,
            ProfileArg::Scanner => Profile::Scanner,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Color,
    Gray,
    /// Black and white.
    Bw,
}

#[derive(Clone, Copy, ValueEnum)]
enum EngineArg {
    Tesseract,
    #[value(alias = "paddle")]
    Oar,
}

pub fn run(args: ScanArgs, quiet: bool) -> anyhow::Result<()> {
    if args.output.exists() && !args.force {
        bail!(
            "{} already exists (--force replaces)",
            args.output.display()
        );
    }
    let opts = options(&args);
    let mut ocr = OcrConfig::default();
    if let Some(e) = args.engine {
        ocr.engine = match e {
            EngineArg::Tesseract => EngineKind::Tesseract,
            EngineArg::Oar => EngineKind::Oar,
        };
    }
    if let Some(lang) = &args.lang {
        sheaf_ocr::set_language(ocr.engine, &mut ocr, lang);
    }
    let report = crate::progress::run("scanning", quiet, |p| {
        sheaf_ops::scan::scan(&args.inputs, &opts, &ocr, &args.output, p)
    })?;
    if !quiet {
        eprintln!(
            "{} page{}{}{}{}",
            report.pages,
            if report.pages == 1 { "" } else { "s" },
            if opts.clean.crop {
                format!(", {} cropped from the photo", report.cropped)
            } else {
                String::new()
            },
            if report.dropped > 0 {
                format!(", {} blank left out", report.dropped)
            } else {
                String::new()
            },
            if opts.ocr {
                format!(", {} words recognized", report.words)
            } else {
                String::new()
            }
        );
    }
    println!("{}", args.output.display());
    Ok(())
}

/// The profile's options, with each step's flag applied when given.
fn options(args: &ScanArgs) -> ScanOptions {
    let mut opts = Profile::from(args.profile).options();
    let c = &mut opts.clean;
    if let Some(mode) = args.mode {
        c.mode = match mode {
            ModeArg::Color => Mode::Color,
            ModeArg::Gray => Mode::Gray,
            ModeArg::Bw => Mode::Bw,
        };
    }
    // Of a pair, clap keeps only the one given last.
    if args.crop || args.no_crop {
        c.crop = args.crop;
    }
    if args.flatten || args.no_flatten {
        c.flatten = args.flatten;
    }
    c.flatten_curl |= args.flatten_curl;
    c.orient &= !args.no_orient;
    c.deskew &= !args.no_deskew;
    if let Some(k) = args.threshold {
        c.threshold_k = k.clamp(0.05, 0.6);
    }
    if let Some(page) = args.page {
        opts.page = page.into();
    }
    if let Some(dpi) = args.dpi {
        opts.dpi = dpi;
    }
    opts.ocr |= args.ocr;
    opts.drop_blanks |= args.drop_blanks;
    opts
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Line {
        #[command(flatten)]
        args: ScanArgs,
    }

    /// `sheaf scan a.png -o out.pdf` and then `flags`, as options.
    fn parse(flags: &str) -> ScanOptions {
        let line = ["scan", "a.png", "-o", "out.pdf"]
            .into_iter()
            .chain(flags.split_whitespace());
        options(&Line::try_parse_from(line).unwrap().args)
    }

    #[test]
    fn the_profile_decides_what_no_flag_does() {
        assert_eq!(parse(""), Profile::Photo.options());
        assert_eq!(parse("--profile photo"), Profile::Photo.options());
        assert_eq!(parse("--profile scanner"), Profile::Scanner.options());
        let o = parse("--profile scanner --crop --mode bw --dpi 200");
        assert!(o.clean.crop && !o.clean.flatten);
        assert_eq!((o.clean.mode, o.dpi), (Mode::Bw, 200));
        let o = parse("--no-flatten --no-orient --drop-blanks --ocr");
        assert!(o.clean.crop && !o.clean.flatten && !o.clean.orient && o.clean.deskew);
        assert!(o.drop_blanks && o.ocr);
    }

    #[test]
    fn of_a_flag_and_its_no_the_last_wins() {
        assert!(!parse("--crop --no-crop").clean.crop);
        assert!(parse("--no-crop --crop").clean.crop);
        assert!(
            !parse("--profile scanner --flatten --no-flatten")
                .clean
                .flatten
        );
        assert!(
            parse("--profile scanner --no-flatten --flatten")
                .clean
                .flatten
        );
    }
}
