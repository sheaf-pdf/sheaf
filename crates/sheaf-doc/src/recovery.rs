//! Crash recovery (M10.5 §11.10). A document's unsaved state is kept in a
//! sidecar next to its file (`<name>.pdf.sheaf-recovery`), written a few
//! seconds after edits stop and whenever a save leaves edits after it.
//!
//! The sidecar is format 2:
//!
//! ```text
//! sheaf-recovery 2\nfull\n<base len>\n<base stamp hex>\n<flags>\n<bytes>
//! ```
//!
//! - **The base** is the file as sheaf last read or wrote it
//!   ([`Document::file`](crate::Document::file)): its length, and its
//!   stamp ([`FileStamp`]) in hex, the tail's hash then the time. A sidecar
//!   is found only while its base is still the file, so it never outlives
//!   it.
//! - **Full** is the whole document: the current version written as a save
//!   writes it ([`Profile`] Keep and Stable), encrypted when the document
//!   is, with save policy's form transforms (`forms::rules::plan`), so a
//!   recovered fill of a hybrid form keeps no stale XFA. Slice 10 adds
//!   Append sections against the saved file.
//! - **Flags:** `removes`, set on every Full sidecar: a restored document
//!   has no record of its file, so its first save is a Rewrite anyway.
//!
//! A restored document opens the bytes from memory
//! ([`MemoryKind::Recovered`](crate::store::MemoryKind::Recovered)), with
//! no record, and its file's base as the base its first save checks
//! ([`Document::set_disk_base`](crate::Document::set_disk_base)).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use in_progress::Progress;

use crate::materialize::{self, Profile, SaveInput, SaveTarget};
use crate::model::SourceInfo;
use crate::serialize::durable::{self, FileStamp};
use crate::store::{BlobKind, Store};

/// The first line of a sidecar in this format.
const FORMAT: &[u8] = b"sheaf-recovery 2\n";
pub const SUFFIX: &str = ".sheaf-recovery";

/// The flag a sidecar whose edits take something out of its base carries.
const REMOVES: &str = "removes";

pub fn sidecar_path(pdf: &Path) -> PathBuf {
    let mut name = pdf
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(SUFFIX);
    pdf.with_file_name(name)
}

/// Write the sidecar of `input`, a document's state ([`recovery_input`]),
/// beside `file`, the document's file, which `base` stamps: the Full form,
/// the version written whole. Returns its size.
///
/// [`recovery_input`]: crate::Document::recovery_input
pub fn write(input: &SaveInput, file: &Path, base: &FileStamp, p: &dyn Progress) -> Result<usize> {
    let stamp = stamp_hex(base).context("the file's time is before 1970")?;
    let target = SaveTarget {
        path: file.into(),
        same_file: true,
        profile: Profile::default(),
        stamp: None,
    };
    // The plan for its transforms: a sidecar asks nothing, and is never
    // the saved file.
    let plan = materialize::plan(input, &target);
    let out = materialize::materialize(input, &plan, target, p)?;
    let head = format!("full\n{}\n{stamp}\n{REMOVES}\n", base.len);
    let mut bytes = Vec::with_capacity(FORMAT.len() + head.len() + out.bytes.len());
    bytes.extend_from_slice(FORMAT);
    bytes.extend_from_slice(head.as_bytes());
    bytes.extend_from_slice(&out.bytes);
    durable::write(&sidecar_path(file), &bytes, None)?;
    Ok(bytes.len())
}

/// Recovery data found next to a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovery {
    pub sidecar: PathBuf,
    /// The file it was written against, which it's found only beside.
    pub base: FileStamp,
    /// Its edits take something out of the base.
    pub removes: bool,
    /// The whole document.
    pub bytes: Vec<u8>,
}

