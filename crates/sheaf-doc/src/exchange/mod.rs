//! Comments in and out of a document as data: FDF, XFDF (`fdf`, `xfdf`),
//! or another PDF's (Acrobat's Import/Export Data File, M9 slice 18).
//!
//! Whatever the format, comments travel as PDF annotation dictionaries
//! ([`Comments`]): XFDF's elements are read into the same keys. Coming in,
//! each becomes a mark through [`crate::import`], the converter that also
//! makes another tool's comment editable, and is matched to what's there
//! by `/NM` ([`import`]).

pub mod fdf;
pub mod xfdf;

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result};
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId};

use crate::annot::AnnotId;
use crate::attachments::Objects;
use crate::file::thread_name;
use crate::import::{Interned, NotAMark, Options};
use crate::model::{Edit, FormRef, PageId, Snapshot};
use crate::store::{Blob, BlobId, BlobKind, Src, Store};

/// Comments as dictionaries, each with its page (from 0). `pdf` holds
/// what they refer to: appearances, files, the comments their `/IRT`
/// names.
pub struct Comments {
    pub pdf: Pdf,
    pub annots: Vec<(u32, ObjectId)>,
    /// The PDF they're comments on, as the data file names it.
    pub file: Option<String>,
}

impl Comments {
    pub fn new() -> Comments {
        Comments {
            pdf: Pdf::with_version("1.2"),
            annots: Vec::new(),
            file: None,
        }
    }

    /// The `i`th comment's dictionary.
    pub fn dict(&self, i: usize) -> Option<&Dictionary> {
        self.pdf.get_dictionary(self.annots.get(i)?.1).ok()
    }

    pub fn len(&self) -> usize {
        self.annots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.annots.is_empty()
    }

    /// The name of what `dict` replies to, or is part of.
    pub fn parent_of(&self, dict: &Dictionary) -> Option<String> {
        crate::import::irt_name(&self.pdf, dict)
    }
}

impl Default for Comments {
    fn default() -> Self {
        Comments::new()
    }
}

/// A data file or PDF, by what it starts with.
pub fn read(bytes: &[u8], password: Option<&str>) -> Result<Comments> {
    let head: Vec<u8> = bytes
        .iter()
        .copied()
        .skip_while(|b| b.is_ascii_whitespace() || *b == 0xEF || *b == 0xBB || *b == 0xBF)
        .take(5)
        .collect();
    match head.as_slice() {
        b"%FDF-" => fdf::read(bytes),
        b"%PDF-" => from_pdf(bytes, password),
        [b'<', ..] => xfdf::read(bytes),
        _ => anyhow::bail!("not an FDF, XFDF or PDF file"),
    }
}

/// Another PDF's comments: every annotation but links, form fields and
/// popups, sheaf's own included (they come back exactly, from
/// `/SheafAnnot`).
pub fn from_pdf(bytes: &[u8], password: Option<&str>) -> Result<Comments> {
    let mut pdf = Pdf::load_mem(bytes).context("reading the PDF")?;
    if pdf.is_encrypted() {
        let options = lopdf::LoadOptions::with_password(password.unwrap_or(""));
        pdf = Pdf::load_mem_with_options(bytes, options)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("decrypting the PDF")?;
    }
    let mut annots = Vec::new();
    for (i, page) in pdf.get_pages().into_values().enumerate() {
        let list = pdf
            .get_dictionary(page)
            .ok()
            .and_then(|d| d.get(b"Annots").ok())
            .and_then(|a| pdf.dereference(a).ok())
            .and_then(|(_, a)| a.as_array().ok())
            .cloned()
            .unwrap_or_default();
        for entry in list {
            let Ok(id) = entry.as_reference() else {
                continue;
            };
            let Ok(d) = pdf.get_dictionary(id) else {
                continue;
            };
            if matches!(
                crate::import::subtype(d).as_str(),
                "Link" | "Widget" | "Popup" | ""
            ) {
                continue;
            }
            annots.push((i as u32, id));
        }
    }
    Ok(Comments {
        pdf,
        annots,
        file: None,
    })
}

