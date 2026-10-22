//! The WIT worlds this host serves: 0.3, the current one, and 0.2 (N−1),
//! each bound on its own (their names differ by version, so both link
//! into one `Linker`). What they have in common is written once,
//! `shared.rs`, and included in each version's module, where `api` is
//! that version's bindings. Marks (0.2 has fewer kinds and fields),
//! `storage` (0.3 adds preferences) and `ocr` (only 0.2 asks for a page's
//! layer, and is answered none) differ: `v03.rs`, `v02.rs`.

pub(crate) mod bindings_v03 {
    wasmtime::component::bindgen!({
        world: "plugin",
        path: "../../wit",
        with: {
            "sheaf:plugin/render.page-renderer": crate::render::PageRenderer,
            "sheaf:plugin/render.image": crate::render::RenderedImage,
            "sheaf:plugin/progress.task": crate::progress::Task,
        },
    });
}

pub(crate) mod bindings_v02 {
    wasmtime::component::bindgen!({
        world: "plugin",
        path: "../../wit/0.2",
        with: {
            "sheaf:plugin/render.page-renderer": crate::render::PageRenderer,
            "sheaf:plugin/render.image": crate::render::RenderedImage,
            "sheaf:plugin/progress.task": crate::progress::Task,
        },
    });
}

/// API 0.3.
pub(crate) mod v03 {
    pub(crate) use super::bindings_v03::Plugin as World;
    use super::bindings_v03::sheaf::plugin as api;

    include!("shared.rs");
    include!("v03.rs");
}

/// API 0.2, still served.
pub(crate) mod v02 {
    pub(crate) use super::bindings_v02::Plugin as World;
    use super::bindings_v02::sheaf::plugin as api;

    include!("shared.rs");
    include!("v02.rs");
}

#[cfg(test)]
mod tests {
    use sheaf_doc::measure::{Measured, Scale};
    use sheaf_doc::prim::Key;
    use sheaf_doc::tx::fixture;
    use sheaf_doc::tx::mark_style;
    use sheaf_doc::{
        Actor, Annot, AnnotId, AnnotKind, CommitOpts, Document, FixedCount, OutlineId, Pt, Store,
    };
    use sheaf_edit::{BookmarkPatch, BookmarkRef, Op, Patch, Rotation, Style};

