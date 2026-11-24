//! What a file claims to conform to, from its XMP metadata: PDF/A (ISO
//! 19005) and PDF/UA (ISO 14289), read by pdf-semantics
//! ([`xmp`](crate::pdf::semantics::xmp)) from the packet its catalog's
//! `/Metadata` names, decoded as sheaf decodes every stream and under the
//! source's cap on a stream's size.

use anyhow::{Context, Result, bail};

use crate::content::{Objects, Unkept};
use crate::pdf::LazyPdf;
use crate::pdf::fault::{Place, Sink};
use crate::pdf::semantics::{self, Fault, xmp};

use super::Limits;

/// The claims of the packet `pdf`'s catalog `/Metadata` names: none where
/// it has none, or it names no stream.
///
/// # Errors
///
/// The catalog doesn't read; the way to the packet or its data doesn't, or
/// it decodes past [`ReadLimits::max_decompressed`]; its first bytes say
/// it isn't UTF-8 (UTF-16 or UTF-32, which XMP allows and the reader
/// doesn't read); its XML stops making sense, whatever it claimed before
/// there; or a claim says nothing, a part that's no number or a level
/// that's no letter. An attribute or a value that doesn't read is read
/// past, so a claim elsewhere in the packet counts.
///
/// [`ReadLimits::max_decompressed`]: crate::pdf::ReadLimits::max_decompressed
pub(super) fn read(pdf: &LazyPdf, limits: &Limits) -> Result<xmp::Conformance> {
    let catalog = pdf.catalog()?;
    let unkept = Unkept::default();
    let mut caps = semantics::Limits::DEFAULT;
    caps.xmp_bytes = limits.reader.max_decompressed;
    let mut unread = NotUtf8::default();
    let claims = xmp::claims(Objects::new(pdf, &unkept), catalog, &caps, &mut unread)
        .context("reading the XMP metadata")?;
    if unread.0 {
        bail!("reading the XMP metadata: it isn't UTF-8, the one encoding read");
    }
    let Some(claims) = claims else {
        return Ok(xmp::Conformance::default());
    };
    (claims.conformance()).context("reading the XMP metadata's claims")
}

/// Whether the reader was told the packet isn't UTF-8, so that it read none
/// of it: a packet that can't be read, not one that claims nothing. What
/// else it tells is read past.
#[derive(Default)]
struct NotUtf8(bool);

impl Sink<Fault> for NotUtf8 {
    fn fault(&mut self, _: Place, fault: Fault) {
        self.0 |= fault == Fault::XmpNotUtf8;
    }
}
