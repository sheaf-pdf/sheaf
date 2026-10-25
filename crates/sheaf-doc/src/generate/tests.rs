//! A watermark and a header laid out on a page, and `/SHEF_Generators`' JSON.

use std::collections::BTreeSet;

use lopdf::dictionary;
use sheaf_edit::{FontChoice, GridPos, HeaderFooterSpec, Rotation, Z};

use super::*;
use crate::content::{GenId, Need, ResBody, ResKind};
use crate::model::{Frozen, ImageRef};
use crate::source::ObjRef;

/// A watermark of `text`, `angle` degrees, at `position`.
fn text(text: &str, angle: f32, position: GridPos) -> WatermarkGen {
    WatermarkGen {
        content: WatermarkContent::Text {
            text: text.into(),
            font: FontChoice::HelveticaBold,
            size: 72.0,
            color: [200, 30, 30],
            opacity: 0.3,
            angle,
        },
        position,
    }
}

/// Watermark `wm` as generator `id`'s, over every page.
fn over(id: u64, wm: &WatermarkGen) -> Generator {
    Generator {
        id: GenId(id),
        spec: GenSpec::Watermark(Arc::new(wm.clone())),
        z: Z::Over,
        scope: Scope::All,
        frozen: Frozen::default(),
        replaces: BTreeSet::from([PaginationKind::Watermark]),
    }
}

/// What a layout reads of a page of `geom`: the first of one.
fn ctx_of(geom: &Geom) -> PageCtx {
    PageCtx {
        geom: *geom,
        position: 0,
        count: 1,
        label: "1".into(),
        bates: None,
    }
}

/// A page whose media box is off the origin, cropped, turned `r`.
fn page(r: Rotation) -> Geom {
    let mut geom = Geom::read(Some(Rect::new(-20.0, 10.0, 592.0, 802.0)), None, 0);
    geom.crop = Rect::new(30.0, 60.0, 542.0, 702.0);
    geom.rotate = r;
    geom
}

/// Where the watermark's frame `m` puts `p` of its own, on the page of
/// `geom` as it shows.
fn shown(geom: &Geom, m: Affine, p: Pt) -> Pt {
    geom.to_view().apply(m.apply(p))
}

/// The frame a page's block draws in: its `cm` after the `q` and `gs`.
fn drawn_in(ops: &[u8]) -> Affine {
    let text = String::from_utf8_lossy(ops);
    let line = (text.lines()).find(|l| l.ends_with(" cm")).expect("a cm");
    let n: Vec<f32> = (line.split(' ').take(6))
        .map(|v| v.parse().unwrap())
        .collect();
    Affine {
        a: n[0],
        b: n[1],
        c: n[2],
        d: n[3],
        e: n[4],
        f: n[5],
    }
}

/// On a page turned any way, a watermark at 0° reads left to right on the
/// page as it shows, its middle in the middle of the visible box; at 90°,
/// up it; and one in a corner keeps 36 pt from both sides it's at.
#[test]
fn a_watermark_is_laid_out_upright_on_the_page_as_it_shows() {
    for r in [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270] {
        let geom = page(r);
        let size = geom.shown();
        let wm = text("DRAFT", 0.0, GridPos::Center);
        let drawn = layout(
            &over(1, &wm),
            &ctx_of(&geom),
            None,
            &Reserved::default(),
            Private::Keep,
        );
        let m = drawn_in(&drawn.ops);
        let w = Std14::HelveticaBold.width("DRAFT", 72.0);
        let h = 72.0 * CAP_HEIGHT;
        let o = shown(&geom, m, Pt { x: 0.0, y: 0.0 });
        let along = shown(&geom, m, Pt { x: 1.0, y: 0.0 });
        let up = shown(&geom, m, Pt { x: 0.0, y: 1.0 });
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(near(along.x - o.x, 1.0) && near(along.y, o.y), "{r:?}");
        assert!(near(up.x, o.x) && near(up.y - o.y, -1.0), "{r:?}");
        let middle = shown(
            &geom,
            m,
            Pt {
                x: w / 2.0,
                y: h / 2.0,
            },
        );
        assert!(near(middle.x, size.width / 2.0), "{r:?}: {middle:?}");
        assert!(near(middle.y, size.height / 2.0), "{r:?}: {middle:?}");

        let wm = text("DRAFT", 90.0, GridPos::BottomRight);
        let drawn = layout(
            &over(1, &wm),
            &ctx_of(&geom),
            None,
            &Reserved::default(),
            Private::Keep,
        );
        let m = drawn_in(&drawn.ops);
        let o = shown(&geom, m, Pt { x: 0.0, y: 0.0 });
        let along = shown(&geom, m, Pt { x: 1.0, y: 0.0 });
        assert!(near(along.x, o.x) && near(along.y - o.y, -1.0), "{r:?}");
        let corners =
            [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].map(|(x, y)| shown(&geom, m, Pt { x, y }));
        let right = corners.iter().map(|p| p.x).fold(f32::MIN, f32::max);
        let bottom = corners.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!(near(right, size.width - EDGE), "{r:?}: {right}");
        assert!(near(bottom, size.height - EDGE), "{r:?}: {bottom}");
    }
}

