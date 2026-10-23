//! Images to a PDF, one page each (`sheaf images-to-pdf`).

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use in_progress::{Progress, Unit};
use serde::{Deserialize, Serialize};
use sheaf_doc::materialize::AskAnswers;
use sheaf_edit::Fit;
use sheaf_image::io::PageImage;

use crate::session::{Decided, SaveTo, Session};

/// Page size for pages made from images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Paper {
    /// The image's own size at its resolution.
    #[default]
    Original,
    Letter,
    A4,
}

impl Paper {
    /// The fit, with `margin` points around a fitted image.
    pub fn fit(self, margin: f32) -> Fit {
        let with = |f: Fit| match f {
            Fit::Paper { width, height, .. } => Fit::Paper {
                width,
                height,
                margin,
            },
            f => f,
        };
        match self {
            Paper::Original => Fit::Original,
            Paper::Letter => with(Fit::LETTER),
            Paper::A4 => with(Fit::A4),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ImagesParams {
    pub page: Paper,
    /// Resolution to use instead of what the images record.
    pub dpi: Option<f32>,
    /// Points of white around each image on a fitted page.
    pub margin: f32,
}

/// Read each image file (a TIFF may hold several pages), in order.
pub fn read_all(
    inputs: &[PathBuf],
    dpi: Option<f32>,
    progress: &dyn Progress,
) -> Result<Vec<PageImage>> {
    if inputs.is_empty() {
        bail!("no images given");
    }
    progress.set_total(Some(inputs.len() as u64), Unit::ITEMS);
    let mut pages = Vec::new();
    for (i, path) in inputs.iter().enumerate() {
        progress.check()?;
        let mut read = sheaf_image::io::read(path)?;
        if let Some(d) = dpi.filter(|d| *d > 0.0) {
            for p in &mut read {
                p.dpi = (d, d);
            }
        }
        pages.extend(read);
        progress.set(i as u64 + 1);
    }
    Ok(pages)
}

/// Write `out` as a PDF of `inputs`, one page per image: the pages made in
/// memory, as a scan's are, inserted into a document of no pages and
/// saved there, durably.
pub fn images_to_pdf(
    inputs: &[PathBuf],
    params: &ImagesParams,
    out: &Path,
    progress: &dyn Progress,
) -> Result<()> {
    let pages = read_all(inputs, params.dpi, &*progress.child("reading", 0.8))?;
    progress.check()?;
    let mut session = Session::new_empty();
    let fit = params.page.fit(params.margin);
    let insert = crate::intake::made_pages(&mut session, &pages, fit, "images")?;
    session.commit(vec![insert], "Images to PDF")?;
    let to = SaveTo::File {
        path: out.to_path_buf(),
        append: false,
    };
    // A file of its own: nothing to ask.
    let writing = progress.child("writing", 0.2);
    session
        .save(to, &AskAnswers::default(), &*writing)
        .decided()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_from_json() {
        let p: ImagesParams = serde_json::from_str(r#"{"page":"a4","margin":36}"#).unwrap();
        assert_eq!(p.page, Paper::A4);
        assert_eq!(p.margin, 36.0);
        assert_eq!(p.dpi, None);
        assert!(matches!(p.page.fit(36.0), Fit::Paper { margin: 36.0, .. }));
    }
}