/// A usable sidecar for `pdf`: present, well-formed, and written against
/// the file as it still is (its length and stamp). A stale or malformed
/// one is removed; one whose file can't be read now is left.
pub fn find(pdf: &Path) -> Option<Recovery> {
    let sidecar = sidecar_path(pdf);
    let bytes = std::fs::read(&sidecar).ok()?;
    let found = parse(&bytes).map(|(base, removes, body)| Recovery {
        sidecar: sidecar.clone(),
        base,
        removes,
        bytes: body.to_vec(),
    });
    match found.as_ref().map(|rec| rec.base.holds(pdf)) {
        Some(Ok(true)) => found,
        Some(Err(_)) => None,
        Some(Ok(false)) | None => {
            let _ = std::fs::remove_file(&sidecar);
            None
        }
    }
}

/// The sidecar's base, flag and bytes, when it's this format's Full form
/// and says nothing it doesn't know.
fn parse(bytes: &[u8]) -> Option<(FileStamp, bool, &[u8])> {
    let mut rest = bytes.strip_prefix(FORMAT)?;
    let mut line = || {
        let nl = rest.iter().position(|&b| b == b'\n')?;
        let line = std::str::from_utf8(&rest[..nl]).ok()?;
        rest = &rest[nl + 1..];
        Some(line)
    };
    if line()? != "full" {
        return None;
    }
    let len: u64 = line()?.parse().ok()?;
    let base = parse_stamp(len, line()?)?;
    let mut removes = false;
    for flag in line()?.split_whitespace() {
        match flag {
            REMOVES => removes = true,
            _ => return None,
        }
    }
    (!rest.is_empty()).then_some((base, removes, rest))
}

/// `stamp` as a sidecar says it: its tail's hash, then its time in
/// nanoseconds since 1970, in 32 hex digits each. None for a time before.
fn stamp_hex(stamp: &FileStamp) -> Option<String> {
    let since = stamp.mtime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(format!("{:032x}{:032x}", stamp.tail, since.as_nanos()))
}

/// The stamp of a file `len` long that [`stamp_hex`] wrote as `hex`.
fn parse_stamp(len: u64, hex: &str) -> Option<FileStamp> {
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let (tail, nanos) = hex.split_at(32);
    let nanos = u128::from_str_radix(nanos, 16).ok()?;
    let secs = u64::try_from(nanos / 1_000_000_000).ok()?;
    let mtime =
        SystemTime::UNIX_EPOCH.checked_add(Duration::new(secs, (nanos % 1_000_000_000) as u32))?;
    Some(FileStamp {
        len,
        mtime,
        tail: u128::from_str_radix(tail, 16).ok()?,
    })
}

/// The document `rec` recovers for `pdf`, opened from memory into `store`
/// with `password` under the file's name, while the file is still its
/// base. It has no record: its first save is a Rewrite, and checks the
/// base ([`Document::set_disk_base`](crate::Document::set_disk_base)).
pub fn restore(
    store: &Store,
    pdf: &Path,
    rec: &Recovery,
    password: Option<&str>,
) -> Result<SourceInfo> {
    let reading = || format!("reading {}", pdf.display());
    if !rec.base.holds(pdf).with_context(reading)? {
        bail!(
            "{} changed since the recovery data was written",
            pdf.display()
        );
    }
    let src = (store.open(rec.bytes.as_slice().into(), BlobKind::recovered(password)))
        .with_context(|| format!("opening the recovery data of {}", pdf.display()))?;
    Ok(SourceInfo {
        src,
        name: pdf.to_path_buf(),
        derived: None,
    })
}

