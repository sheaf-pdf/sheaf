//! The `assets` interface: bytes a plugin hands over for its edits to hold
//! (M10.5 §14.3), interned into the document's store as a paste's assets
//! are, and granted in the call's transaction for its ops to name as
//! `{"asset": n}`. A plugin never names a blob by id.

use sheaf_doc::store::BlobKind;
use sheaf_doc::{Blob, FormRef, Grant};
use sheaf_edit::BlobSel;

use super::api;
use super::api::assets::AssetKind;
use crate::runtime::HostState;

/// Most bytes one call may upload, all its uploads together.
pub const MAX_UPLOAD_BYTES: usize = 64 << 20;

/// What a call uploaded: its blob, held while the call runs, granted in
/// each of the call's transactions in turn (one made after a commit too),
/// so the handle the plugin was given names it in each.
pub(crate) struct Upload {
    pub(crate) blob: Blob,
    pub(crate) grant: Grant,
    /// The bytes the plugin handed over, and for an image the bytes its
    /// pixels took made a stamp: what the host did for it.
    size: usize,
}

impl api::assets::Host for HostState {
    fn upload(&mut self, bytes: Vec<u8>, kind: AssetKind) -> Result<u32, String> {
        let mut size = bytes.len();
        let held: usize = self.uploads.iter().map(|u| u.size).sum();
        if held + size > MAX_UPLOAD_BYTES {
            return Err(format!(
                "a call's uploads hold at most {} MiB",
                MAX_UPLOAD_BYTES >> 20
            ));
        }
        let (tx, base) = self.tx().ok_or("no document is open")?;
        let store = base.store();
        let (blob, grant) = match kind {
            // A page stamp's page, as a library image stamp is made. Its
            // pixels count too, so what the host decodes is bounded:
            // decoding runs on the host, where the call's deadline can't
            // stop it.
            AssetKind::Image => {
                let left = (MAX_UPLOAD_BYTES - held - size) as u64;
                let made = sheaf_doc::stamp_lib::image_stamp_bytes(&bytes, left);
                let (page, pixels) = made.map_err(|e| format!("{e:#}"))?;
                size += pixels as usize;
                let blob =
                    (store.intern(page.into(), BlobKind::stamp())).map_err(|e| e.to_string())?;
                (blob.clone(), stamp(blob))
            }
            AssetKind::Pdf => {
                let blob =
                    (store.intern(bytes.into(), BlobKind::stamp())).map_err(|e| e.to_string())?;
                let pdf = store.source(&blob).map_err(|e| format!("not a PDF: {e}"))?;
                if pdf.page_count() == 0 {
                    return Err("the PDF has no pages".into());
                }
                (blob.clone(), stamp(blob))
            }
            AssetKind::File => {
                let blob = (store.intern(bytes.into(), BlobKind::Attachment))
                    .map_err(|e| e.to_string())?;
                (blob.clone(), Grant::Blob(blob.into()))
            }
        };
        tx.pin(blob.clone());
        let BlobSel::Asset(n) = tx.grant(grant.clone()) else {
            unreachable!("a grant is named as an asset")
        };
        self.uploads.push(Upload { blob, grant, size });
        Ok(n)
    }
}

/// A page stamp's page: `blob`'s first page, named by its stem.
fn stamp(blob: Blob) -> Grant {
    Grant::Stamp {
        stem: blob.id().stem(),
        form: FormRef::Blob(blob.into()),
    }
}