/// A block is marked content tagged `/Artifact`, its property list named
/// in the page's `/Properties` and saying its kind and generator, drawn in
/// its own graphics state at its opacity, its text in its face's WinAnsi:
/// a character the face hasn't got is `?`.
#[test]
fn a_block_is_a_pagination_artifact_in_its_own_state() {
    let geom = page(Rotation::R0);
    let wm = text("Café 漢字", 0.0, GridPos::Center);
    let drawn = layout(
        &over(0xab, &wm),
        &ctx_of(&geom),
        None,
        &Reserved::default(),
        Private::Keep,
    );
    let ops = &drawn.ops;
    let kinds: Vec<ResKind> = drawn.res.0.iter().map(|n| n.kind).collect();
    assert_eq!(
        kinds,
        [ResKind::Properties, ResKind::ExtGState, ResKind::Font]
    );
    let props = &drawn.res.0[0];
    let mut start = b"/Artifact /".to_vec();
    start.extend(&props.name);
    start.extend(b" BDC\nq\n");
    assert!(ops.starts_with(&start), "{}", String::from_utf8_lossy(ops));
    assert!(ops.ends_with(b"Q\nEMC\n"));
    let Object::Dictionary(list) = object(&drawn.res.0[0]) else {
        panic!("a property list");
    };
    let want = dictionary! {
        "Type" => "Pagination",
        "Subtype" => "Watermark",
        "SHEF_Gen" => Object::string_literal("gab"),
    };
    assert_eq!(list, &want);
    let Object::Dictionary(gs) = object(&drawn.res.0[1]) else {
        panic!("a graphics state");
    };
    assert_eq!(gs.get(b"ca").unwrap().as_float().unwrap(), 0.3);
    assert_eq!(gs.get(b"CA").unwrap().as_float().unwrap(), 0.3);
    let drawn_text = b"(Caf\xe9 ??) Tj";
    assert!(ops.windows(drawn_text.len()).any(|w| w == drawn_text));
    // Without sheaf's private data, its list says its kind alone, and it
    // draws the same.
    let stripped = layout(
        &over(0xab, &wm),
        &ctx_of(&geom),
        None,
        &Reserved::default(),
        Private::Strip,
    );
    let Object::Dictionary(list) = object(&stripped.res.0[0]) else {
        panic!("a property list");
    };
    assert_eq!(
        list,
        &dictionary! { "Type" => "Pagination", "Subtype" => "Watermark" }
    );
    let after = |ops: &[u8]| ops[ops.windows(3).position(|w| w == b"BDC").unwrap()..].to_vec();
    assert_eq!(after(&stripped.ops), after(ops));
}

/// The object a resource is.
fn object(need: &Need) -> &Object {
    match &need.res.body {
        ResBody::Objects(objects) => &objects[0],
        ResBody::Written(_) => panic!("a resource made here"),
    }
}

