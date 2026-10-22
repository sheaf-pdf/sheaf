//! Op JSON to test with, for this crate's tests and other crates' (the
//! `fixtures` feature): every op, between them every form of every
//! reference, selector and patch, and every kind of mark. Op JSON fuzzing
//! starts from them.

use std::sync::Arc;

use crate::measure::{Measured, Scale};
use crate::tape::{self, Line, Row, Tape, TapeLook};
use crate::{
    Anchor, AnnotId, AnnotRef, BatesSpec, BlobId, BlobSel, BookmarkPatch, BookmarkRef, Box2,
    BoxSpec, Bring, Delta, DestView, FontChoice, FormBring, GridPos, HeaderFooterSpec, Imported,
    LabelStyle, Local, Margins, MarkKind, MarkPatch, NewMark, OcrWord, Op, OutlineBring, OutlineId,
    PageId, PageLabelRange, PageRef, PageSel, PageSource, Patch, Pt, RangeSet, Rotation, ScopeSel,
    Sibling, Style, TextLayer, TextStyle, Turn, Vec2, WatermarkContent, WatermarkSpec, Z,
};

fn r(x: f32, y: f32, w: f32, h: f32) -> Box2 {
    Box2 { x, y, w, h }
}

fn p(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

fn style() -> Style {
    Style {
        color: [0, 120, 255],
        opacity: 0.5,
        stroke: 1.5,
        fill: Some([255, 230, 0]),
        dashed: true,
        cloudy: true,
    }
}

fn pos(s: &str) -> RangeSet {
    s.parse().unwrap()
}

/// Every kind of mark sheaf makes.
fn kinds() -> Vec<MarkKind> {
    let rects = vec![r(1.0, 2.0, 3.0, 4.0), r(5.0, 6.0, 7.0, 8.0)];
    let tape = Tape {
        lines: vec![
            Line::new(Row::Title("Cash".into())),
            Line {
                row: Row::Entry {
                    value: 1234.5,
                    op: tape::Op::Sub,
                },
                note: "petty".into(),
            },
            Line::new(Row::Total(-1234.5)),
        ],
    };
    vec![
        MarkKind::Highlight {
            rects: rects.clone(),
        },
        MarkKind::Note { pos: p(10.0, 20.0) },
        MarkKind::FreeText {
            rect: r(0.0, 0.0, 100.0, 20.0),
            size: 12.0,
        },
        MarkKind::Square {
            rect: r(1.0, 1.0, 2.0, 2.0),
        },
        MarkKind::Ellipse {
            rect: r(1.0, 1.0, 2.0, 2.0),
        },
        MarkKind::Line {
            a: p(0.0, 0.0),
            b: p(10.0, 10.0),
            arrow: true,
            arrow_start: true,
        },
        MarkKind::Ink {
            paths: vec![vec![p(0.0, 0.0), p(1.0, 1.5)], vec![p(2.0, 2.0)]],
        },
        MarkKind::Stamp {
            rect: r(0.0, 0.0, 24.0, 24.0),
            glyph: "✓".into(),
        },
        MarkKind::Connector {
            rect: r(0.0, 0.0, 22.0, 22.0),
            label: "AA".into(),
        },
        MarkKind::Tape {
            rect: r(0.0, 0.0, 110.0, 60.0),
            size: 10.0,
            tape,
            look: TapeLook {
                numbers: false,
                titles: true,
                rotation: 90,
                paper: [255, 255, 200],
                decimals: 0,
            },
        },
        MarkKind::Underline {
            rects: rects.clone(),
        },
        MarkKind::StrikeOut {
            rects: rects.clone(),
        },
        MarkKind::Squiggly {
            rects: rects.clone(),
        },
        MarkKind::Replace {
            rects,
            struck: "old".into(),
            at: r(9.0, 9.0, 1.0, 4.0),
        },
        MarkKind::Insert {
            at: r(9.0, 9.0, 1.0, 4.0),
        },
        MarkKind::Reply {},
        MarkKind::Status {
            model: "Review".into(),
            state: "Accepted".into(),
        },
        MarkKind::PageStamp {
            rect: r(0.0, 0.0, 200.0, 60.0),
            name: "Approved".into(),
            text: Some("GE 10/19/2026".into()),
        },
        MarkKind::TextStamp {
            rect: r(0.0, 0.0, 200.0, 60.0),
            text: "APPROVED\nGE".into(),
            status: true,
        },
        MarkKind::PageBorder {
            rect: r(0.0, 0.0, 612.0, 792.0),
            status: "Reviewed".into(),
        },
        MarkKind::Polygon {
            points: vec![p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0)],
        },
        MarkKind::Polyline {
            points: vec![p(0.0, 0.0), p(1.0, 0.0)],
        },
        MarkKind::Callout {
            rect: r(0.0, 0.0, 100.0, 40.0),
            knee: p(120.0, 50.0),
            point: p(150.0, 80.0),
            size: 10.0,
        },
        MarkKind::FileAttachment {
            pos: p(30.0, 40.0),
            icon: "Paperclip".into(),
            name: "notes.txt".into(),
        },
        MarkKind::Measure {
            what: Measured::Area,
            points: vec![p(0.0, 0.0), p(72.0, 0.0), p(72.0, 72.0)],
            scale: Box::new(Scale {
                page: 0.125,
                page_unit: "in".into(),
                real: 1.0,
                unit: "ft".into(),
                precision: 1,
            }),
        },
    ]
}

