//! Op JSON: every op and every reference form round trips, references and
//! specs refuse what they don't know with an error that says so, and an
//! envelope refuses a version this build can't read.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use sheaf_edit::fixture::{blob, every_op, layer};
use sheaf_edit::{
    AnnotId, AnnotRef, BlobId, BlobSel, BookmarkPatch, BookmarkRef, BoxSpec, Envelope, FontChoice,
    GridPos, Local, Margins, Op, OutlineId, PageId, PageRef, PageSel, Patch, RangeSet, Rotation,
    ScopeSel, Turn, WatermarkContent, WatermarkSpec, Z,
};

fn pos(s: &str) -> RangeSet {
    s.parse().unwrap()
}

fn read<T: serde::de::DeserializeOwned>(json: Value) -> Result<T, String> {
    serde_json::from_value(json).map_err(|e| e.to_string())
}

#[test]
fn every_op_and_every_reference_form_round_trips() {
    for op in every_op() {
        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(read::<Op>(json.clone()), Ok(op), "{json}");
    }
    let envelope = Envelope::new(Some("Everything".into()), every_op());
    let text = serde_json::to_string(&envelope).unwrap();
    assert!(text.starts_with(r#"{"v":"1.0","label":"Everything","ops":["#));
    assert_eq!(serde_json::from_str::<Envelope>(&text).unwrap(), envelope);
}

/// The values `"op"` and a mark's `"type"` take in the schema: every op
/// and every kind there is.
fn tags(schema: &Value, def: &str, tag: &str) -> BTreeSet<String> {
    let variants = schema["$defs"][def]["oneOf"].as_array().unwrap();
    let tag = |v: &Value| v["properties"][tag]["const"].as_str().unwrap().to_owned();
    variants.iter().map(tag).collect()
}

#[test]
fn the_fixture_has_every_op_and_every_kind_of_mark() {
    let schema = serde_json::to_value(schemars::schema_for!(Envelope)).unwrap();
    let ops = every_op();
    let json = serde_json::to_value(&ops).unwrap();
    let json = json.as_array().unwrap();
    let ops_in: BTreeSet<String> = json
        .iter()
        .map(|o| o["op"].as_str().unwrap().into())
        .collect();
    assert_eq!(ops_in, tags(&schema, "Op", "op"));
    let kinds_in: BTreeSet<String> = (json.iter())
        .filter_map(|o| o["mark"]["kind"]["type"].as_str())
        .map(Into::into)
        .collect();
    assert_eq!(kinds_in, tags(&schema, "MarkKind", "type"));
    // And each reference in each of its forms: an id, a local, an object.
    let text = serde_json::to_string(&ops).unwrap();
    for form in [
        r#""pages":{"ids":[17,"$1"]}"#,
        r#""before":{"pos":3}"#,
        r#""pages":"all""#,
        r#""pages":{"span":{"first":{"pos":1},"last":9}}"#,
        r#""pages":{"positions":"1-3,7"}"#,
        r#""pages":{"bookmark":{"path":["Part I","Ch. 2"]}}"#,
        r#""pages":{"bookmark":4}"#,
        r#""pages":{"bookmark":"$2"}"#,
        r#""by":90"#,
        r#""by":180"#,
        r#""by":270"#,
        r#""from":{"type":"file","file":{"blob":"b99aa06d3014798d86001c324468d497f-2048"},"name":"form.pdf"}"#,
        r#""file":{"with":"logo"}"#,
        r#""file":{"asset":2}"#,
        r#""at":"last""#,
        r#""at":"first""#,
        r#""at":{"after":"$1"}"#,
        r#""reply_to":{"nm":"a1b2-c3","page":{"pos":4}}"#,
        r#""marks":[42,"$7"]"#,
        r#""marks":[{"nm":"sheaf:12"}]"#,
        r#""by":{"page":{"x":10.0,"y":-5.5}}"#,
        r#""by":{"view":{"x":0.0,"y":12.0}}"#,
        r#""target":"$5","view":null"#,
        r#""fill":null"#,
        r#""fill":[1,2,3]"#,
        r#""scope":"all""#,
        r#""scope":{"pages":{"ids":[{"pos":2}]}}"#,
        r#""crop":{"margins":{"top":36.0,"bottom":36.0,"left":18.0,"right":18.0}}"#,
        r#""trim":{"rect":{"x":18.0,"y":18.0,"w":576.0,"h":756.0}}"#,
        r#""bleed":null"#,
        r#""crop":null"#,
    ] {
        assert!(text.contains(form), "{form}");
    }
}

#[test]
fn references_read_in_their_forms_and_say_what_is_wrong() {
    assert_eq!(read(json!(17)), Ok(PageRef::Id(PageId(17))));
    assert_eq!(read(json!("$3")), Ok(PageRef::New(Local(3))));
    assert_eq!(read(json!({"pos": 3})), Ok(PageRef::Pos(3)));
    assert_eq!(read(json!(9)), Ok(BookmarkRef::Id(OutlineId(9))));
    assert_eq!(
        read(json!({"path": ["Part I"]})),
        Ok(BookmarkRef::Path(vec!["Part I".into()]))
    );
    assert_eq!(read(json!(4)), Ok(AnnotRef::Id(AnnotId(4))));
    assert_eq!(
        read(json!({"nm": "x"})),
        Ok(AnnotRef::Nm {
            nm: "x".into(),
            page: None
        })
    );
    // A misspelled local is an error, not another form.
    for bad in ["$x", "$", "3", "x$1", "$70000", "$-1"] {
        let e = read::<PageRef>(json!(bad)).unwrap_err();
        assert!(e.contains("isn't a local"), "{bad}: {e}");
        assert!(read::<AnnotRef>(json!(bad)).is_err(), "{bad}");
        assert!(read::<BookmarkRef>(json!(bad)).is_err(), "{bad}");
    }
    let e = read::<PageRef>(json!({"pos": 0})).unwrap_err();
    assert!(e.contains("1-based"), "{e}");
    let e = read::<PageRef>(json!({"pos": 1, "page": 2})).unwrap_err();
    assert!(e.contains("unknown field `page`"), "{e}");
    let e = read::<PageRef>(json!({"position": 1})).unwrap_err();
    assert!(e.contains("unknown field `position`"), "{e}");
    let e = read::<PageRef>(json!(-3)).unwrap_err();
    assert!(e.contains("a page: its id"), "{e}");
    let e = read::<PageRef>(json!(true)).unwrap_err();
    assert!(e.contains("a page: its id"), "{e}");
    let e = read::<BookmarkRef>(json!({"path": []})).unwrap_err();
    assert!(e.contains("at least its own title"), "{e}");
    let e = read::<AnnotRef>(json!({"nm": "x", "pag": 1})).unwrap_err();
    assert!(e.contains("unknown field `pag`"), "{e}");
    let e = read::<BlobSel>(json!({"blob": "b12-3"})).unwrap_err();
    assert!(e.contains("a blob id"), "{e}");
    let e = read::<BlobSel>(json!({"path": "a.pdf"})).unwrap_err();
    assert!(e.contains("unknown variant `path`"), "{e}");
    let e = read::<Turn>(json!(45)).unwrap_err();
    assert!(e.contains("90, 180, 270 or -90"), "{e}");
    assert_eq!(read(json!(-90)), Ok(Turn::Back));
    assert_eq!(Turn::Back.rotation(), Rotation::R270);
}

#[test]
fn patches_keep_clear_and_set() {
    let patch = |json| read::<BookmarkPatch>(json).unwrap();
    assert_eq!(patch(json!({})), BookmarkPatch::default());
    assert_eq!(patch(json!({"color": null})).color, Patch::Clear);
    assert_eq!(
        patch(json!({"color": [1, 2, 3]})).color,
        Patch::Set([1, 2, 3])
    );
    assert_eq!(
        patch(json!({"target": "$2"})).target,
        Patch::Set(PageRef::New(Local(2)))
    );
    // Kept is left out, cleared is null.
    let cleared = BookmarkPatch {
        view: Patch::Clear,
        ..BookmarkPatch::default()
    };
    assert_eq!(
        serde_json::to_value(&cleared).unwrap(),
        json!({"view": null})
    );
    assert!(serde_json::to_value(Patch::<u8>::Keep).is_err());
    let mut slot = Some(1);
    Patch::Keep.apply(&mut slot);
    assert_eq!(slot, Some(1));
    Patch::Set(2).apply(&mut slot);
    assert_eq!(slot, Some(2));
    Patch::Clear.apply(&mut slot);
    assert_eq!(slot, None);
}

/// A page's boxes are patches too: one left out is kept, `null` clears
/// it, and a box is set by margins or as a rectangle.
#[test]
fn page_boxes_are_kept_cleared_or_set() {
    let op = read::<Op>(json!({
        "op": "set-page-boxes", "pages": "all", "art": null,
        "crop": {"margins": {"top": 1, "bottom": 2, "left": 3, "right": 4}},
    }));
    let margins = Margins {
        top: 1.0,
        bottom: 2.0,
        left: 3.0,
        right: 4.0,
    };
    let want = Op::SetPageBoxes {
        pages: PageSel::All,
        crop: Patch::Set(BoxSpec::Margins(margins)),
        trim: Patch::Keep,
        bleed: Patch::Keep,
        art: Patch::Clear,
    };
    assert_eq!(op, Ok(want));
    let e = read::<Op>(json!({"op": "set-page-boxes", "pages": "all", "trim": {"margin": {}}}))
        .unwrap_err();
    assert!(e.contains("unknown variant `margin`"), "{e}");
}

/// A watermark says what it draws by `type`; its font, position, angle
/// and whether it takes the author's own are said only when they aren't
/// the usual (Helvetica Bold, the middle, upright, not), and its image or
/// PDF only when it isn't the one drawn now (a PDF's page 1 when none is
/// said); misspelled, or with a field of another kind's, it's refused.
#[test]
fn watermarks_read_with_their_defaults() {
    let text = json!({
        "op": "set-watermark", "z": "over", "scope": "all",
        "spec": {"content": {"type": "text", "text": "DRAFT", "size": 72, "color": [200, 30, 30],
                             "opacity": 0.3}},
    });
    let want = Op::SetWatermark {
        spec: WatermarkSpec {
            content: WatermarkContent::Text {
                text: "DRAFT".into(),
                font: FontChoice::HelveticaBold,
                size: 72.0,
                color: [200, 30, 30],
                opacity: 0.3,
                angle: 0.0,
            },
            position: GridPos::Center,
        },
        z: Z::Over,
        scope: ScopeSel::All,
    };
    assert_eq!(read::<Op>(text), Ok(want));
    let page = json!({
        "op": "set-watermark", "z": "under", "scope": {"pages": {"positions": "2-3"}},
        "spec": {"content": {"type": "page", "pdf": {"with": "logo"}, "page": 1, "scale": 0.5,
                             "opacity": 1},
                 "position": "bottom-right"},
    });
    let want = Op::SetWatermark {
        spec: WatermarkSpec {
            content: WatermarkContent::Page {
                pdf: Some(BlobSel::With("logo".into())),
                page: 1,
                scale: 0.5,
                opacity: 1.0,
            },
            position: GridPos::BottomRight,
        },
        z: Z::Under,
        scope: ScopeSel::Pages(PageSel::Positions(pos("2-3"))),
    };
    assert_eq!(read::<Op>(page), Ok(want));
    // With no file, the image or page drawn now, as its z draws it.
    let kept = json!({
        "op": "set-watermark", "z": "under", "scope": "all",
        "spec": {"content": {"type": "image", "scale": 0.25, "opacity": 0.5},
                 "position": "center"},
    });
    let want = Op::SetWatermark {
        spec: WatermarkSpec {
            content: WatermarkContent::Image {
                image: None,
                scale: 0.25,
                opacity: 0.5,
            },
            position: GridPos::Center,
        },
        z: Z::Under,
        scope: ScopeSel::All,
    };
    let op = read::<Op>(kept.clone());
    assert_eq!(op, Ok(want.clone()));
    assert_eq!(serde_json::to_value(&want).unwrap(), kept);
    let page = read::<Op>(json!({
        "op": "set-watermark", "z": "under", "scope": "all",
        "spec": {"content": {"type": "page", "scale": 0.25, "opacity": 0.5}},
    }));
    let kept = WatermarkContent::Page {
        pdf: None,
        page: 1,
        scale: 0.25,
        opacity: 0.5,
    };
    assert!(matches!(page, Ok(Op::SetWatermark { spec, .. }) if spec.content == kept));
    let remove = Op::RemoveWatermark {
        z: None,
        pages: PageSel::All,
        include_authored: false,
    };
    let json = json!({"op": "remove-watermark", "pages": "all"});
    assert_eq!(serde_json::to_value(&remove).unwrap(), json);
    assert_eq!(read::<Op>(json), Ok(remove));
    let font =
        |font: &str| read::<FontChoice>(json!(font)).map(|f| serde_json::to_value(f).unwrap());
    for face in ["times-roman", "helvetica-bold-oblique", "courier"] {
        assert_eq!(font(face), Ok(json!(face)));
    }
    let image = |content: Value| json!({"op": "set-watermark", "z": "over", "scope": "all", "spec": {"content": content}});
    for (bad, why) in [
        (
            image(
                json!({"type": "image", "image": {"asset": 0}, "scale": 1, "opacity": 1,
                         "angle": 45}),
            ),
            "unknown field `angle`",
        ),
        (
            image(json!({"type": "image", "image": {"asset": 0}, "scale": 1})),
            "missing field `opacity`",
        ),
        (
            image(
                json!({"type": "text", "text": "A", "size": 9, "color": [0, 0, 0],
                         "opacity": 1, "font": "arial"}),
            ),
            "unknown variant `arial`",
        ),
        (
            image(json!({"type": "picture", "image": {"asset": 0}})),
            "unknown variant `picture`",
        ),
        (
            json!({"op": "set-watermark", "z": "behind", "scope": "all",
                   "spec": {"content": {"type": "image", "image": {"asset": 0}, "scale": 1,
                                        "opacity": 1}}}),
            "unknown variant `behind`",
        ),
        (
            json!({"op": "set-watermark", "z": "over", "scope": "all",
                   "spec": {"content": {"type": "image", "image": {"asset": 0}, "scale": 1,
                                        "opacity": 1},
                            "position": "middle"}}),
            "unknown variant `middle`",
        ),
        (
            json!({"op": "set-watermark", "z": "over", "scope": "all",
                   "spec": {"content": {"type": "image", "image": {"asset": 0}, "scale": 1,
                                        "opacity": 1},
                            "anchor": "center"}}),
            "unknown field `anchor`",
        ),
        (
            json!({"op": "remove-watermark", "pages": "all", "include_authord": true}),
            "unknown field `include_authord`",
        ),
    ] {
        let e = read::<Op>(bad.clone()).unwrap_err();
        assert!(e.contains(why), "{bad}: {e}");
    }
}

#[test]
fn a_misspelled_field_is_refused() {
    let delete = json!({"op": "delete-pages", "pages": "all"});
    assert!(read::<Op>(delete).is_ok());
    let bookmark = |more: Value| {
        let mut op = json!({"op": "add-bookmark", "at": "last", "title": "A"});
        op.as_object_mut()
            .unwrap()
            .extend(more.as_object().unwrap().clone());
        op
    };
    let ocr = |edit: &dyn Fn(&mut Value)| {
        let mut layer = serde_json::to_value(layer()).unwrap();
        edit(&mut layer);
        json!({"op": "set-ocr-text", "page": 1, "layer": layer})
    };
    for (bad, field) in [
        (
            bookmark(json!({"view": {"Xyz": {"lft": 3.0, "top": null, "zoom": null}}})),
            "lft",
        ),
        (
            bookmark(json!({"view": {"Other": {"kind": "Fit", "params": [], "zoom": 1}}})),
            "zoom",
        ),
        (bookmark(json!({"style": {"bld": true}})), "bld"),
        (ocr(&|l| l["engin"] = "typo".into()), "engin"),
        (ocr(&|l| l["words"][0]["conf"] = 1.into()), "conf"),
        (json!({"op": "delete-pages", "page": "all"}), "page"),
        (
            json!({"op": "delete-pages", "pages": "all", "with_pages": true}),
            "with_pages",
        ),
        (
            json!({"op": "move-marks", "marks": [1], "by": {"page": {"x": 1, "y": 2, "z": 3}}}),
            "z",
        ),
        (
            json!({"op": "add-mark", "page": 1, "mark": {
                "kind": {"type": "note", "pos": {"x": 1, "y": 2}, "size": 3},
                "style": {"color": [0, 0, 0], "opacity": 1, "stroke": 1},
            }}),
            "size",
        ),
        (
            json!({"op": "add-mark", "page": 1, "mark": {
                "kind": {"type": "note", "pos": {"x": 1, "y": 2}},
                "style": {"color": [0, 0, 0], "opacity": 1, "stroke": 1, "colour": [1, 1, 1]},
            }}),
            "colour",
        ),
        (
            json!({"op": "add-mark", "page": 1, "mark": {
                "kind": {"type": "note", "pos": {"x": 1, "y": 2}},
                "style": {"color": [0, 0, 0], "opacity": 1, "stroke": 1},
                "id": 7,
            }}),
            "id",
        ),
        (
            json!({"op": "insert-pages", "from": {"type": "file", "file": {"asset": 1}, "name": "a.pdf"},
                   "bring": {"marks": true, "comments": false}}),
            "comments",
        ),
        (
            json!({"op": "patch-marks", "marks": [1], "patch": {"modified": "D:2026"}}),
            "modified",
        ),
        (
            json!({"op": "set-page-boxes", "pages": "all", "media": null}),
            "media",
        ),
        (
            json!({"op": "set-page-boxes", "pages": "all", "crop": {"margins": {
                "top": 1, "bottom": 1, "left": 1, "right": 1, "inner": 1,
            }}}),
            "inner",
        ),
    ] {
        let e = read::<Op>(bad.clone()).unwrap_err();
        assert!(
            e.contains(&format!("unknown field `{field}`")),
            "{bad}: {e}"
        );
    }
    let e = read::<Op>(json!({"op": "delete-page", "pages": "all"})).unwrap_err();
    assert!(e.contains("unknown variant `delete-page`"), "{e}");
}

#[test]
fn envelopes_refuse_versions_they_cant_read() {
    let ops = json!([{"op": "delete-pages", "pages": {"positions": "1"}}]);
    let envelope = |v: &str| read::<Envelope>(json!({"v": v, "ops": ops}));
    let read_one = envelope("1.0").unwrap();
    assert_eq!(
        (read_one.v.as_str(), read_one.label, read_one.ops.len()),
        ("1.0", None, 1)
    );
    for (v, why) in [
        ("1.1", "written in version 1.1"),
        ("2.0", "written in version 2.0"),
        ("0.9", "written in version 0.9"),
        ("1", "isn't an Op JSON version"),
        ("1.x", "isn't an Op JSON version"),
    ] {
        let e = envelope(v).unwrap_err();
        assert!(e.contains(why), "{v}: {e}");
    }
    // A newer version's op is refused for its version, written first.
    let newer = r#"{"v": "1.1", "ops": [{"op": "flatten-annotations", "pages": "all"}]}"#;
    let e = serde_json::from_str::<Envelope>(newer)
        .unwrap_err()
        .to_string();
    assert!(e.contains("written in version 1.1"), "{e}");
    let e = read::<Envelope>(json!({"ops": ops})).unwrap_err();
    assert!(e.contains("missing field `v`"), "{e}");
    let e = read::<Envelope>(json!({"v": "1.0", "ops": ops, "user": "x"})).unwrap_err();
    assert!(e.contains("unknown field `user`"), "{e}");
    let e = serde_json::from_str::<Envelope>(r#"{"v": "1.0", "v": "1.0", "ops": []}"#)
        .unwrap_err()
        .to_string();
    assert!(e.contains("duplicate field `v`"), "{e}");
}

#[test]
fn range_sets_parse_sorted_and_merged() {
    let set = pos(" 10-12, 1-3 ,5,4, 2");
    assert_eq!(set.runs(), [1..=5, 10..=12]);
    assert_eq!(set.to_string(), "1-5,10-12");
    assert_eq!(set.iter().collect::<Vec<_>>(), [1, 2, 3, 4, 5, 10, 11, 12]);
    assert_eq!(set.last(), 12);
    assert_eq!(pos("7").iter().collect::<Vec<_>>(), [7]);
    assert_eq!(read(json!("3,1")), Ok(pos("1,3")));
    assert_eq!(serde_json::to_value(pos("3,1-2")).unwrap(), json!("1-3"));
    let merged = RangeSet::from_runs([4..=6, 1..=2, 3..=3, 5..=9]).unwrap();
    assert_eq!(merged.to_string(), "1-9");
    assert_eq!(
        RangeSet::from_runs([u32::MAX..=u32::MAX, 1..=1])
            .unwrap()
            .last(),
        u32::MAX
    );
    for bad in [
        "",
        " ",
        "0",
        "0-2",
        "3-1",
        "a",
        "1-",
        "-2",
        "1,,2",
        "1.5",
        "+1",
        "4294967296",
    ] {
        let e = bad.parse::<RangeSet>().unwrap_err().to_string();
        assert!(
            e.contains("1-based") || e.contains("no pages"),
            "{bad}: {e}"
        );
    }
    assert!(RangeSet::from_runs([]).is_err());
    assert!(RangeSet::from_runs(std::iter::once(0..=1)).is_err());
    let backwards = std::ops::RangeInclusive::new(3, 1);
    assert!(RangeSet::from_runs(std::iter::once(backwards)).is_err());
}

#[test]
fn blob_ids_are_written_as_b_hex_and_length() {
    let id = blob();
    let json = serde_json::to_value(id).unwrap();
    assert_eq!(json, json!("b99aa06d3014798d86001c324468d497f-2048"));
    assert_eq!(read(json), Ok(id));
    assert_eq!(BlobId::from_stem(&id.stem()), Some(id));
    let small = BlobId { hash: 1, len: 0 };
    assert_eq!(
        serde_json::to_value(small).unwrap(),
        json!(format!("b{:032x}-0", 1))
    );
    for bad in [
        "99aa06d3014798d86001c324468d497f-2048",
        "b99AA06D3014798D86001C324468D497F-2048",
        "b99aa06d3014798d-2048",
        "b99aa06d3014798d86001c324468d497f-02048",
        "b99aa06d3014798d86001c324468d497f",
        "b99aa06d3014798d86001c324468d497f-",
    ] {
        assert!(read::<BlobId>(json!(bad)).is_err(), "{bad}");
    }
}