/// The layout's digest is what's drawn, whatever the names: kept clear of
/// a page that binds one of them otherwise, the content names another, and
/// the digest is the same; a different opacity or place is another.
#[test]
fn the_layout_digest_covers_what_is_drawn_never_names() {
    let geom = page(Rotation::R0);
    let wm = text("DRAFT", 45.0, GridPos::Center);
    let free = layout(
        &over(1, &wm),
        &ctx_of(&geom),
        None,
        &Reserved::default(),
        Private::Keep,
    );
    let font = (free.res.0.iter())
        .find(|n| n.kind == ResKind::Font)
        .unwrap();
    let mut taken = Reserved::default();
    let other = ResourceNeeds(vec![Need {
        kind: ResKind::Font,
        name: font.name.clone(),
        res: Res::dict(dictionary! { "Other" => 1 }),
    }]);
    taken.take(&other);
    let clear = layout(&over(1, &wm), &ctx_of(&geom), None, &taken, Private::Keep);
    assert_ne!(clear.ops, free.ops);
    let renamed = (clear.res.0.iter())
        .find(|n| n.kind == ResKind::Font)
        .unwrap();
    assert_ne!(renamed.name, font.name);
    assert_eq!(clear.layout_digest, free.layout_digest);
    let faint = WatermarkGen {
        content: match &wm.content {
            WatermarkContent::Text { text, .. } => WatermarkContent::Text {
                text: text.clone(),
                font: FontChoice::HelveticaBold,
                size: 72.0,
                color: [200, 30, 30],
                opacity: 0.2,
                angle: 45.0,
            },
            _ => unreachable!(),
        },
        ..wm.clone()
    };
    let other = layout(
        &over(1, &faint),
        &ctx_of(&geom),
        None,
        &Reserved::default(),
        Private::Keep,
    );
    assert_ne!(other.layout_digest, free.layout_digest);
    let moved = WatermarkGen {
        position: GridPos::Top,
        ..wm.clone()
    };
    let other = layout(
        &over(1, &moved),
        &ctx_of(&geom),
        None,
        &Reserved::default(),
        Private::Keep,
    );
    assert_ne!(other.layout_digest, free.layout_digest);
}

/// An image or a page that can't be had draws nothing; an image `scale`
/// of the page's width wide keeps its proportions, turned as it shows.
#[test]
fn an_image_keeps_its_proportions_and_its_turn() {
    let geom = page(Rotation::R0);
    let size = geom.shown();
    let d = crate::tx::tests::doc_of(1, &[]);
    let at = ObjRef {
        src: d.primary().clone(),
        id: (1, 0),
    };
    let image = WatermarkGen {
        content: WatermarkContent::Image {
            image: ImageRef::Placed(Placed {
                at,
                res: ResId(1),
                turns: 0,
            }),
            scale: 0.5,
            opacity: 1.0,
        },
        ..text("x", 0.0, GridPos::Center)
    };
    let none = layout(
        &over(1, &image),
        &ctx_of(&geom),
        None,
        &Reserved::default(),
        Private::Keep,
    );
    assert!(none.ops.is_empty() && none.res.0.is_empty());
    let res = Res::dict(dictionary! { "Subtype" => "Image" });
    for (turns, across, up) in [(0, 40.0, 20.0), (1, 20.0, 40.0)] {
        let art = Art::Image {
            res: res.clone(),
            width: 40,
            height: 20,
            turns,
        };
        let drawn = layout(
            &over(1, &image),
            &ctx_of(&geom),
            Some(&art),
            &Reserved::default(),
            Private::Keep,
        );
        let text = String::from_utf8_lossy(&drawn.ops).into_owned();
        let w = size.width * 0.5;
        let h = w * up / across;
        let cm: Vec<&str> = text.lines().filter(|l| l.ends_with(" cm")).collect();
        let image_cm = turned(turns, w, h);
        let want = format!(
            "{} {} {} {} {} {} cm",
            num(image_cm.a),
            num(image_cm.b),
            num(image_cm.c),
            num(image_cm.d),
            num(image_cm.e),
            num(image_cm.f)
        );
        assert_eq!(cm[1], want, "{turns}");
        assert!(text.contains(" Do\n"), "{text}");
    }
    // The image's top-left corner as stored lands where its turn shows it:
    // at the box's top-left upright, top-right turned a quarter, and so on.
    let (w, h) = (4.0, 2.0);
    let top_left = Pt { x: 0.0, y: 1.0 };
    let corners = [(0, (0.0, h)), (1, (w, h)), (2, (w, 0.0)), (3, (0.0, 0.0))];
    for (turns, (x, y)) in corners {
        let p = turned(turns, w, h).apply(top_left);
        assert_eq!((p.x, p.y), (x, y), "{turns}");
    }
}