fn imported() -> Imported {
    Imported {
        nm: "a1b2-c3".into(),
        created: "D:20261011153000".into(),
        modified: "D:20261012090000Z".into(),
        author: "Reviewer".into(),
    }
}

/// Recognized text of two words on one line, read upright turned 90°.
pub fn layer() -> Arc<TextLayer> {
    Arc::new(TextLayer {
        words: vec![
            OcrWord {
                text: "Form".into(),
                rect: r(72.0, 700.0, 40.0, 12.0),
                confidence: 0.9,
            },
            OcrWord {
                text: "1040".into(),
                rect: r(116.0, 700.0, 30.0, 12.0),
                confidence: 0.75,
            },
        ],
        lines: std::iter::once(0..2).collect(),
        upright: Rotation::R90,
        engine: "tesseract 5.5.2".into(),
    })
}

/// A blob no store has.
pub fn blob() -> BlobId {
    BlobId {
        hash: 0x99aa06d3014798d86001c324468d497f,
        len: 2048,
    }
}

/// Every op, between them every form of every reference, selector and
/// patch, and every kind of mark.
pub fn every_op() -> Vec<Op> {
    let file = |file, name: &str| PageSource::File {
        file,
        name: name.into(),
    };
    let mut ops = vec![
        Op::MovePages {
            pages: PageSel::Ids(vec![PageRef::Id(PageId(17)), PageRef::New(Local(1))]),
            before: Some(PageRef::Pos(3)),
        },
        Op::MovePages {
            pages: PageSel::All,
            before: None,
        },
        Op::RotatePages {
            pages: PageSel::Span {
                first: PageRef::Pos(1),
                last: PageRef::Id(PageId(9)),
            },
            by: Turn::Quarter,
        },
        Op::RotatePages {
            pages: PageSel::Positions(pos("1-3,7")),
            by: Turn::Half,
        },
        Op::RotatePages {
            pages: PageSel::Bookmark(BookmarkRef::Path(vec!["Part I".into(), "Ch. 2".into()])),
            by: Turn::Back,
        },
        Op::DeletePages {
            pages: PageSel::Bookmark(BookmarkRef::Id(OutlineId(4))),
        },
        Op::DuplicatePages {
            pages: PageSel::Bookmark(BookmarkRef::New(Local(2))),
            locals: vec![Local(3), Local(4)],
        },
        Op::InsertPages {
            from: file(BlobSel::Id(blob()), "form.pdf"),
            pages: Some(pos("2,4-5")),
            before: Some(PageRef::Id(PageId(1))),
            bring: Bring {
                marks: false,
                form: FormBring::New,
                outline: OutlineBring::None,
            },
            locals: vec![Local(5)],
        },
        Op::InsertPages {
            from: file(BlobSel::With("logo".into()), "logo.pdf"),
            pages: None,
            before: None,
            bring: Bring::default(),
            locals: vec![],
        },
        Op::ReplacePages {
            pages: vec![PageRef::Pos(2), PageRef::Pos(3)],
            from: file(BlobSel::Asset(2), "scan.pdf"),
            with: Some(pos("1")),
            bring: Bring {
                marks: true,
                form: FormBring::New,
                outline: OutlineBring::MergeByTitle,
            },
        },
        Op::SetPageBoxes {
            pages: PageSel::Positions(pos("2-3")),
            crop: Patch::Set(BoxSpec::Margins(Margins {
                top: 36.0,
                bottom: 36.0,
                left: 18.0,
                right: 18.0,
            })),
            trim: Patch::Set(BoxSpec::Rect(r(18.0, 18.0, 576.0, 756.0))),
            bleed: Patch::Clear,
            art: Patch::Keep,
        },
        Op::SetPageBoxes {
            pages: PageSel::All,
            crop: Patch::Clear,
            trim: Patch::Keep,
            bleed: Patch::Keep,
            art: Patch::Clear,
        },
        Op::SetPageLabels {
            ranges: Some(vec![
                PageLabelRange {
                    start: PageRef::Pos(1),
                    style: LabelStyle::LowerRoman,
                    prefix: String::new(),
                    first: 1,
                },
                PageLabelRange {
                    start: PageRef::Id(PageId(30)),
                    style: LabelStyle::Decimal,
                    prefix: "A-".into(),
                    first: 3,
                },
            ]),
        },
        Op::SetPageLabels { ranges: None },
        Op::AddBookmark {
            parent: None,
            at: Sibling::Last,
            title: "Part I".into(),
            target: Some(PageRef::Pos(1)),
            view: Some(DestView::Xyz {
                left: Some(72.0),
                top: None,
                zoom: Some(1.5),
            }),
            color: Some([220, 40, 40]),
            style: TextStyle {
                bold: true,
                italic: false,
            },
            local: Some(Local(1)),
        },
        Op::AddBookmark {
            parent: Some(BookmarkRef::New(Local(1))),
            at: Sibling::First,
            title: "Ch. 1".into(),
            target: None,
            view: Some(DestView::Other {
                kind: "FitR".into(),
                params: vec![Some(0.0), None, Some(612.0), Some(792.0)],
            }),
            color: None,
            style: TextStyle::default(),
            local: None,
        },
        Op::PatchBookmarks {
            bookmarks: vec![BookmarkRef::Id(OutlineId(9))],
            patch: BookmarkPatch {
                title: Some("Renamed".into()),
                target: Patch::Set(PageRef::New(Local(5))),
                view: Patch::Clear,
                color: Patch::Keep,
                style: Some(TextStyle {
                    bold: false,
                    italic: true,
                }),
            },
        },
        Op::MoveBookmarks {
            bookmarks: vec![BookmarkRef::Path(vec!["Appendix".into()])],
            parent: Some(BookmarkRef::Id(OutlineId(2))),
            at: Sibling::After(BookmarkRef::New(Local(1))),
            with_pages: true,
        },
        Op::DeleteBookmarks {
            bookmarks: vec![BookmarkRef::Id(OutlineId(3))],
        },
        Op::AddMark {
            page: PageRef::New(Local(5)),
            mark: NewMark {
                kind: MarkKind::Reply {},
                style: style(),
                contents: "Agreed".into(),
                author: Some("GE".into()),
                turn: Rotation::R270,
                asset: Some(BlobSel::Asset(1)),
            },
            reply_to: Some(AnnotRef::Nm {
                nm: "a1b2-c3".into(),
                page: Some(PageRef::Pos(4)),
            }),
            local: Some(Local(6)),
            imported: Some(imported()),
        },
        Op::AddConnector {
            a: Anchor {
                page: PageRef::Pos(1),
                rect: r(500.0, 220.0, 22.0, 22.0),
            },
            b: Anchor {
                page: PageRef::Id(PageId(8)),
                rect: r(100.0, 100.0, 22.0, 22.0),
            },
            label: "1".into(),
            style: style(),
            locals: vec![Local(7), Local(8)],
        },
        Op::PatchMarks {
            marks: vec![AnnotRef::Id(AnnotId(42)), AnnotRef::New(Local(7))],
            patch: MarkPatch {
                kind: Some(MarkKind::Square {
                    rect: r(1.0, 2.0, 3.0, 4.0),
                }),
                color: Some([0, 0, 0]),
                opacity: Some(1.0),
                stroke: Some(2.0),
                fill: Patch::Clear,
                dashed: Some(false),
                cloudy: Some(true),
                contents: Some(String::new()),
            },
            imported: Some(imported()),
        },
        Op::PatchMarks {
            marks: vec![AnnotRef::Nm {
                nm: "sheaf:12".into(),
                page: None,
            }],
            patch: MarkPatch {
                fill: Patch::Set([1, 2, 3]),
                ..MarkPatch::default()
            },
            imported: None,
        },
        Op::MoveMarks {
            marks: vec![AnnotRef::Id(AnnotId(42))],
            by: Delta::Page(Vec2 { x: 10.0, y: -5.5 }),
            to_page: None,
        },
        Op::MoveMarks {
            marks: vec![AnnotRef::New(Local(6))],
            by: Delta::View(Vec2 { x: 0.0, y: 12.0 }),
            to_page: Some(PageRef::Pos(2)),
        },
        Op::DeleteAnnotations {
            annots: vec![AnnotRef::Id(AnnotId(7)), AnnotRef::New(Local(8))],
        },
        Op::AdoptAnnotations {
            annots: vec![AnnotRef::Nm {
                nm: "acro-1".into(),
                page: Some(PageRef::Id(PageId(3))),
            }],
        },
        Op::SetOcrText {
            page: PageRef::Pos(1),
            layer: layer(),
        },
        Op::ClearOcrText {
            pages: PageSel::Positions(pos("1-7000")),
        },
        Op::SetWatermark {
            spec: WatermarkSpec {
                content: WatermarkContent::Text {
                    text: "Café DRAFT".into(),
                    font: FontChoice::TimesBoldItalic,
                    size: 72.0,
                    color: [200, 30, 30],
                    opacity: 0.3,
                    angle: 45.0,
                },
                position: GridPos::Center,
            },
            z: Z::Over,
            scope: ScopeSel::All,
        },
        Op::SetWatermark {
            spec: WatermarkSpec {
                content: WatermarkContent::Image {
                    image: Some(BlobSel::With("seal".into())),
                    scale: 0.5,
                    opacity: 0.2,
                },
                position: GridPos::BottomRight,
            },
            z: Z::Under,
            scope: ScopeSel::Pages(PageSel::Positions(pos("2-4"))),
        },
        Op::SetWatermark {
            spec: WatermarkSpec {
                content: WatermarkContent::Page {
                    pdf: Some(BlobSel::Asset(3)),
                    page: 2,
                    scale: 1.0,
                    opacity: 1.0,
                },
                position: GridPos::TopLeft,
            },
            z: Z::Under,
            scope: ScopeSel::All,
        },
        Op::RemoveWatermark {
            z: None,
            pages: PageSel::All,
            include_authored: true,
        },
        Op::SetHeaderFooter {
            spec: HeaderFooterSpec {
                texts: [
                    "<<file>>".into(),
                    String::new(),
                    "<<date>>".into(),
                    String::new(),
                    "Page <<1>> of <<N>>".into(),
                    "<<bates>>".into(),
                ],
                font: FontChoice::Helvetica,
                size: 10.0,
                color: [20, 20, 20],
                margins: Margins {
                    top: 36.0,
                    bottom: 36.0,
                    left: 72.0,
                    right: 72.0,
                },
                first_number: 1,
                bates: Some(BatesSpec {
                    prefix: "ACME".into(),
                    ..BatesSpec::default()
                }),
            },
            scope: ScopeSel::All,
        },
        Op::SetHeaderFooter {
            spec: HeaderFooterSpec {
                texts: [
                    "<<label>>".into(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
                font: FontChoice::TimesRoman,
                size: 9.0,
                color: [0, 0, 0],
                margins: Margins {
                    top: 18.0,
                    bottom: 18.0,
                    left: 36.0,
                    right: 36.0,
                },
                first_number: 5,
                bates: None,
            },
            scope: ScopeSel::Pages(PageSel::Ids(vec![PageRef::Pos(2)])),
        },
        Op::RemoveHeaderFooter {
            pages: PageSel::All,
            include_authored: true,
        },
    ];
    ops.extend(kinds().into_iter().map(|kind| Op::AddMark {
        page: PageRef::Id(PageId(1)),
        mark: NewMark {
            kind,
            style: Style {
                fill: None,
                dashed: false,
                cloudy: false,
                ..style()
            },
            contents: String::new(),
            author: None,
            turn: Rotation::R0,
            asset: None,
        },
        reply_to: Some(AnnotRef::New(Local(6))),
        local: None,
        imported: None,
    }));
    ops
}