/// Remove the sidecar, if any.
pub fn discard(pdf: &Path) {
    let _ = std::fs::remove_file(sidecar_path(pdf));
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use in_progress::NoProgress;
    use lopdf::{Document as Pdf, Object, Stream, dictionary};
    use sheaf_core::Rotation;

    use super::*;
    use crate::model::{Document, Edit};
    use crate::serialize::fixture::holds;
    use crate::store::{MemoryKind, Origin};

    /// An empty directory of the test's own.
    fn scratch() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("sheaf-recovery-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn two_page_pdf(path: &Path) {
        let mut pdf = Pdf::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let mut kids = Vec::new();
        for _ in 0..2 {
            let content = pdf.add_object(Stream::new(dictionary! {}, b"".to_vec()));
            let page = pdf.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id,
                "MediaBox" => Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
                "Contents" => content,
            });
            kids.push(Object::Reference(page));
        }
        pdf.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 2 }),
        );
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        pdf.trailer.set("Root", catalog);
        pdf.save(path).unwrap();
    }

    /// `doc`'s sidecar, written as the app writes it: beside its file,
    /// against the file as sheaf last read or wrote it.
    fn sidecar_of(doc: &Document) -> usize {
        let (file, base) = doc.file().map(|(f, s)| (f.clone(), *s)).unwrap();
        write(&doc.recovery_input(), &file, &base, &NoProgress).unwrap()
    }

    /// The document `rec` restores for `pdf`, opened as the app opens it.
    fn restored(pdf: &Path, rec: &Recovery, password: Option<&str>) -> Document {
        let store = Store::new();
        let info = restore(&store, pdf, rec, password).unwrap();
        let src = info.src.clone();
        let mut doc = crate::open::start(store, info, None, crate::history::default_policy());
        doc.ready(crate::open::finish(&src, &NoProgress)).unwrap();
        doc.mark_unsaved();
        doc.set_disk_base(pdf, rec.base);
        doc
    }

    /// A sidecar holds the whole document, against its file: found while
    /// the file is still that one, restored from memory with no record,
    /// and stale once another program changed the file, when it goes. A
    /// malformed one goes, and so does one of the format before.
    #[test]
    fn sidecar_round_trips_and_detects_staleness() {
        let dir = scratch();
        let pdf = dir.join("doc.pdf");
        two_page_pdf(&pdf);
        let original = std::fs::read(&pdf).unwrap();
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        let p1 = model.page(1).unwrap();
        let turn = Edit::RotatePages {
            ids: vec![p1],
            by: Rotation::R90,
        };
        model.apply(turn, "rot").unwrap();
        let n = sidecar_of(&model);
        assert_eq!(
            std::fs::metadata(sidecar_path(&pdf)).unwrap().len() as usize,
            n
        );
        // The original is untouched.
        assert_eq!(std::fs::read(&pdf).unwrap(), original);

        let rec = find(&pdf).expect("sidecar found");
        assert_eq!(rec.base, FileStamp::read(&pdf).unwrap());
        assert_eq!(rec.base.len, original.len() as u64);
        assert!(rec.removes);
        let recovered = restored(&pdf, &rec, None);
        assert!(recovered.is_dirty() && recovered.saved_record().is_none());
        let primary = recovered.source(recovered.primary()).unwrap();
        assert_eq!(primary.name, pdf);
        let origin = match primary.src.blob().kind() {
            BlobKind::Pdf { origin, .. } => origin.clone(),
            BlobKind::Attachment => unreachable!("a PDF"),
        };
        assert_eq!(origin, Origin::Memory(MemoryKind::Recovered));
        assert_eq!(primary.src.geom(1).rotate, Rotation::R90);
        assert_eq!(primary.src.geom(0).rotate, Rotation::R0);
        assert_eq!(
            recovered.geom(recovered.page(1).unwrap()).rotate,
            Rotation::R90
        );
        // Nothing of it in a temp file of its own.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);

        // The file changed underneath: the sidecar is stale and goes away.
        std::fs::write(&pdf, [original.clone(), b"\n%extra\n".to_vec()].concat()).unwrap();
        assert!(find(&pdf).is_none());
        assert!(!sidecar_path(&pdf).exists());
        // So does one whose file changed after it was found.
        std::fs::write(&pdf, &original).unwrap();
        model = crate::open::file(&pdf, None).unwrap().doc;
        model
            .apply(
                Edit::RotatePages {
                    ids: vec![model.page(0).unwrap()],
                    by: Rotation::R90,
                },
                "rot",
            )
            .unwrap();
        sidecar_of(&model);
        let rec = find(&pdf).unwrap();
        std::fs::write(&pdf, [original.clone(), b"\n".to_vec()].concat()).unwrap();
        let store = Store::new();
        assert!(restore(&store, &pdf, &rec, None).is_err());

        // A malformed sidecar, or one of the format before, goes.
        std::fs::write(&pdf, &original).unwrap();
        let base = FileStamp::read(&pdf).unwrap();
        let v1 = format!("sheaf-recovery 1\n{}\n%%section", original.len());
        let head = |flags: &str| {
            let stamp = stamp_hex(&base).unwrap();
            format!(
                "sheaf-recovery 2\nfull\n{}\n{stamp}\n{flags}\n%PDF-",
                base.len
            )
        };
        for bad in [
            v1,
            head("removes and more"),
            head("removes")[..40].to_string(),
        ] {
            std::fs::write(sidecar_path(&pdf), bad.as_bytes()).unwrap();
            assert!(find(&pdf).is_none(), "{bad}");
            assert!(!sidecar_path(&pdf).exists(), "{bad}");
        }
        std::fs::write(sidecar_path(&pdf), head("removes").as_bytes()).unwrap();
        assert!(find(&pdf).is_some_and(|rec| rec.removes && rec.bytes == b"%PDF-"));

        discard(&pdf);
        discard(&pdf); // no-op without a sidecar
        assert!(!sidecar_path(&pdf).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stamp_reads_back_as_written() {
        let stamp = FileStamp {
            len: 12345,
            mtime: SystemTime::UNIX_EPOCH + Duration::new(1_760_000_000, 123_456_789),
            tail: 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210,
        };
        let hex = stamp_hex(&stamp).unwrap();
        assert_eq!(parse_stamp(12345, &hex), Some(stamp));
        assert_eq!(parse_stamp(12345, &hex[1..]), None);
        let before = FileStamp {
            mtime: SystemTime::UNIX_EPOCH - Duration::from_secs(1),
            ..stamp
        };
        assert_eq!(stamp_hex(&before), None);
    }

    #[test]
    fn sidecar_carries_ocr_layers() {
        use crate::annot::Box2;
        use crate::text_layer::{OcrWord, TextLayer};
        let dir = scratch();
        let pdf = dir.join("doc.pdf");
        two_page_pdf(&pdf);
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        let p1 = model.page(1).unwrap();
        let layer = TextLayer::from_engine(
            vec![OcrWord {
                text: "Hello".into(),
                rect: Box2 {
                    x: 72.0,
                    y: 100.0,
                    w: 100.0,
                    h: 12.0,
                },
                confidence: 1.0,
            }],
            &crate::geom::Geom::read(None, None, 0),
            "test",
        );
        model
            .apply(
                Edit::SetTextLayers {
                    layers: vec![(p1, Arc::new(layer))],
                },
                "ocr",
            )
            .unwrap();
        sidecar_of(&model);
        let rec = find(&pdf).expect("sidecar found");
        let recovered = Pdf::load_mem(&rec.bytes).unwrap();
        let pages = recovered.get_pages();
        let p2 = recovered.get_dictionary(pages[&2]).unwrap();
        let contents = recovered
            .dereference(p2.get(b"Contents").unwrap())
            .unwrap()
            .1
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(contents.len(), 3);
        let text = String::from_utf8_lossy(&recovered.get_page_content(pages[&2])).into_owned();
        assert!(
            text.contains("3 Tr") && text.contains("<00480065006C006C006F>"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A page deleted from a hybrid form changes its form, so a sidecar,
    /// as any save, takes the XFA part out (save policy, `rules::plan`): a
    /// recovered document has no stale XFA.
    #[test]
    fn a_page_delete_on_a_hybrid_form_gives_a_sidecar_with_no_xfa() {
        use crate::forms::fixture::{Form, row};
        let dir = scratch();
        let pdf = dir.join("form.pdf");
        let mut f = Form::new(2);
        let x = f.text("x[0]", 0, row(0));
        let y = f.text("y[0]", 1, row(0));
        f.fill(x, "Ex");
        f.fill(y, "Why");
        f.group("form1[0]", &[x, y]);
        f.xfa();
        std::fs::write(&pdf, f.bytes()).unwrap();
        let mut model = crate::open::file(&pdf, None).unwrap().doc;
        assert!(model.state().form().xfa.present);
        let second = model.page(1).unwrap();
        model
            .apply(Edit::DeletePages { ids: vec![second] }, "delete")
            .unwrap();
        sidecar_of(&model);
        let sidecar = std::fs::read(sidecar_path(&pdf)).unwrap();
        assert!(!holds(&sidecar, b"xfa-template"));
        let rec = find(&pdf).unwrap();
        let src = crate::forms::fixture::source(rec.bytes.clone());
        assert!(!src.form().facts.xfa);
        assert_eq!(src.form().fields.len(), 1);
        let report = crate::forms::check(&rec.bytes, None).unwrap();
        assert!(report.is_clean(), "{:?}", report.findings);
        // The file itself keeps its XFA: a sidecar is never the file.
        assert!(
            crate::forms::fixture::source(std::fs::read(&pdf).unwrap())
                .form()
                .facts
                .xfa
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The sidecar of an encrypted file opened without a password is
    /// encrypted as the file is: neither the page's string nor a mark's
    /// note is in it in the clear, and it restores, encrypted, with both.
    #[test]
    fn an_encrypted_sidecar_holds_no_plaintext() {
        use std::collections::BTreeMap;

        use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
        use lopdf::{EncryptionState, EncryptionVersion, LazyPdf, Permissions, XrefKind};

        use crate::serialize::fixture::{OWNER, encrypted, whole};
        use crate::serialize::tests::{NOTE, mark, notes};
        use crate::source::Limits;
        use crate::source::lazy_tests::{PAGE_STRING, plaintext};

        let dir = scratch();
        let plain = plaintext();
        // Opened with its empty user password, as such files open.
        let state = EncryptionState::try_from(EncryptionVersion::V4 {
            document: &plain,
            encrypt_metadata: true,
            crypt_filters: BTreeMap::from([(
                b"StdCF".to_vec(),
                Arc::new(Aes128CryptFilter) as Arc<dyn CryptFilter>,
            )]),
            stream_filter: b"StdCF".to_vec(),
            string_filter: b"StdCF".to_vec(),
            owner_password: OWNER,
            user_password: "",
            permissions: Permissions::all(),
        })
        .unwrap();
        let [table, _] = encrypted(&plain, &state, |_| {});
        let unencrypted = whole(&plain.objects, &plain.trailer, XrefKind::Table, "1.7", None);
        for (encrypted, bytes) in [(false, unencrypted), (true, table)] {
            let pdf = dir.join(format!("encrypted-{encrypted}.pdf"));
            std::fs::write(&pdf, &bytes).unwrap();
            let mut model = crate::open::file(&pdf, None).unwrap().doc;
            let p0 = model.page(0).unwrap();
            let turn = Edit::RotatePages {
                ids: vec![p0],
                by: Rotation::R90,
            };
            model.apply(turn, "rot").unwrap();
            mark(&mut model);
            sidecar_of(&model);
            // The page's string and the mark's note, in the clear only
            // when the file is.
            let sidecar = std::fs::read(sidecar_path(&pdf)).unwrap();
            for text in [PAGE_STRING, NOTE] {
                assert_eq!(holds(&sidecar, text), !encrypted, "{encrypted}");
            }
            let rec = find(&pdf).expect("sidecar found");
            let bytes: Arc<[u8]> = rec.bytes.as_slice().into();
            let lazy = LazyPdf::open(bytes, None, &Limits::DEFAULT.reader).unwrap();
            assert_eq!(lazy.is_encrypted(), encrypted);
            assert_eq!(notes(&lazy), [NOTE], "{encrypted}");
            let recovered = restored(&pdf, &rec, None);
            let page = recovered.page(0).unwrap();
            assert_eq!(recovered.geom(page).rotate, Rotation::R90);
            assert_eq!(recovered.state().marks_on(page).count(), 1);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