/// What [`import`] did, for the notice.
#[derive(Debug, Default)]
pub struct Imported {
    pub edits: Vec<Edit>,
    /// What the edits' marks hold that's interned for them: kept until
    /// they're applied.
    pub held: Vec<Blob>,
    /// New comments.
    pub added: usize,
    /// Comments already here (by `/NM`) brought up to date.
    pub updated: usize,
    /// Comments sheaf can't hold, by subtype.
    pub skipped: BTreeMap<String, usize>,
    /// Comments on pages this document doesn't have.
    pub off_the_end: usize,
}

impl Imported {
    /// "12 added, 3 updated; 2 Sound comments skipped (sheaf can't hold
    /// them)".
    pub fn summary(&self) -> String {
        let mut out = format!("{} added, {} updated", self.added, self.updated);
        if !self.skipped.is_empty() {
            let kinds: Vec<String> = self
                .skipped
                .iter()
                .map(|(k, n)| format!("{n} {k}"))
                .collect();
            out.push_str(&format!(
                "; skipped {} (sheaf can't hold them)",
                kinds.join(", ")
            ));
        }
        if self.off_the_end > 0 {
            out.push_str(&format!(
                "; {} on pages this document doesn't have",
                self.off_the_end
            ));
        }
        out
    }
}

/// The edits that bring `comments` into the document `snap`, whose blobs
/// are in `store`: one comment whose `/NM` is one of our marks' updates
/// it; one whose `/NM` is another tool's comment here takes its place, as
/// making it editable does; the rest are added. Pages are counted in the
/// document's order, and a comment reads upright as its page shows. What
/// our marks hold, a page stamp's page and an attachment's file, is the
/// document's where it has it, else interned from the comments, and held
/// ([`Imported::held`]); an attachment of another tool's is interned too.
pub fn import(snap: &Snapshot, store: &Store, comments: &Comments) -> Imported {
    let st = &snap.state;
    let mut out = Imported::default();
    let interned = Interned::new(store);
    let ours: HashMap<String, (PageId, AnnotId)> = st
        .marks()
        .map(|(page, a)| (a.name(), (page, a.id)))
        .collect();
    let theirs: std::collections::HashSet<&str> = (st.annots())
        .filter_map(|(_, a)| Some(a.foreign()?.summary.name.as_str()))
        .collect();
    let mut seen = std::collections::HashSet::new();
    // By page, so a replacement's parts meet.
    let mut by_page: BTreeMap<u32, Vec<ObjectId>> = BTreeMap::new();
    for &(page, id) in &comments.annots {
        if comments.pdf.get_dictionary(id).is_ok() {
            by_page.entry(page).or_default().push(id);
        }
    }
    for (page, ids) in by_page {
        let entries: Vec<Object> = ids.iter().map(|&id| Object::Reference(id)).collect();
        let dicts: Vec<&Dictionary> = ids
            .iter()
            .filter_map(|&id| comments.pdf.get_dictionary(id).ok())
            .collect();
        let Some((id, rotation)) = st
            .page_at(page as usize)
            .and_then(|id| Some((id, st.geom(id)?.rotate)))
        else {
            out.off_the_end += dicts
                .iter()
                .filter(|d| crate::import::subtype(d) != "Popup")
                .count();
            continue;
        };
        let files = Options {
            files: Some(&interned),
        };
        let converted = crate::import::convert_page(&comments.pdf, &dicts, rotation, files);
        let page_annots = crate::file::PageAnnots::new(&entries);
        for ((dict, entry), mark) in dicts.iter().zip(&entries).zip(converted) {
            // One of sheaf's comes back exactly as it was, under the name
            // it had there, so what replies to it there does here; one
            // another tool has edited since is that tool's.
            let own = crate::file::sheaf_mark(&comments.pdf, entry, rotation, &page_annots);
            let mark = match own {
                Some(mut a) => {
                    bring_held(&comments.pdf, dict, &mut a, snap, &interned);
                    if a.name.is_empty() {
                        a.name = a.name();
                    }
                    if let crate::annot::AnnotKind::Connector { partner, .. } = &mut a.kind {
                        *partner = None;
                    }
                    Ok(a)
                }
                None => mark,
            };
            let mut a = match mark {
                Ok(a) => a,
                Err(NotAMark::Unsupported(kind)) => {
                    *out.skipped.entry(kind).or_default() += 1;
                    continue;
                }
                Err(_) => continue,
            };
            if a.name.is_empty() {
                a.name = thread_name(dict);
            }
            if !seen.insert(a.name.clone()) {
                continue;
            }
            match ours.get(&a.name) {
                Some(&(at, id)) => {
                    a.id = id;
                    out.edits.push(Edit::AnnotUpdate { page: at, annot: a });
                    out.updated += 1;
                }
                None => {
                    if theirs.contains(a.name.as_str()) {
                        out.updated += 1;
                    } else {
                        out.added += 1;
                    }
                    out.edits.push(Edit::AnnotAdd { page: id, annot: a });
                }
            }
        }
    }
    out.held = interned.into_held();
    out
}