    fn mark(kind: AnnotKind) -> Annot {
        Annot {
            id: AnnotId(7),
            kind,
            color: [1, 2, 3],
            opacity: 0.5,
            stroke: 2.0,
            author: "Greg".into(),
            contents: "c".into(),
            modified: "D:20260101000000".into(),
            created: "D:20260101000000".into(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        }
    }

    /// Mark `$a` shown to a plugin through API `$api` and said back as it
    /// was: its kind, look and text.
    macro_rules! said_back {
        ($api:ident, $a:expr) => {{
            let said = super::$api::said(super::$api::annot_to_wit($a).unwrap());
            let style = said.style();
            (said.kind, style, said.contents)
        }};
    }

    /// What a plugin is shown of `a`, as it says it.
    fn shown(a: &Annot) -> (AnnotKind, Style, String) {
        (a.kind.clone(), mark_style(a), a.contents.clone())
    }

    #[test]
    fn marks_round_trip_in_both_apis() {
        let ink = mark(AnnotKind::Ink {
            paths: vec![vec![Pt { x: 1.0, y: 2.0 }, Pt { x: 3.0, y: 4.0 }]],
        });
        assert_eq!(said_back!(v02, &ink), shown(&ink));
        assert_eq!(said_back!(v03, &ink), shown(&ink));
        // 0.3 has every kind and field; 0.2 doesn't show the new kinds.
        let mut area = mark(AnnotKind::Measure {
            what: Measured::Area,
            points: vec![
                Pt { x: 0.0, y: 0.0 },
                Pt { x: 72.0, y: 0.0 },
                Pt { x: 72.0, y: 72.0 },
            ],
            scale: Box::new(Scale::default()),
        });
        area.dashed = true;
        assert_eq!(said_back!(v03, &area), shown(&area));
        assert!(super::v02::annot_to_wit(&area).is_none());
        let mut poly = mark(AnnotKind::Polygon {
            points: vec![
                Pt { x: 0.0, y: 0.0 },
                Pt { x: 9.0, y: 0.0 },
                Pt { x: 9.0, y: 9.0 },
            ],
        });
        poly.fill = Some([9, 9, 9]);
        poly.cloudy = true;
        let wit = super::v03::annot_to_wit(&poly).unwrap();
        assert_eq!(wit.name, "sheaf:7");
        assert_eq!(said_back!(v03, &poly), shown(&poly));
    }

    /// A plugin's units are sheaf's, with the nouns the status bar shows.
    #[test]
    fn units_cross_both_apis() {
        use super::bindings_v02::sheaf::plugin::progress::Unit as V02;
        use super::bindings_v03::sheaf::plugin::progress::Unit as V03;
        use in_progress::Unit;
        let v02 = [V02::Items, V02::Pages, V02::Bytes].map(super::v02::unit);
        let v03 = [V03::Items, V03::Pages, V03::Bytes].map(super::v03::unit);
        for units in [v02, v03] {
            assert_eq!(units, [Unit::ITEMS, sheaf_doc::PAGES, Unit::Bytes]);
            assert_eq!(units.map(Unit::noun), ["items", "pages", "bytes"]);
        }
    }

    #[test]
    fn outline_edit_keeps_clear_vs_unchanged() {
        use super::bindings_v03::sheaf::plugin::types as w;
        let mut ops = super::v03::Adapter::new(None, "Edit".into());
        ops.edit(w::Edit::OutlineEdit(w::OutlineEdit {
            id: 3,
            title: None,
            target: Some(None),
            color: None,
            view: None,
        }));
        assert_eq!(
            ops.finish().0.ops,
            [Op::PatchBookmarks {
                bookmarks: vec![BookmarkRef::Id(OutlineId(3))],
                patch: BookmarkPatch {
                    target: Patch::Clear,
                    ..BookmarkPatch::default()
                },
            }]
        );
    }

    /// A bookmark tree's top ones go in order from where it says, each
    /// under the last one a level up. Only one with others under it is
    /// named by a local, so a flat list of any length fits in a batch, and
    /// more with others under them than locals can name is refused.
    #[test]
    fn a_bookmark_tree_names_only_its_parents() {
        use super::bindings_v03::sheaf::plugin::types as w;
        let item = |title: String, depth| w::NewOutlineItem {
            title,
            target: None,
            view: None,
            depth,
        };
        let tree = |items| {
            let mut ops = super::v03::Adapter::new(None, "Outline".into());
            ops.edit(w::Edit::OutlineInsertTree(w::OutlineInsertTree {
                parent: None,
                after: None,
                items,
            }));
            ops.finish().0
        };
        let named = |ops: &[Op]| {
            let named = |op: &&Op| matches!(op, Op::AddBookmark { local: Some(_), .. });
            ops.iter().filter(named).count()
        };
        let said = [
            ("A", 0),
            ("A1", 1),
            ("A1a", 2),
            ("A2", 1),
            ("B", 0),
            ("C", 0),
            ("C1", 1),
        ];
        let batch = tree(said.map(|(t, depth)| item(t.into(), depth)).into());
        assert_eq!(named(&batch.ops), 3, "A, A1 and C");
        let mut d = blank();
        let plugin = Actor::Plugin {
            id: "outliner".into(),
            version: "1.0.0".into(),
        };
        let mut tx = d.tx(plugin);
        batch.lowered(&mut tx, &d).unwrap();
        d.commit(tx, CommitOpts::default()).unwrap();
        let st = d.state();
        let mut placed = Vec::new();
        let mut stack: Vec<(OutlineId, u16)> = (st.outline_roots().iter().rev())
            .map(|&id| (id, 0))
            .collect();
        while let Some((id, depth)) = stack.pop() {
            let b = st.bookmark(id).unwrap();
            placed.push((b.title.to_string(), depth));
            stack.extend(b.kids.iter().rev().map(|&kid| (kid, depth + 1)));
        }
        assert_eq!(placed, said.map(|(t, depth)| (t.to_string(), depth)));
        let flat = tree((0..70_000).map(|i| item(i.to_string(), 0)).collect());
        assert!(flat.invalid.is_none());
        assert_eq!((flat.ops.len(), named(&flat.ops)), (70_000, 0));
        let pairs = (0..u16::MAX).flat_map(|_| [item("p".into(), 0), item("c".into(), 1)]);
        let deep = tree(pairs.collect());
        assert!(
            matches!(deep.invalid, Some(sheaf_doc::EditError::Invalid { .. })),
            "{:?}",
            deep.invalid
        );
    }

    /// A blank page.
    fn blank() -> Document {
        let store = Store::new();
        let primary = sheaf_doc::store::blank(&store, 1);
        Document::new(
            store,
            primary,
            "a.pdf".into(),
            None,
            &[],
            Box::new(FixedCount(10)),
        )
    }

    /// One blank page, with `a` on it as the user made it: what a plugin
    /// then reads.
    fn with_mark(a: &Annot) -> (Document, AnnotId) {
        let mut d = blank();
        let page = d.page(0).unwrap();
        let made = d.commit_ops(vec![fixture::add(page, a)], "Add").unwrap();
        let id = made.created.annots().next().expect("the mark");
        (d, id)
    }

    /// `d`'s mark `id` as a plugin reads it through API `$api` (its types
    /// `$w`), changes it with `$change` and updates it, committed as the
    /// plugin's: the mark after.
    macro_rules! updated {
        ($api:ident, $w:ident, $d:expr, $id:expr, $change:expr) => {{
            let d: &mut Document = $d;
            let snap = d.snapshot();
            let page = d.page(0).unwrap();
            let (_, a) = snap.state.find_annot($id).unwrap();
            let to_view = super::$api::to_frame(&snap, page);
            let mut back = super::$api::annot_to_wit(&a.mapped(&to_view)).unwrap();
            let change: fn(&mut $w::Annot) = $change;
            change(&mut back);
            let mut ops = super::$api::Adapter::new(Some(&snap), "Check".into());
            ops.edit($w::Edit::AnnotUpdate($w::PageAnnot {
                page: page.0,
                annot: back,
            }));
            let plugin = Actor::Plugin {
                id: "checker".into(),
                version: "1.0.0".into(),
            };
            let (batch, read) = ops.finish();
            assert_eq!(read, [Key::Annot($id)], "the mark it keeps parts of");
            let mut tx = d.tx(plugin);
            batch.lowered(&mut tx, d).unwrap();
            d.commit(tx, CommitOpts::default()).unwrap();
            d.state().find_annot($id).unwrap().1.clone()
        }};
    }

    /// A plugin's update says what its API shows of a mark, and no more:
    /// the mark's creation date, name and turn, and a tape's rows and
    /// look, stay as they were; so do a 0.2 plugin's fill, dashes, cloud
    /// and start arrowhead, which 0.2 can't show. What it keeps is the mark
    /// as the call read it, so the update reads the mark.
    #[test]
    fn a_plugin_update_keeps_what_it_cant_see() {
        use super::bindings_v02::sheaf::plugin::types as w02;
        use super::bindings_v03::sheaf::plugin::types as w03;
        use sheaf_edit::tape::{Line, Op as TapeOp, Row, Tape};
        let mut tape = mark(AnnotKind::Tape {
            rect: sheaf_doc::Box2 {
                x: 10.0,
                y: 10.0,
                w: 90.0,
                h: 60.0,
            },
            size: 9.0,
            tape: Tape::default(),
            look: Default::default(),
        });
        if let AnnotKind::Tape { tape, look, .. } = &mut tape.kind {
            let entry = |value| {
                Line::new(Row::Entry {
                    value,
                    op: TapeOp::Add,
                })
            };
            tape.lines.extend([entry(12.5), entry(7.5)]);
            look.decimals = 3;
        }
        tape.turn = Rotation::R90;
        let (mut d, id) = with_mark(&tape);
        let was = d.state().find_annot(id).unwrap().1.clone();
        let now = updated!(v03, w03, &mut d, id, |b| {
            b.color = w03::Rgb { r: 9, g: 9, b: 9 };
            // What the host owns, which a plugin's update doesn't say.
            b.created = "D:20000101000000Z".into();
            b.name = "elsewhere".into();
            b.author = "the plugin".into();
        });
        assert_eq!(now.color, [9, 9, 9]);
        assert_eq!(
            (&now.created, now.name(), now.turn, &now.author),
            (&was.created, was.name(), was.turn, &was.author),
            "the host's, as it was made"
        );
        let rows = |a: &Annot| match &a.kind {
            AnnotKind::Tape { tape, look, .. } => (tape.lines.len(), look.decimals),
            other => panic!("{other:?}"),
        };
        assert_eq!(rows(&now), (2, 3), "the rows and look the API doesn't show");

        let mut line = mark(AnnotKind::Line {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
            arrow: true,
            arrow_start: true,
        });
        line.dashed = true;
        let (mut d, id) = with_mark(&line);
        let was = d.state().find_annot(id).unwrap().1.clone();
        let now = updated!(v02, w02, &mut d, id, |b| b.contents = "checked".into());
        assert_eq!(now.contents, "checked");
        assert_eq!(
            (now.dashed, &now.created, now.name()),
            (was.dashed, &was.created, was.name())
        );
        assert!(
            matches!(
                now.kind,
                AnnotKind::Line {
                    arrow_start: true,
                    ..
                }
            ),
            "{:?}",
            now.kind
        );
    }
}
