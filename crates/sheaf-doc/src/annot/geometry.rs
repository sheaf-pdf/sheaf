//! The geometry the screen paints marks from and the writer draws them
//! by ([`caret_points`], [`attachment_icon`], [`cloud_curves`]), held to
//! what it was when sheaf made it itself (M18 slice 24): pdf-annots' shapes
//! give it now, through those three, and the screen paints from them, so
//! not a bit may move. Each input below is a line of
//! `geometry_golden.txt`: the function, the case, and the xxh3-128 of the
//! line the record has for it (its input's and its output's every `f32` by
//! its bits). The golden was written once, from the record sheaf's own
//! geometry made before the family's took over, and is what it's held to
//! since: a line that differs is a bit of what the screen paints that
//! moved, to find, not a golden to write again.
//! `SHEAF_GEOMETRY_OUT=<file>` writes the whole record, to set beside one
//! made before a change. `SHEAF_GOLDEN_UPDATE=1` writes the golden.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::{
    ATTACHMENT_ICONS, Annot, AnnotKind, Box2, Pt, attachment_icon, caret_box, caret_points,
    cloud_curves, cloud_step, icon_box, outline_of,
};
use sheaf_edit::Rotation;

const TURNS: [Rotation; 4] = [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270];

fn bits(v: f32) -> String {
    format!("{:08x}", v.to_bits())
}

fn point(p: Pt) -> String {
    format!("{},{}", bits(p.x), bits(p.y))
}

fn points(ps: &[Pt]) -> String {
    ps.iter().copied().map(point).collect::<Vec<_>>().join(";")
}

fn boxed(b: Box2) -> String {
    [b.x, b.y, b.w, b.h].map(bits).join(",")
}

