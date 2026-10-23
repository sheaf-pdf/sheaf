//! `sheaf-cli split` end to end, through the binary.

use std::path::{Path, PathBuf};
use std::process::Command;

use lopdf::{Document as Pdf, Object, Stream, dictionary};

/// Pages each carrying ~20 KB of incompressible-ish content.
fn heavy_pdf(dir: &Path, n: usize) -> PathBuf {
    let mut pdf = Pdf::with_version("1.5");
    let pages_id = pdf.new_object_id();
    let mut kids: Vec<Object> = Vec::new();
    let mut seed = 1u32;
    for _ in 0..n {
        let noise: String = (0..20_000)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                char::from(b'a' + ((seed >> 16) % 26) as u8)
            })
            .collect();
        let content = pdf.add_object(
            Stream::new(dictionary! {}, format!("% {noise}\n").into_bytes())
                .with_compression(false),
        );
        kids.push(
            pdf.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            })
            .into(),
        );
    }
    pdf.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n as i64 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    let path = dir.join("big.pdf");
    pdf.save(&path).unwrap();
    path
}

fn sheaf(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sheaf-cli"))
        .args(args)
        .output()
        .unwrap()
}

fn pages(p: &str) -> usize {
    Pdf::load(p).unwrap().get_pages().len()
}

#[test]
fn splits_by_count_ranges_and_size() {
    let dir = std::env::temp_dir().join(format!("sheaf-cli-split-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let input = heavy_pdf(&dir, 7);
    let input = input.to_str().unwrap();

    let out = dir.join("every");
    let run = sheaf(&[
        "split",
        "-q",
        input,
        "--every",
        "3",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let files: Vec<String> = String::from_utf8(run.stdout)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(files.len(), 3);
    assert!(files[0].ends_with("big_Part_1.pdf"));
    assert_eq!(
        files.iter().map(|f| pages(f)).collect::<Vec<_>>(),
        vec![3, 3, 1]
    );
    // Again refuses to overwrite.
    let again = sheaf(&["split", input, "--every", "3", "-o", out.to_str().unwrap()]);
    assert_eq!(again.status.code(), Some(1));

    let out = dir.join("ranges");
    let run = sheaf(&[
        "split",
        "-q",
        input,
        "--ranges",
        "1-2; 3, 7",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let counts: Vec<usize> = String::from_utf8(run.stdout)
        .unwrap()
        .lines()
        .map(pages)
        .collect();
    assert_eq!(counts, vec![2, 2]);

    let out = dir.join("size");
    let run = sheaf(&[
        "split",
        "-q",
        input,
        "--max-size",
        "50KB",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let files: Vec<String> = String::from_utf8(run.stdout)
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    // The parts a size split made before it was an export (slice 19).
    let counts: Vec<usize> = files.iter().map(|f| pages(f)).collect();
    assert_eq!(counts, [2, 2, 2, 1], "{files:?}");
    for f in &files {
        assert!(std::fs::metadata(f).unwrap().len() <= 50_000, "{f}");
    }

    // A method is required.
    assert_eq!(sheaf(&["split", input]).status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A signed file split a page a part, its signature on the last page: the
/// last part asks before its signature breaks (exit 3), and nothing is
/// written, the parts before it neither (M10.5 proposal 40). Answered,
/// the split writes every part, with no `--force`.
#[test]
fn a_split_that_asks_writes_no_part() {
    use sheaf_doc::forms::fixture::{Form, row};
    let dir = std::env::temp_dir().join(format!("sheaf-cli-split-signed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut form = Form::new(3);
    let sig = form.signature("sig", 2, row(2));
    form.sign(sig);
    let input = dir.join("signed.pdf");
    std::fs::write(&input, form.bytes()).unwrap();
    let out = dir.join("parts");
    let split = |more: &[&str]| {
        let mut args = vec!["split", "-q", input.to_str().unwrap(), "--every", "1"];
        args.extend(["-o", out.to_str().unwrap()]);
        args.extend(more);
        sheaf(&args)
    };
    let run = split(&[]);
    let said = String::from_utf8_lossy(&run.stderr);
    assert_eq!(run.status.code(), Some(3), "{said}");
    let asked = String::from_utf8(run.stdout).unwrap();
    assert!(asked.contains(r#""kind":"signatures""#), "{asked}");
    let written: Vec<_> = std::fs::read_dir(&out).unwrap().collect();
    assert!(written.is_empty(), "{written:?}");
    let run = split(&["--signatures", "break"]);
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "{said}");
    let counts: Vec<usize> = String::from_utf8(run.stdout)
        .unwrap()
        .lines()
        .map(pages)
        .collect();
    assert_eq!(counts, [1, 1, 1]);
    let _ = std::fs::remove_dir_all(&dir);
}