/// What `a`, one of our marks, holds in `pdf`, its dictionary `dict`, for
/// an edit of `snap`'s document: what the document has under the stem `a`
/// names it by ([`find_stamp`], [`find_file`]), else its stamp's page as a
/// one-page PDF or its file's bytes, interned, `a` then naming it by the
/// blob's stem. What's interned, or found in the store, is held in
/// `interned` for the edit.
///
/// [`find_stamp`]: crate::model::find_stamp
/// [`find_file`]: crate::model::find_file
fn bring_held(
    pdf: &Pdf,
    dict: &Dictionary,
    a: &mut crate::Annot,
    snap: &Snapshot,
    interned: &Interned,
) {
    use crate::annot::AnnotKind;
    let (store, sources, primary) = (interned.store(), &snap.sources, snap.primary);
    match &mut a.kind {
        AnnotKind::PageStamp { stamp, name, .. } => {
            match crate::model::find_stamp(store, sources, primary, stamp) {
                Some(FormRef::Blob(page)) => interned.hold(page.blob().clone()),
                Some(FormRef::Base(_)) => {}
                None => {
                    let page = crate::file::stamp_page(pdf, dict)
                        .map(|form| crate::stamp_lib::form_to_pdf(pdf, form));
                    match page {
                        Some(Ok(page)) => {
                            if let Some(blob) = interned.intern(page.into(), BlobKind::stamp()) {
                                *stamp = blob.id().stem();
                            }
                        }
                        Some(Err(e)) => log::warn!("stamp {name}: {e:#}"),
                        None => {}
                    }
                }
            }
        }
        AnnotKind::FileAttachment {
            file, name, size, ..
        } => match crate::model::find_file(store, sources, primary, file, *size) {
            Some(found) => interned.hold(found.blob().clone()),
            None => {
                let bytes = crate::file::attached_file(pdf, dict)
                    .and_then(|id| pdf.get_object(id).ok()?.as_stream().ok())
                    .map(|s| crate::attachments::mark_file_bytes(s, *size));
                match bytes {
                    Some(Ok(bytes)) => {
                        if let Some(blob) = interned.intern(bytes.into(), BlobKind::Attachment) {
                            *file = blob.id().stem();
                        }
                    }
                    Some(Err(e)) => log::warn!("attachment {name}: {e:#}"),
                    None => {}
                }
            }
        },
        _ => {}
    }
}