/// Each input the three are asked of, and what each gives, a line each:
/// the function, the case, its input's bits and its output's.
fn record() -> String {
    let marks = crate::forms::fixture::every_mark_kind();
    let mut out = String::new();
    let mut caret = |case: &str, at: Box2| {
        let got = caret_points(at);
        let _ = writeln!(
            out,
            "caret_points\t{case}\tat={}\t{}",
            boxed(at),
            points(&got)
        );
    };
    for a in &marks {
        for turn in TURNS {
            let turned = Annot { turn, ..a.clone() };
            if let AnnotKind::Insert { at } | AnnotKind::Replace { at, .. } = turned.upright() {
                caret(&format!("mark{}:turn{}", a.id.0, turn.degrees()), at);
            }
        }
    }
    for h in [0.0, 1.0, 4.0, 6.67, 12.0, 100.0] {
        caret(&format!("caret_box:h{h}"), caret_box(100.3, 200.7, h));
    }
    caret(
        "zero",
        Box2 {
            x: 10.0,
            y: 10.0,
            w: 0.0,
            h: 0.0,
        },
    );
    let mut icon = |case: &str, name: &str, at: Box2| {
        for (i, stroke) in attachment_icon(name, at).iter().enumerate() {
            let _ = writeln!(
                out,
                "attachment_icon\t{case}:{name}:{i}\tat={}\t{}",
                boxed(at),
                points(stroke)
            );
        }
    };
    let names: Vec<&str> = ATTACHMENT_ICONS
        .iter()
        .copied()
        .chain(["Unknown"])
        .collect();
    for a in &marks {
        for turn in TURNS {
            let turned = Annot { turn, ..a.clone() };
            if let AnnotKind::FileAttachment { pos, .. } = turned.upright() {
                for name in &names {
                    icon(
                        &format!("mark{}:turn{}", a.id.0, turn.degrees()),
                        name,
                        icon_box(pos),
                    );
                }
            }
        }
    }
    for (case, at) in [
        (
            "zero",
            Box2 {
                x: 50.0,
                y: 50.0,
                w: 0.0,
                h: 0.0,
            },
        ),
        (
            "small",
            Box2 {
                x: 50.25,
                y: 50.5,
                w: 7.0,
                h: 7.0,
            },
        ),
        (
            "large",
            Box2 {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
        ),
        (
            "wide",
            Box2 {
                x: 10.0,
                y: 20.0,
                w: 30.0,
                h: 10.0,
            },
        ),
    ] {
        for name in &names {
            icon(case, name, at);
        }
    }
    let mut cloud = |case: &str, kind: &AnnotKind, stroke: f32| {
        let Some(outline) = outline_of(kind) else {
            let _ = writeln!(out, "cloud_curves\t{case}\toutline=-");
            return;
        };
        let step = cloud_step(stroke);
        let curves: Vec<String> = (cloud_curves(&outline, step).iter())
            .map(|c| points(c))
            .collect();
        let _ = writeln!(
            out,
            "cloud_curves\t{case}\toutline={}\tstep={}\t{}",
            points(&outline),
            bits(step),
            curves.join("|")
        );
    };
    let square = (marks.iter())
        .find(|a| matches!(a.kind, AnnotKind::Square { .. }))
        .expect("a square")
        .clone();
    let cloudy: Vec<&Annot> = (marks.iter())
        .filter(|a| a.cloudy)
        .chain([&square])
        .collect();
    for a in cloudy {
        for turn in TURNS {
            let turned = Annot { turn, ..a.clone() };
            for stroke in [0.0, 0.5, a.stroke, 5.0, 10.0] {
                let case = format!("mark{}:turn{}:w{stroke}", a.id.0, turn.degrees());
                cloud(&case, &turned.upright(), stroke);
            }
        }
    }
    let p = |x: f32, y: f32| Pt { x, y };
    let edges = [
        (
            "square-under-four-lines",
            AnnotKind::Square {
                rect: Box2 {
                    x: 10.0,
                    y: 10.0,
                    w: 3.0,
                    h: 3.0,
                },
            },
        ),
        (
            "ellipse-flat",
            AnnotKind::Ellipse {
                rect: Box2 {
                    x: 10.0,
                    y: 10.0,
                    w: 40.0,
                    h: 0.0,
                },
            },
        ),
        (
            "polygon-two-points",
            AnnotKind::Polygon {
                points: vec![p(0.0, 0.0), p(10.0, 0.0)],
            },
        ),
        (
            "polygon-clockwise",
            AnnotKind::Polygon {
                points: vec![p(0.0, 0.0), p(0.0, 40.0), p(30.0, 40.0), p(30.0, 0.0)],
            },
        ),
        (
            "polygon-repeated-point",
            AnnotKind::Polygon {
                points: vec![p(0.0, 0.0), p(0.0, 0.0), p(30.0, 0.0), p(15.0, 20.0)],
            },
        ),
    ];
    for (case, kind) in &edges {
        for stroke in [0.0, 1.0, 5.0] {
            cloud(&format!("{case}:w{stroke}"), kind, stroke);
        }
    }
    out
}

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/annot/geometry_golden.txt");

/// Each of the record's lines as the golden holds it: its function, its
/// case, and the digest of the whole line.
fn digests(record: &str) -> Vec<String> {
    (record.lines())
        .map(|line| {
            let mut fields = line.splitn(3, '\t');
            let (function, case) = (fields.next(), fields.next());
            format!(
                "{}\t{}\t{:032x}",
                function.unwrap_or_default(),
                case.unwrap_or_default(),
                sheaf_util::checksum(line.as_bytes())
            )
        })
        .collect()
}

/// What the screen's geometry gives for every input above is what it gave
/// when sheaf made it itself, line for line.
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "recorded on Linux: an ellipse's outline goes through the platform's cosf and \
              sinf, whose last bit may differ elsewhere"
)]
fn the_screen_s_geometry_is_what_was_recorded() {
    let text = record();
    assert_eq!(text, record(), "the same twice");
    if let Some(to) = std::env::var_os("SHEAF_GEOMETRY_OUT") {
        std::fs::write(&to, &text).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
    }
    let got = digests(&text);
    let cases: BTreeSet<(&str, &str)> = (got.iter())
        .map(|l| {
            let mut fields = l.split('\t');
            (
                fields.next().unwrap_or_default(),
                fields.next().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(cases.len(), got.len(), "a case's name is its own");
    if std::env::var_os("SHEAF_GOLDEN_UPDATE").is_some_and(|v| v == "1") {
        let golden: String = got.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(GOLDEN, golden).expect("the golden written");
        return;
    }
    let want = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|e| panic!("{GOLDEN}: {e}; SHEAF_GOLDEN_UPDATE=1 writes it"));
    let want: Vec<&str> = want.lines().collect();
    let mut differs = String::new();
    for (i, (line, recorded)) in got.iter().zip(text.lines()).enumerate() {
        let held = want.get(i).copied().unwrap_or("(no line)");
        if held != line {
            let _ = writeln!(
                differs,
                "line {}:\n  held {held}\n  got  {line}\n  {recorded}",
                i + 1
            );
        }
    }
    for held in want.iter().skip(got.len()) {
        let _ = writeln!(differs, "held, and not made: {held}");
    }
    assert!(
        differs.is_empty(),
        "the screen's geometry differs from {GOLDEN}: a bit it paints moved, to find (the \
         golden isn't written again):\n{differs}"
    );
}
