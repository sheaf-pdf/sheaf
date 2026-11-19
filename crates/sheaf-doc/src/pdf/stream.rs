//! Stream compression and bounded filter decoding through `pdf-filters` with
//! `Backend::ZlibRs` (M18 slices 11–12, D4).

use pdf_filters::{Backend, Encoder, Filter, Level, Prediction, Predictor};
use pdf_objects::{Resolve, Stream};
use pdf_structure::write::Deflate;

use super::{Error, Limit, ReadLimits, Result};

/// Workspace Flate writer settings (`Backend::ZlibRs`, default level 6).
#[must_use]
pub fn deflate_options() -> Deflate {
    let mut deflate = Deflate::DEFAULT;
    deflate.backend = Backend::ZlibRs;
    deflate
}

/// Deflates `data` at default level 6 with `Backend::ZlibRs`.
#[must_use]
pub fn deflate(data: &[u8]) -> Vec<u8> {
    deflate_with(data, Level::DEFAULT)
}

/// Deflates `data` at best compression (`Level::BEST`) with `Backend::ZlibRs`.
#[must_use]
pub fn deflate_best(data: &[u8]) -> Vec<u8> {
    deflate_with(data, Level::BEST)
}

/// Deflates `data` at fast compression (level 1) with `Backend::ZlibRs`.
#[must_use]
pub fn deflate_fast(data: &[u8]) -> Vec<u8> {
    deflate_with(data, Level::new(1).expect("level 1 is valid"))
}

fn deflate_with(data: &[u8], level: Level) -> Vec<u8> {
    let mut out = Vec::new();
    let flate = Encoder::Flate {
        level,
        predictor: Prediction::None,
    };
    pdf_filters::encode(data, &[flate], Backend::ZlibRs, &mut out)
        .expect("Flate encoding with Prediction::None is infallible");
    out
}

/// Inflates raw zlib/deflate `data` up to `limit` bytes using `Backend::ZlibRs`.
pub fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    let filter = Filter::Flate {
        predictor: Predictor::None,
    };
    pdf_filters::decode(data, &[filter], limit, Backend::ZlibRs).map_err(|err| {
        if err
            .faults
            .iter()
            .any(|(_, f)| *f == pdf_filters::Fault::Limit)
        {
            Error::Limit(Limit::Decompressed)
        } else {
            Error::InvalidStream("Flate decode failed".to_string())
        }
    })
}

/// Filter and payload helpers on [`pdf_objects::Stream`].
pub trait StreamExt {
    /// Returns the in-memory raw stream bytes (`&[]` if unplaced).
    fn raw_bytes(&self) -> &[u8];

    /// Compresses an unfiltered stream's in-memory bytes with `/FlateDecode`
    /// (`Backend::ZlibRs`). No-op if `/Filter` is already present.
    fn compress(&mut self);

    /// Replaces the stream's bytes with uncompressed `bytes`, stripping
    /// `/Filter` and `/DecodeParms` and updating `/Length`.
    fn set_plain_content(&mut self, bytes: Vec<u8>);

    /// Decodes the stream through its `/Filter` pipeline up to
    /// `ReadLimits::DEFAULT.max_decompressed` bytes.
    fn decompressed_content(&self) -> Result<Vec<u8>>;

    /// Decodes the stream through its `/Filter` pipeline up to `limit` bytes,
    /// returning `Error::Limit(Limit::Decompressed)` if output exceeds `limit`.
    fn decompressed_content_with_limit(&self, limit: usize) -> Result<Vec<u8>>;

    /// Decodes the stream through its `/Filter` pipeline up to `limit` bytes,
    /// resolving indirect `/Filter` or `/DecodeParms` entries via `resolve`.
    fn decompressed_content_resolved<'o>(
        &'o self,
        limit: usize,
        resolve: Option<&dyn Resolve<'o>>,
    ) -> Result<Vec<u8>>;
}

impl StreamExt for Stream {
    #[inline]
    fn raw_bytes(&self) -> &[u8] {
        self.data.bytes().unwrap_or(&[])
    }

    fn compress(&mut self) {
        if self.dict.has(b"Filter") {
            return;
        }
        let compressed = deflate(self.raw_bytes());
        self.dict
            .set("Filter", pdf_objects::Object::name("FlateDecode"));
        self.set_bytes(compressed);
    }

    fn set_plain_content(&mut self, bytes: Vec<u8>) {
        self.dict.remove(b"Filter");
        self.dict.remove(b"DecodeParms");
        self.set_bytes(bytes);
    }

    fn decompressed_content(&self) -> Result<Vec<u8>> {
        self.decompressed_content_with_limit(ReadLimits::DEFAULT.max_decompressed)
    }

    fn decompressed_content_with_limit(&self, limit: usize) -> Result<Vec<u8>> {
        self.decompressed_content_resolved(limit, None)
    }

    fn decompressed_content_resolved<'o>(
        &'o self,
        limit: usize,
        resolve: Option<&dyn Resolve<'o>>,
    ) -> Result<Vec<u8>> {
        let raw = self.raw_bytes();
        let filters = pdf_structure::read::filters(&self.dict, resolve)
            .map_err(|e| Error::InvalidStream(e.to_string()))?;
        if filters.is_empty() {
            if raw.len() > limit {
                return Err(Error::Limit(Limit::Decompressed));
            }
            return Ok(raw.to_vec());
        }
        match pdf_filters::decode(raw, &filters, limit, Backend::ZlibRs) {
            Ok(data) => Ok(data),
            Err(err) => {
                let limited = err
                    .faults
                    .iter()
                    .any(|(_, f)| *f == pdf_filters::Fault::Limit);
                match (limited, err.data.is_empty()) {
                    (false, false) => Ok(err.data),
                    (true, _) => Err(Error::Limit(Limit::Decompressed)),
                    (false, true) => Err(Error::InvalidStream(
                        "stream filter decode failed".to_string(),
                    )),
                }
            }
        }
    }
}