/// `x` as the writer writes an operand.
fn num(x: f32) -> String {
    let s = format!("{x:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// `/SHEF_Generators` holds each watermark, in ASCII, with its id as its
/// blocks name it, its kind, z, scope and what it replaces; an image's or
/// a page's file is "placed", never a path or an object number. Nothing
/// when there are none.
#[test]
fn the_generators_json_holds_the_specs_without_paths() {
    let mut d = crate::tx::tests::doc_of(2, &[]);
    let unplaced = |_: &WatermarkContent| (ResId(0), 0);
    assert_eq!(catalog_json(d.state(), unplaced), None);
    let first = d.page(0).unwrap();
    let set = |text: &str, z, pages: Option<&[crate::model::PageId]>| {
        crate::tx::fixture::set_watermark(crate::tx::fixture::watermark(text), z, pages)
    };
    let ops = vec![
        set("Café 漢", Z::Over, None),
        set("under", Z::Under, Some(&[first])),
    ];
    d.commit_ops(ops, "watermark").unwrap();
    let json = catalog_json(d.state(), unplaced).unwrap();
    assert!(json.is_ascii(), "{}", String::from_utf8_lossy(&json));
    let stored: Stored = serde_json::from_slice(&json).unwrap();
    assert_eq!(stored.v, 1);
    let over = d.state().watermark(Z::Over).unwrap();
    let under = d.state().watermark(Z::Under).unwrap();
    let mut want = vec![
        StoredGen::of(over, &mut unplaced.clone()),
        StoredGen::of(under, &mut unplaced.clone()),
    ];
    want.sort_by(|a, b| a.id.cmp(&b.id));
    let mut got = stored.generators;
    got.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(got, want);
    let text = String::from_utf8(json).unwrap();
    assert!(text.contains(r#""text":"Caf\u00e9 \u6f22""#), "{text}");
    assert!(text.contains(&format!(r#""id":"{}""#, over.id)), "{text}");
    assert!(text.contains(r#""scope":"all""#) && text.contains(r#""scope":"pages""#));
    assert!(text.contains(r#""replaces":["background"]"#), "{text}");
}

/// Header and footer `spec` as generator `id`'s, on every page, its date
/// and file fixed.
fn header_gen(id: u64, spec: HeaderFooterSpec) -> Generator {
    Generator {
        id: GenId(id),
        spec: GenSpec::HeaderFooter(Arc::new(spec)),
        z: Z::Over,
        scope: Scope::All,
        frozen: Frozen {
            date: "2026-10-17".into(),
            file: "report.pdf".into(),
            bates: None,
        },
        replaces: BTreeSet::from([PaginationKind::Header, PaginationKind::Footer]),
    }
}

/// A header's text goes out in its face's WinAnsi, one byte a character,
/// escaped; each slot is an artifact of its own, its list named, never
/// inline, and saying it's the header's or the footer's and whose; the
/// face is the spec's, one font for the whole header.
#[test]
fn a_header_goes_out_in_win_ansi_with_a_named_list() {
    let mut spec = crate::tx::fixture::header("Café “x” – € (a\\b)");
    spec.texts[5] = "<<file>>".into();
    spec.font = FontChoice::TimesRoman;
    let g = header_gen(0x2a, spec);
    let drawn = layout(
        &g,
        &ctx_of(&page(Rotation::R0)),
        None,
        &Reserved::default(),
        Private::Keep,
    );
    let out = &drawn.ops;
    let text = b"(Caf\xe9 \x93x\x94 \x96 \x80 \\(a\\\\b\\)) Tj";
    assert!(out.windows(text.len()).any(|w| w == text), "{out:?}");
    assert!(out.starts_with(b"/Artifact /Sh"), "{out:?}");
    assert!(!out.windows(2).any(|w| w == b"<<"), "no list inline");
    let kinds: Vec<ResKind> = drawn.res.0.iter().map(|n| n.kind).collect();
    assert_eq!(
        kinds,
        [ResKind::Properties, ResKind::Font, ResKind::Properties]
    );
    let font = &drawn.res.0[1];
    assert_eq!(font.res, Std14::TimesRoman.resource().into());
    for (need, kind) in [(&drawn.res.0[0], "Header"), (&drawn.res.0[2], "Footer")] {
        let want = dictionary! {
            "Type" => "Pagination",
            "Subtype" => kind,
            "SHEF_Gen" => Object::string_literal("g2a"),
        };
        assert_eq!(object(need), &Object::Dictionary(want));
        let name = format!("/{} BDC", String::from_utf8_lossy(&need.name));
        assert!(String::from_utf8_lossy(out).contains(&name), "{out:?}");
    }
    let marked = String::from_utf8_lossy(out).matches(" BDC").count();
    assert_eq!(marked, 2);
}

/// The same header on the same page gives the same bytes, names and
/// digest, twice and on two threads.
#[test]
fn identical_headers_write_identical_bytes() {
    fn write() -> Drawn {
        let spec = HeaderFooterSpec {
            texts: [
                "Café “x”".into(),
                "Page <<1>> of <<N>>".into(),
                "<<bates>>".into(),
                "<<file>>".into(),
                String::new(),
                "<<date>>".into(),
            ],
            bates: Some(sheaf_edit::BatesSpec::default()),
            color: [200, 30, 30],
            ..crate::tx::fixture::header("")
        };
        let ctx = PageCtx {
            geom: page(Rotation::R90),
            position: 4,
            count: 9,
            label: "5".into(),
            bates: Some(17),
        };
        layout(
            &header_gen(7, spec),
            &ctx,
            None,
            &Reserved::default(),
            Private::Keep,
        )
    }
    let first = write();
    assert!(!first.ops.is_empty());
    assert_eq!(write(), first);
    let threads: Vec<_> = (0..2).map(|_| std::thread::spawn(write)).collect();
    for t in threads {
        assert_eq!(t.join().unwrap(), first);
    }
}

/// On a page turned any way, a header's text reads left to right along
/// the top of the page as it shows, its baseline `top` and its cap height
/// below the top edge, at the left margin; a footer's along the bottom,
/// `bottom` above it.
#[test]
fn a_header_is_laid_out_upright_on_the_page_as_it_shows() {
    let mut spec = crate::tx::fixture::header("Head");
    spec.texts[4] = "Foot".into();
    let g = header_gen(1, spec);
    for r in [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270] {
        let geom = page(r);
        let size = geom.shown();
        let drawn = layout(
            &g,
            &ctx_of(&geom),
            None,
            &Reserved::default(),
            Private::Keep,
        );
        let text = String::from_utf8_lossy(&drawn.ops).into_owned();
        let tm: Vec<Affine> = (text.lines())
            .filter(|l| l.ends_with(" Tm"))
            .map(|line| {
                let n: Vec<f32> = (line.split(' ').take(6))
                    .map(|v| v.parse().unwrap())
                    .collect();
                Affine {
                    a: n[0],
                    b: n[1],
                    c: n[2],
                    d: n[3],
                    e: n[4],
                    f: n[5],
                }
            })
            .collect();
        let [head, foot] = tm[..] else {
            panic!("{text}")
        };
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        for m in [head, foot] {
            let o = shown(&geom, m, Pt { x: 0.0, y: 0.0 });
            let along = shown(&geom, m, Pt { x: 1.0, y: 0.0 });
            let up = shown(&geom, m, Pt { x: 0.0, y: 1.0 });
            assert!(near(along.x - o.x, 1.0) && near(along.y, o.y), "{r:?}");
            assert!(near(up.x, o.x) && near(up.y - o.y, -1.0), "{r:?}");
        }
        let o = shown(&geom, head, Pt { x: 0.0, y: 0.0 });
        assert!(
            near(o.x, 72.0) && near(o.y, 36.0 + 10.0 * 0.72),
            "{r:?}: {o:?}"
        );
        let o = shown(&geom, foot, Pt { x: 0.0, y: 0.0 });
        let w = Std14::Helvetica.width("Foot", 10.0);
        assert!(near(o.x + w / 2.0, size.width / 2.0), "{r:?}: {o:?}");
        assert!(near(o.y, size.height - 36.0), "{r:?}: {o:?}");
    }
}

/// Where every slot is blank, as a Bates number on a page without one,
/// nothing is drawn: the block is empty, laid out as nothing is.
#[test]
fn a_header_with_nothing_to_draw_draws_nothing() {
    let spec = HeaderFooterSpec {
        bates: Some(sheaf_edit::BatesSpec::default()),
        ..crate::tx::fixture::header("<<bates>>")
    };
    let g = header_gen(1, spec);
    let geom = page(Rotation::R0);
    let none = layout(
        &g,
        &ctx_of(&geom),
        None,
        &Reserved::default(),
        Private::Keep,
    );
    assert!(none.ops.is_empty() && none.res.0.is_empty());
    assert_eq!(none.layout_digest, Drawn::empty().layout_digest);
    let numbered = PageCtx {
        bates: Some(3),
        ..ctx_of(&geom)
    };
    let drawn = layout(&g, &numbered, None, &Reserved::default(), Private::Keep);
    assert!(String::from_utf8_lossy(&drawn.ops).contains("(000003) Tj"));
    assert_eq!(layout_digest(&g, &numbered, None), drawn.layout_digest);
}