/// Comments to write out of `snap`: our marks `ours` and other tools'
/// comments `theirs`, by id, as dictionaries in a [`Comments`], replies
/// pointing at what they reply to when it's written too. Pages are
/// counted in the document's order.
pub fn gather(snap: &Snapshot, ours: &[AnnotId], theirs: &[AnnotId]) -> Result<Comments> {
    let (st, sources) = (&snap.state, &snap.sources);
    let mut out = Comments::new();
    let mut cx = crate::annot::WriteCx::new();
    // Each written comment's name and parent's name, to link them after.
    let mut names: HashMap<String, ObjectId> = HashMap::new();
    let mut parents: Vec<(ObjectId, String)> = Vec::new();
    for &id in ours {
        let Some((page, m)) = st.annot(id).and_then(|a| Some((a.page, a.mark_record()?))) else {
            continue;
        };
        let Some(index) = st.pos_of(page) else {
            continue;
        };
        let a = &m.annot;
        let written = crate::annot::to_pdf_in(m, &mut out.pdf, &mut cx);
        let mut main = written.main;
        main.set("Page", index as i64);
        let main_id = out.pdf.add_object(Object::Dictionary(main));
        out.annots.push((index as u32, main_id));
        names.insert(a.name(), main_id);
        if let Some(p) = a.thread_parent() {
            parents.push((main_id, p.to_string()));
        }
        for mut part in written.companions {
            part.set("IRT", main_id);
            part.set("RT", Object::Name(b"Group".to_vec()));
            part.set("Page", index as i64);
            let part_id = out.pdf.add_object(Object::Dictionary(part));
            out.annots.push((index as u32, part_id));
        }
    }
    let mut maps: HashMap<BlobId, HashMap<ObjectId, ObjectId>> = HashMap::new();
    for &id in theirs {
        let Some((page, f)) = st.annot(id).and_then(|a| Some((a.page, a.foreign()?))) else {
            continue;
        };
        let (Some(index), Some(def)) = (st.pos_of(page), st.def(page)) else {
            continue;
        };
        // Its object, or the dictionary at its place in its page's.
        let (src, dict) = match &f.src {
            Some(at) => (&at.src, at.src.pdf().dictionary(at.id)),
            None => (
                &def.base.src,
                annot_in(&def.base.src, def.base.index, f.summary.slot),
            ),
        };
        let Some(dict) = dict else {
            continue;
        };
        let f = &f.summary;
        let mut dict = dict.clone();
        // Not what points back into the file it came from.
        for key in [
            b"P".as_slice(),
            b"Popup",
            b"Parent",
            b"IRT",
            b"StructParent",
        ] {
            dict.remove(key);
        }
        let map = maps.entry(src.id()).or_default();
        let Ok(Object::Dictionary(mut copied)) =
            crate::save::remap(src.pdf(), &Object::Dictionary(dict), &mut out.pdf, map)
        else {
            continue;
        };
        copied.set("Page", index as i64);
        if !copied.has(b"NM") {
            copied.set("NM", crate::save::text_string(&f.name));
        }
        let id = out.pdf.add_object(Object::Dictionary(copied));
        out.annots.push((index as u32, id));
        names.insert(f.name.clone(), id);
        if let Some(p) = &f.parent {
            parents.push((id, p.clone()));
        }
    }
    for (child, parent) in parents {
        let to = match names.get(&parent) {
            Some(&p) => Object::Reference(p),
            // Not written: by name, as XFDF says it.
            None => crate::save::text_string(&parent),
        };
        if let Ok(d) = out.pdf.get_dictionary_mut(child) {
            d.set("IRT", to);
        }
    }
    out.file = st
        .pages()
        .next()
        .and_then(|(_, def, _)| sources.get(&def.base.source))
        .and_then(|s| s.name.file_name())
        .map(|n| n.to_string_lossy().into_owned());
    Ok(out)
}

/// Page `page`'s annotation in `/Annots` slot `slot`.
fn annot_in(src: &Src, page: u32, slot: usize) -> Option<&Dictionary> {
    let entry = src.page(page)?.annots.get(slot)?;
    src.pdf().dereference(entry).ok()?.1.as_dict().ok()
}

#[cfg(test)]
mod tests;
