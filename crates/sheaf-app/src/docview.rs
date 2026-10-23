//! One open document: continuous vertical scroll of pages at a shared zoom,
//! rendered as tiles so only what's on screen is rasterized, with a cheap
//! low-res preview under the tiles so something appears immediately. Also
//! owns the per-document sidebar content (thumbnails, outline), since those
//! share its views and caches.
//!
//! The page sequence comes from `sheaf_doc::Document` (see
//! docs/document-model.md): an order of pages, each showing a page of one
//! of its source files. A page drawn renders from its view (M10.5 §10):
//! its source's engine while it's untouched, or else its group's overlay,
//! on the process's one render worker ([`engines`]). Images are cached by
//! what a page draws, its render key, so reordering pages, a save, or an
//! edit of another page costs nothing visually, and duplicates share their
//! images.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use futures::StreamExt;
use futures::channel::oneshot;
use gpui::{
    AnyElement, App, Context, DragMoveEvent, Entity, FocusHandle, Focusable, ObjectFit,
    RenderImage, ScrollHandle, ScrollWheelEvent, SharedString, Subscription, Task, Window, div,
    img, point, prelude::*, px, relative, rgb, rgba,
};
use sheaf_doc::export::{Export, ExportError, ExportFrom, Exported, Out};
use sheaf_doc::geom::{Affine, Geom};
use sheaf_doc::materialize;
use sheaf_doc::materialize::Came;
use sheaf_doc::prim::Key;
use sheaf_doc::realize::{EngineKey, PageView};
use sheaf_doc::{Annot, AnnotId, AnnotKind, Box2, DocState, FormRef, Pt};
use sheaf_doc::{OcgOverride, RenderKey, ViewInputs};
use sheaf_edit::{PageSize, Rotation};
use sheaf_ui::button::Button;
use sheaf_ui::list::ListLayout;
use sheaf_ui::text_area::{TextArea, TextAreaEvent};
use sheaf_ui::text_input::{TextInput, TextInputEvent};
use sheaf_ui::theme;

use crate::markup_tools::{Drag, Tool, arrow_head, stroke_canvas, stroke_canvas_dashed};
pub(crate) use crate::pane::Zoom;
use crate::pane::{Layout, LayoutSpec, MARGIN, PageMode, Pane, Shown, Spread};
use crate::{ClearGuides, FullScreen, GoToPage, ReadMode, ToggleGrid, ToggleRulers};
use sheaf_doc::tx::{CommitOpts, Grant, Rule, Tx, mark_kind, mark_style};
use sheaf_doc::{
    Actor, Cause, ChangeSet, Commit, Document, EditError, Identity, OutlineId, PageId, Src, Store,
};
use sheaf_edit::{Anchor, AnnotRef, BlobSel, MarkPatch, NewMark, Op, PageRef, PageSel, Turn};
use sheaf_image::raster::PixelFormat;
use sheaf_ops::text::{Found, Text};
use sheaf_ops::views::Views;
use sheaf_render::{Budget, EnginePool, Job, OpenError, Owner, Rendered};

use crate::comments_panel::ExportKind;
use crate::{
    CancelOcr, CloseSearch, CopySelection, CycleTextMarkup, DeletePage, DuplicatePages,
    ExportCleanCopy, ExportFlattened, ExtractPages, Find, FirstPage, InsertPdf, LastPage, LineDown,
    LineUp, NewBookmark, NextPage, NextPane, PasteMarks, PasteMarksInPlace, PrevPage, Redo,
    RemoveRecognizedText, RotatePageCcw, RotatePageCw, RotateViewCcw, RotateViewCw, Save, SaveAs,
    ScreenDown, ScreenUp, SearchNext, SearchPrev, SelectAllPages, SinglePage, SinglePageContinuous,
    SplitView, ToggleCheckmark, ToggleComments, ToggleCoverPage, ToolArrow, ToolConnector,
    ToolEllipse, ToolFreeText, ToolHighlight, ToolInk, ToolLine, ToolNote, ToolSelect, ToolSquare,
    ToolSquiggly, ToolStamp, ToolStrikeOut, ToolTextEdit, ToolTick, ToolUnderline, TwoUp,
    TwoUpContinuous, Undo, ZoomActual, ZoomFitPage, ZoomFitWidth, ZoomIn, ZoomOut,
};

pub const KEY_CONTEXT: &str = "DocView";

/// The notice when an edit, or something that writes the document out,
/// comes while the file is still opening (M10.5 proposal 28).
pub(crate) const STILL_OPENING: &str = "Still opening the file — try again in a moment";

const STATUS_H: f32 = 24.0;
/// Height of the divider between split panes, logical px.
const SPLITTER_H: f32 = 5.0;
/// Pages beyond the visible range to prefetch previews for.
const PREFETCH: usize = 1;
/// Pages beyond the visible range to keep cached before releasing textures.
const KEEP: usize = 3;
const LINE_STEP: f32 = 80.0;
/// Tile edge in device pixels. 1024² BGRA = 4 MB per tile.
const TILE: u32 = 1024;
/// Width of the low-res whole-page preview, device px. Also used for thumbnails.
const PREVIEW_W: u32 = 320;
/// Vertical overdraw for tile prefetch, as a fraction of the viewport.
const TILE_OVERDRAW: f32 = 0.5;
const ZOOM_STEP: f32 = 1.25;
const ZOOM_MIN: f32 = 0.1;
const ZOOM_MAX: f32 = 16.0;

/// Thumbnail panel geometry, logical px.
const THUMB_W: f32 = 150.0;
const THUMB_PAD: f32 = 10.0;
const THUMB_LABEL_H: f32 = 18.0;

pub(crate) enum State {
    Loading,
    Failed(String),
    /// Encrypted: waiting for a password.
    Locked(Locked),
    Ready(Box<Doc>),
}

pub(crate) struct Locked {
    input: Entity<TextInput>,
    _sub: Subscription,
    /// A password was tried and rejected.
    wrong: bool,
    focus_pending: bool,
}

/// An image by what it draws: a page's render key, and the job its
/// engine drew it by (M10.5 §10.2).
type TileKey = (RenderKey, Job);

pub(crate) struct Doc {
    pub(crate) model: Document,
    /// What renders each page (M10.5 §10): its view, from the document's
    /// realizer, drawn by the process's pool ([`engines`]).
    pub(crate) views: Views,
    /// The pages' text, read through `views` in the screen's budget (M10.5
    /// §10.5): what Find and Patterns search, and what text is selected,
    /// picked and marked up from.
    pub(crate) text: Text,
    /// What the screen draws besides the pages' own (M10.5 §10.4): the
    /// comments hidden, the one dragged and the layers turned off, as data
    /// ([`Doc::set_inputs`]). The panes, the thumbnails and the blank-page
    /// review draw with it.
    pub(crate) inputs: ViewInputs,
    /// Bumped on every change the document goes through
    /// ([`DocView::on_change`]), so what's made from it (the Comments
    /// panel's view, the History rows, the page labels) can tell.
    pub(crate) changes: u64,
    /// The last change, for the `--script` dump.
    pub(crate) last_change: Option<Arc<ChangeSet>>,
    /// The document as plugin calls read it, published again at every
    /// change ([`DocView::on_change`]) and when who the user is changes: a
    /// call makes its transaction on it at its first read or submit (M10.5
    /// §14.3). Each document has its own, so a call can tell when the view
    /// was reopened on another meanwhile.
    #[cfg(feature = "plugins")]
    pub(crate) plugin_cell: Arc<arc_swap::ArcSwap<sheaf_doc::Published>>,
    /// Bumped when where the pages go changed: their order or their
    /// boxes. The panes' layouts are built at one count, and again at the
    /// next.
    pub(crate) layout: u64,
    /// One view of the document, or two when the window is split.
    pub(crate) panes: Vec<Pane>,
    /// The pane keys, menus and the status bar act on.
    pub(crate) active: usize,
    /// Low-res whole-page renders, by the page's render key and the
    /// rotation drawn: the thumbnails and a rotated view of the same page
    /// need both. A page whose key a change leaves alone keeps its images,
    /// and a save changes no key.
    pub(crate) previews: HashMap<(RenderKey, Rotation), Arc<RenderImage>>,
    pub(crate) tiles: HashMap<TileKey, Arc<RenderImage>>,
    /// The key each page was last drawn by, at each rotation drawn: until
    /// an image of its new key lands, a page that changed shows these
    /// ([`Doc::image_key`]).
    pub(crate) drawn: HashMap<(PageId, Rotation), RenderKey>,
    /// Dropping the task drops the oneshot receiver, which tells the worker
    /// to skip the render if it hasn't started.
    pub(crate) inflight: HashMap<TileKey, Task<()>>,
    /// Images we've stopped showing; GPU textures are released in `render`,
    /// the only place we have a `Window`.
    pub(crate) retired: Vec<Arc<RenderImage>>,
    pub(crate) first_paint: Option<Instant>,
    /// Last edit notice, shown in the status bar.
    pub(crate) notice: Option<String>,
    /// How far the file's facts are read, until it's ready.
    pub(crate) reading: Option<sheaf_ops::Tracker>,
    // Sidebar state.
    pub(crate) thumb_scroll: ScrollHandle,
    /// Pages whose previews the thumbnail panel is showing, with their keys
    /// and the rotation drawn; kept cached, with what they were last drawn
    /// by.
    pub(crate) thumb_keep: HashSet<(PageId, RenderKey, Rotation)>,
    /// The engines the thumbnail panel's visible pages render from.
    pub(crate) strip_pins: Pins,
    /// Previews the blank-page review is showing, as the thumbnails'; kept
    /// cached.
    pub(crate) review_keep: HashSet<(PageId, RenderKey, Rotation)>,
    /// Last page the thumbnail panel auto-scrolled to follow.
    pub(crate) thumb_followed: Option<PageId>,
    pub(crate) outline_expanded: HashSet<OutlineId>,
    pub(crate) outline_scroll: ScrollHandle,
    // Selection and drag state for the thumbnail panel.
    pub(crate) selection: HashSet<PageId>,
    /// Index the last plain/ctrl click landed on; shift-click extends from it.
    pub(crate) sel_anchor: Option<usize>,
    /// Insert index under the cursor while pages are being dragged.
    pub(crate) drop_target: Option<usize>,
    /// A save is running on a background thread.
    pub(crate) saving: bool,
    // Outline panel editing state (see outline_panel.rs).
    pub(crate) outline_focus: FocusHandle,
    pub(crate) outline_selected: Option<OutlineId>,
    pub(crate) outline_drop: Option<crate::outline_panel::OutlineDrop>,
    pub(crate) outline_rename: Option<crate::outline_panel::Rename>,
    pub(crate) outline_menu: Option<(OutlineId, gpui::Point<gpui::Pixels>)>,
    /// Rows as last rendered: (id, depth, display index of target).
    pub(crate) outline_rows: Vec<(OutlineId, u16, Option<usize>)>,
    // Markup tool state (see markup_tools.rs).
    pub(crate) drag: Option<Drag>,
    /// The marks selected, in the order picked. The last is the one the
    /// handles and the text editor are on ([`Doc::selected_annot`]).
    pub(crate) selected_marks: Vec<(PageId, AnnotId)>,
    /// What the Comments panel shows (and so the page): filter and sort.
    pub(crate) comment_filter: crate::comments_panel::CommentFilter,
    /// View > Hide Comments.
    pub(crate) comments_hidden: bool,
    /// The panel's view, and what it was made from.
    pub(crate) comment_view: Option<(
        crate::comments_panel::ViewKey,
        Arc<crate::comments_panel::CommentView>,
    )>,
    /// Our marks the comment filter hides on the page.
    pub(crate) hidden_marks: HashSet<AnnotId>,
    /// Text selected with the Select tool (see text_select.rs).
    pub(crate) text_sel: Option<crate::text_select::TextSel>,
    pub(crate) text_drag: Option<crate::text_select::TextDrag>,
    /// Pages whose characters `text` is reading for the screen.
    pub(crate) chars_loading: HashSet<PageId>,
    pub(crate) annot_editor: Option<AnnotEditor>,
    /// A polygon, polyline or cloud being drawn (see shapes.rs).
    pub(crate) poly: Option<crate::shapes::PolyDraft>,
    /// The scale new measurements take (see `measure_tools`).
    pub(crate) measure_scale: sheaf_doc::measure::Scale,
    pub(crate) calibrate: crate::measure_tools::Calibrate,
    /// "Pages with" or "without" a group of marks (see groups.rs).
    pub(crate) group_list: Option<crate::groups::GroupList>,
    /// The right-click menu on a mark, where it was opened.
    pub(crate) mark_menu: Option<gpui::Point<gpui::Pixels>>,
    /// Another tool's comment selected, here or in the Comments panel.
    pub(crate) selected_foreign: Option<AnnotId>,
    /// Another tool's comment being dragged, made ours when it's let go:
    /// the screen leaves it out meanwhile ([`Doc::set_inputs`]).
    pub(crate) adopting: Option<AnnotId>,
    /// Files embedded in the primary file, and its portfolio schema if it
    /// is one (see attachments_panel.rs), once the file has opened.
    pub(crate) attachments: Vec<sheaf_doc::attachments::Attachment>,
    pub(crate) collection: Option<sheaf_doc::portfolio::Collection>,
    pub(crate) attachment_selected: Option<usize>,
    /// The forms in the document's files (see form_bar.rs).
    pub(crate) forms: crate::form_bar::Forms,
    /// Optional content groups and which are hidden (see layers_panel.rs).
    pub(crate) layers: Vec<sheaf_doc::layers::Layer>,
    pub(crate) layers_hidden: HashSet<sheaf_doc::layers::LayerId>,
    /// The Layers panel has chosen which show: the screen draws the
    /// primary with the hidden ones off from then on, its file's default
    /// configuration aside.
    pub(crate) layers_chosen: bool,
    /// Every page's label (from the model), and the change count it's for.
    pub(crate) labels: std::cell::RefCell<(u64, Option<std::sync::Arc<Vec<String>>>)>,
    /// With two panes: the top pane's share of the height.
    pub(crate) split: f32,
    /// The split divider is being dragged.
    pub(crate) split_drag: bool,
}

impl Doc {
    /// A document's view state as it opens: `model`, in one pane, nothing
    /// rendered or selected yet.
    pub(crate) fn new(
        model: Document,
        outline_focus: FocusHandle,
        measure_scale: sheaf_doc::measure::Scale,
    ) -> Doc {
        #[cfg(feature = "plugins")]
        let plugin_cell = Arc::new(arc_swap::ArcSwap::from_pointee(model.published()));
        let views = Views::new(engines());
        let text = Text::new(views.clone(), Budget::Screen);
        let strip_pins = Pins::new(views.pool());
        Doc {
            model,
            views,
            text,
            inputs: ViewInputs::screen(),
            changes: 0,
            last_change: None,
            #[cfg(feature = "plugins")]
            plugin_cell,
            layout: 0,
            panes: vec![Pane::new(Zoom::FitWidth, PageMode::default())],
            active: 0,
            previews: HashMap::new(),
            tiles: HashMap::new(),
            drawn: HashMap::new(),
            inflight: HashMap::new(),
            retired: Vec::new(),
            first_paint: None,
            notice: None,
            reading: None,
            thumb_scroll: ScrollHandle::new(),
            thumb_keep: HashSet::new(),
            strip_pins,
            review_keep: HashSet::new(),
            thumb_followed: None,
            outline_expanded: HashSet::new(),
            outline_scroll: ScrollHandle::new(),
            selection: HashSet::new(),
            sel_anchor: None,
            drop_target: None,
            saving: false,
            outline_focus,
            outline_selected: None,
            outline_drop: None,
            outline_rename: None,
            outline_menu: None,
            outline_rows: Vec::new(),
            drag: None,
            selected_marks: Vec::new(),
            comment_filter: Default::default(),
            comments_hidden: false,
            comment_view: None,
            hidden_marks: HashSet::new(),
            text_sel: None,
            text_drag: None,
            chars_loading: HashSet::new(),
            annot_editor: None,
            poly: None,
            measure_scale,
            calibrate: Default::default(),
            group_list: None,
            mark_menu: None,
            selected_foreign: None,
            adopting: None,
            labels: std::cell::RefCell::new((u64::MAX, None)),
            attachments: Vec::new(),
            collection: None,
            attachment_selected: None,
            forms: Default::default(),
            layers: Vec::new(),
            layers_hidden: HashSet::new(),
            layers_chosen: false,
            split: 0.5,
            split_drag: false,
        }
    }

    /// A save's file is written: its version is the saved one now, with
    /// the file's record, and the save is over: what it wrote, and the
    /// change, which is in what's saved alone. Nothing the document shows
    /// changes (M10.5 §9.1), so nothing rendered goes: every tile and
    /// preview stays.
    pub(crate) fn take_save(
        &mut self,
        written: materialize::Written,
    ) -> anyhow::Result<(materialize::SaveReport, ChangeSet)> {
        self.saving = false;
        let report = written.report();
        let version = written.record.version;
        match written.record_in(&mut self.model) {
            Ok(change) => Ok((report, change)),
            Err(e) => {
                self.model.abandon_save(version);
                Err(e)
            }
        }
    }

    /// The mark picked last, if any: the one with handles.
    pub(crate) fn selected_annot(&self) -> Option<(PageId, AnnotId)> {
        self.selected_marks.last().copied()
    }

    /// Select just this mark, or nothing.
    pub(crate) fn select_mark(&mut self, mark: Option<(PageId, AnnotId)>) {
        self.selected_marks = mark.into_iter().collect();
    }

    /// Shift+click: add the mark to the selection, or take it out.
    pub(crate) fn toggle_mark(&mut self, mark: (PageId, AnnotId)) {
        match self.selected_marks.iter().position(|m| *m == mark) {
            Some(i) => {
                self.selected_marks.remove(i);
            }
            None => self.selected_marks.push(mark),
        }
    }

    /// The selected marks themselves, with their pages.
    pub(crate) fn selected_mark_list(&self) -> Vec<(PageId, sheaf_doc::Annot)> {
        let st = self.model.state();
        self.selected_marks
            .iter()
            .filter_map(|(p, id)| {
                let a = st.find_annot(*id).filter(|(on, _)| on == p)?.1.clone();
                Some((*p, a))
            })
            .collect()
    }

    /// The page at position `index`.
    pub(crate) fn page_id(&self, index: usize) -> Option<PageId> {
        self.model.page(index)
    }

    pub(crate) fn pane(&self) -> &Pane {
        &self.panes[self.active]
    }

    pub(crate) fn pane_mut(&mut self) -> &mut Pane {
        &mut self.panes[self.active]
    }

    /// The active pane's current page.
    pub(crate) fn current_page(&self) -> usize {
        self.pane().current_page
    }

    /// Page `index` as the active pane shows it (with rotate view): for
    /// mapping between page and screen, never for edits.
    pub(crate) fn shown_ref(&self, index: usize) -> Option<Shown> {
        let st = self.model.state();
        self.pane().shown(st, st.page_at(index)?)
    }

    /// Page `index` as the active pane draws it (M10.5 §5): its boxes,
    /// turned by its rotation and the pane's rotate view. Its `to_view`
    /// takes PageSpace to the page as drawn, in points from its top-left
    /// (times the zoom's px per point), and `shown` is the size drawn.
    pub(crate) fn pane_geom(&self, index: usize) -> Option<Geom> {
        self.shown_ref(index).map(|p| p.geom)
    }

    /// Page `index`'s edit frame: the page as it shows without rotate
    /// view, in points from its top-left, y down, which is where gestures
    /// build and edit marks. Its `from_view` takes them to PageSpace.
    pub(crate) fn edit_geom(&self, index: usize) -> Option<Geom> {
        self.page_id(index).map(|p| self.model.geom(p))
    }

    /// Bring `b`, a box in PageSpace on page `index`, into the active
    /// pane's view.
    pub(crate) fn reveal(&mut self, index: usize, b: Box2) {
        let Some(geom) = self.pane_geom(index) else {
            return;
        };
        let r = geom.to_view().apply_box(b);
        let shown = geom.shown();
        self.scroll_to(ScrollTo::Reveal {
            page: index,
            fx: (r.x + r.w / 2.0) / shown.width.max(1.0),
            fy: (r.y + r.h / 2.0) / shown.height.max(1.0),
        });
    }

    /// Scroll the active pane.
    pub(crate) fn scroll_to(&mut self, to: ScrollTo) {
        self.pane_mut().pending_scroll = Some(to);
    }

    /// Catch up with `ch`, a change the document went through: what's
    /// selected or read of what it removed goes, the screen's inputs follow
    /// the comments it has now, and the panes lay the pages out again when
    /// they go elsewhere: their order or their boxes changed. Nothing else
    /// is laid out again: a mark's edit isn't, nor a page shown from
    /// another of its size. Images follow the pages' render keys (M10.5
    /// §10.2), so none goes here. Whether the page selection changed.
    pub(crate) fn follow(&mut self, ch: &ChangeSet) -> bool {
        self.changes += 1;
        let selection = self.prune(ch);
        self.set_inputs();
        if ch.order || !ch.geometry.is_empty() {
            self.layout += 1;
            for p in &mut self.panes {
                p.layout = None;
            }
        }
        selection
    }

    /// What's selected or read of the pages and annotations `ch` removed
    /// goes; nothing is looked at when it removed no page and changed no
    /// annotation. Whether the page selection changed.
    fn prune(&mut self, ch: &ChangeSet) -> bool {
        if !ch.annots.is_empty() {
            let st = self.model.state();
            (self.selected_marks)
                .retain(|(pid, aid)| st.find_annot(*aid).is_some_and(|(on, _)| on == *pid));
            if self.selected_foreign.is_some_and(|id| !st.has_annot(id)) {
                self.selected_foreign = None;
            }
        }
        if ch.pages_removed.is_empty() {
            return false;
        }
        if (self.text_sel.as_ref()).is_some_and(|s| ch.pages_removed.contains(&s.page)) {
            self.text_sel = None;
        }
        let before = self.selection.len();
        for p in &ch.pages_removed {
            self.selection.remove(p);
        }
        self.selection.len() != before
    }

    /// The screen's inputs made again from what they follow (M10.5
    /// §10.4): the comments the Comments panel's filter hides, or every
    /// other tool's with View > Hide Comments, by id, so a comment hides on
    /// its page alone: comments only, never a link, nor one its file hides
    /// itself; the comment being dragged, whose ghost GPUI draws in its
    /// place; and once the Layers panel has changed them, the primary's
    /// layers it turns off. sheaf's marks GPUI draws, and hides, itself.
    pub(crate) fn set_inputs(&mut self) {
        let st = self.model.state();
        let comment = |id: &AnnotId| {
            let f = st.annot(*id).and_then(|a| a.foreign());
            f.is_some_and(|f| !f.summary.is_link() && !f.summary.hidden)
        };
        let hidden: im::HashSet<AnnotId> = match self.comments_hidden {
            true => st.annots().map(|(id, _)| id).filter(comment).collect(),
            false => (self.comment_view.iter())
                .flat_map(|(_, view)| &view.hidden_foreign)
                .copied()
                .filter(comment)
                .collect(),
        };
        let ocg_off = self.layers_chosen.then(|| {
            let off = self.layers_hidden.iter().copied().collect();
            let primary = self.model.primary().id();
            Arc::new(OcgOverride {
                off: [(primary, off)].into(),
            })
        });
        let dragging: im::HashSet<AnnotId> = self.adopting.into_iter().collect();
        // The sets as they were when they haven't changed, so the realizer
        // knows the inputs by their pointers and digests them once.
        let was = &self.inputs;
        let kept = |set: im::HashSet<AnnotId>, was: &Arc<im::HashSet<AnnotId>>| match set == **was {
            true => was.clone(),
            false => Arc::new(set),
        };
        self.inputs = ViewInputs {
            hidden: kept(hidden, &was.hidden),
            dragging: kept(dragging, &was.dragging),
            ocg_off: match (ocg_off, &was.ocg_off) {
                (Some(off), Some(was)) if off == *was => Some(was.clone()),
                (off, _) => off,
            },
            ..ViewInputs::screen()
        };
    }

    /// The key page `id`'s images at `rotation` are found by, `key` being
    /// what it draws now: `key`, once it has one there, which makes it the
    /// key the page was last drawn by; until then that one's, so a page
    /// that changed keeps its old image until its new one lands.
    pub(crate) fn image_key(
        &mut self,
        id: PageId,
        key: RenderKey,
        rotation: Rotation,
    ) -> RenderKey {
        let drawn = self.previews.contains_key(&(key, rotation))
            || (self.tiles.keys()).any(|(k, job)| *k == key && job.rotation() == rotation);
        if drawn {
            self.drawn.insert((id, rotation), key);
            return key;
        }
        self.drawn.get(&(id, rotation)).copied().unwrap_or(key)
    }

    /// The pool's render of `want`, in the screen's budget: from its page's
    /// realization, as the view found it and under the screen's inputs, as
    /// they were when it was found. None when the page has gone.
    pub(crate) fn request(
        &self,
        want: Want,
    ) -> Option<oneshot::Receiver<anyhow::Result<Rendered>>> {
        let (st, format) = (self.model.state(), PixelFormat::Bgra);
        let (view, job, priority) = (&want.view, want.job, want.priority);
        (self.views)
            .draw(
                st,
                want.id,
                view,
                &self.inputs,
                job,
                format,
                priority,
                Budget::Screen,
            )
            .ok()
    }
}

/// The process's engine pool (M10.5 §10.3): every document's pages render
/// on its one worker, started with the first document.
pub(crate) fn engines() -> EnginePool<EngineKey> {
    static POOL: OnceLock<EnginePool<EngineKey>> = OnceLock::new();
    let pool = POOL.get_or_init(|| EnginePool::new().expect("the render thread starts"));
    pool.clone()
}

/// What one surface showing a document (a pane, the thumbnail strip)
/// pins in the pool: the engines its visible pages render from, which no
/// budget closes (M10.5 §10.3). Dropped with its surface, it unpins them.
pub(crate) struct Pins {
    pool: EnginePool<EngineKey>,
    owner: Owner,
    engines: Vec<EngineKey>,
}

impl Pins {
    pub(crate) fn new(pool: &EnginePool<EngineKey>) -> Pins {
        Pins {
            pool: pool.clone(),
            owner: pool.owner(),
            engines: Vec::new(),
        }
    }

    /// Pins `engines`, each once, in place of what it pinned before,
    /// telling the pool only when they differ.
    pub(crate) fn set(&mut self, engines: impl IntoIterator<Item = EngineKey>) {
        let mut pinned = Vec::new();
        for e in engines {
            if !pinned.contains(&e) {
                pinned.push(e);
            }
        }
        if pinned != self.engines {
            self.pool.pin(self.owner, pinned.clone());
            self.engines = pinned;
        }
    }
}

impl Drop for Pins {
    fn drop(&mut self) {
        self.set([]);
    }
}

/// A render a frame wants: page `id`'s, as `view` found it drawn, by
/// `job` as its engine draws the page (the view's turn and the pane's in
/// it), at `priority`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Want {
    pub(crate) id: PageId,
    pub(crate) view: PageView,
    pub(crate) job: Job,
    pub(crate) priority: u8,
}

#[derive(Clone, Copy)]
pub(crate) enum ScrollTo {
    /// The top of the row (page or spread) page `n` is in.
    Page(usize),
    By(f32),
    /// The end of the document.
    End,
    /// The top or bottom of what's laid out (in a non-continuous mode, the
    /// shown spread).
    Top,
    Bottom,
    /// In a non-continuous mode: the next spread's top, or the previous
    /// spread's bottom (scrolling on past an end).
    NextRow,
    PrevRowEnd,
    /// Bring a page-relative point (fractions) into view if it isn't already,
    /// landing it a third of the way down the viewport.
    Reveal {
        page: usize,
        fx: f32,
        fy: f32,
    },
    /// Put a page point (fractions of the page as shown) at the top-left of
    /// the viewport: a bookmark's position.
    Point {
        page: usize,
        fx: f32,
        fy: f32,
    },
    /// Keep the document point (`page`, `fx`, `fy` as fractions of the page)
    /// under viewport-local point (`vx`, `vy`). Used to anchor zooms.
    Anchor {
        page: usize,
        fx: f32,
        fy: f32,
        vx: f32,
        vy: f32,
    },
}

/// Inline editor for an annotation's text.
pub(crate) struct AnnotEditor {
    pub page: PageId,
    pub id: AnnotId,
    pub input: Entity<TextArea>,
    pub _sub: Subscription,
    pub _blur: Subscription,
}

/// What happened to a document, for listeners outside the view (plugins,
/// see `todos/plugins.md`).
#[derive(Debug, Clone, PartialEq)]
pub enum DocEvent {
    /// The file has opened: what it says besides its pages is read into
    /// the opened version, and edits are taken from now on.
    Opened,
    /// The document went through a change: a commit, an amend, an undo, a
    /// redo, a jump, a save, a rollback or a revert, one event each
    /// ([`DocView::on_change`]). Who made it is the change's actor.
    Changed { change: Arc<ChangeSet> },
    /// The file a save wrote (after the save's [`Changed`](Self::Changed)).
    Saved { path: PathBuf },
    /// The set of selected pages changed.
    SelectionChanged,
    /// The selected region (Shift+drag) changed: live while dragging.
    Region {
        page: PageId,
        /// PageSpace.
        rect: Box2,
    },
    /// The region was cleared.
    RegionCleared,
    /// The document closed. Emitted by the workspace just before it drops the view.
    Closed,
}

impl gpui::EventEmitter<DocEvent> for DocView {}

pub struct DocView {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) path: PathBuf,
    pub(crate) timing: bool,
    pub(crate) launched: Instant,
    pub(crate) state: State,
    pub(crate) search: Option<Search>,
    pub(crate) pattern: Option<crate::pattern_panel::PatternSearch>,
    pub(crate) initial_pattern: Option<String>,
    pub(crate) calc: Option<crate::calc_panel::CalcTape>,
    /// Focus to give the calculator's entry next frame (a note finished).
    pub(crate) pending_calc_focus: Option<Entity<sheaf_ui::text_input::TextInput>>,
    pub(crate) comments: Option<crate::comments_panel::Comments>,
    pub(crate) history: Option<crate::history_panel::History>,
    /// The icon the Attach File tool gives.
    pub(crate) attach_icon: String,
    /// The Connector tool's label; None for the next unused one.
    pub(crate) connector_label: Option<String>,
    pub(crate) stamps: Option<crate::stamps_panel::StampsPanel>,
    /// The stamp the Stamp tool places.
    pub(crate) stamp_choice: Option<sheaf_ops::stamps::StampRef>,
    pub(crate) pending_history_focus: bool,
    /// A Recognize Text run in progress (see ocr_run.rs).
    pub(crate) ocr: Option<crate::ocr_run::OcrRun>,
    /// A background job in progress: Export Pages, Split (see jobs.rs).
    pub(crate) job: Option<crate::jobs::Job>,
    /// The planners in progress, oldest first: inserts, Trim Whites,
    /// watermarks' and headers and footers' removals, comment imports, the
    /// blank-page search (see jobs.rs).
    pub(crate) planners: Vec<std::rc::Rc<crate::jobs::Job>>,
    /// Blank pages found, waiting for review (see blank_review.rs).
    pub(crate) blank_review: Option<crate::blank_review::Review>,
    /// A plugin command running on this document, with its progress
    /// (see plugins.rs).
    #[cfg_attr(not(feature = "plugins"), allow(dead_code))]
    pub(crate) plugin_run: Option<(String, sheaf_ops::Tracker)>,
    pub(crate) pending_comments_focus: bool,
    /// The Measure tools' scale bar, and whether to focus it next frame.
    pub(crate) measure_bar: Option<crate::measure_tools::MeasureBar>,
    pub(crate) pending_measure_focus: bool,
    pub(crate) initial_query: Option<String>,
    pub(crate) search_started: Option<Instant>,
    pub(crate) selftest_save: Option<PathBuf>,
    /// Re-focus the document on the next frame (after a text field closes).
    pub(crate) pending_focus: bool,
    /// Re-focus the Bookmarks panel on its next render (after a rename field closes).
    pub(crate) pending_outline_focus: bool,
    pub(crate) selftest_insert: Option<PathBuf>,
    pub(crate) selftest_annot: Option<PathBuf>,
    pub(crate) selftest_clean: Option<PathBuf>,
    pub(crate) selftest_extract: Option<PathBuf>,
    pub(crate) selftest_flatten: Option<PathBuf>,
    /// Dev: OCR every page, then save to this path (see ocr_run.rs).
    pub(crate) selftest_ocr: Option<PathBuf>,
    /// Password the file needs, once known.
    password: Option<String>,
    /// Open the inline editor on this mark next frame (a text edit just
    /// placed; dev hooks).
    pub(crate) pending_edit: Option<(PageId, AnnotId)>,
    /// Recovery data found when opening, until the user has been asked.
    pending_recovery: Option<sheaf_doc::recovery::Recovery>,
    /// A save stopped to ask the reader something (M10.5 §9.2), its
    /// version still sealed, until they've answered: whether to replace a
    /// file another program changed, to break a signed file's signatures,
    /// or to drop an XFA part that holds values; or an export, which asks
    /// the last two (§12.1).
    pub(crate) pending_ask: Option<(Asking, materialize::Ask)>,
    autosave: Option<Task<()>>,
    /// When the edits since the last sidecar write began.
    autosave_since: Option<Instant>,
    /// The version the last sidecar written holds, so autosave writes
    /// nothing new for the same: a version's state never changes (an amend
    /// makes a new one). None once a save changed the file it's written
    /// against.
    sidecar_of: Option<sheaf_doc::VersionId>,
    /// A sidecar is being written: the next waits for it.
    sidecar_writing: bool,
    pub(crate) tool: Tool,
    pub(crate) tick_glyph: String,
    /// The Select tool is over text (the pointer is an I-beam).
    pub(crate) text_hover: bool,
    /// Where the pointer last was over a page: page, point (in PageSpace),
    /// and how near counts as on text.
    pub(crate) last_hover: Option<(PageId, sheaf_doc::Pt, f32)>,
    /// Shift+drag region (see region.rs).
    pub(crate) region: Option<crate::region::Region>,
    /// Full Screen / Read Mode (see presentation.rs).
    pub(crate) presentation: crate::presentation::Presentation,
    pub(crate) saved_view: Option<crate::presentation::SavedView>,
    /// A dock panel this document asks the workspace to show (by panel id),
    /// e.g. Attachments when a portfolio opens.
    pub(crate) wanted_panel: Option<&'static str>,
    /// View > Go to Page, while open.
    pub(crate) goto: Option<crate::goto::GoTo>,
    /// View > Rulers / Grid, and the guides dragged out of the rulers.
    pub(crate) rulers: bool,
    pub(crate) grid: bool,
    pub(crate) guides: Vec<crate::rulers::Guide>,
    pub(crate) guide_drag: Option<crate::rulers::GuideDrag>,
    /// Rows of plugin panels for this document, by `<plugin>.<panel>`.
    #[cfg(feature = "plugins")]
    pub(crate) plugin_panels: HashMap<String, crate::plugin_panel::PluginPanelState>,
    /// Plugin calls given while the file opens, until it's ready.
    #[cfg(feature = "plugins")]
    pub(crate) plugin_calls: crate::plugins::Waiting,
}

/// A save between its seal and its record (M10.5 §9.1): the version it
/// writes, sealed ([`Document::save_input`]), where it writes, what the
/// plan decided, what the reader has answered of what it asks, and when
/// it began.
pub(crate) struct Saving {
    input: Arc<materialize::SaveInput>,
    target: materialize::SaveTarget,
    plan: materialize::SavePlan,
    answers: materialize::AskAnswers,
    started: Instant,
}

/// An export the reader is asked about before it writes (M10.5 §12.1):
/// how it's answered so far, and what runs it again with the answer.
pub(crate) struct Exporting {
    pub(crate) answers: materialize::AskAnswers,
    pub(crate) again: Again,
}

/// What runs an export again with the reader's answers.
pub(crate) type Again =
    Box<dyn FnOnce(&mut DocView, materialize::AskAnswers, &mut Context<DocView>)>;

/// What a write asks the reader about: a save's, or an export's.
pub(crate) enum Asking {
    Save(Saving),
    Export(Exporting),
}

/// A search match, addressed by page id so it survives reordering.
#[derive(Debug, Clone)]
pub(crate) struct Hit {
    pub page: PageId,
    /// The page's position in the document searched.
    pub order: usize,
    /// In PageSpace.
    pub rects: Vec<Box2>,
    pub text: String,
}

impl Hit {
    /// The matches a search found on `page`, at `position`.
    pub(crate) fn on(
        position: usize,
        page: PageId,
        found: Vec<sheaf_doc::TextHit>,
    ) -> impl Iterator<Item = Hit> {
        found.into_iter().map(move |h| Hit {
            page,
            order: position,
            rects: h.rects,
            text: h.text,
        })
    }
}

/// An open search bar and the running / finished query behind it.
pub(crate) struct Search {
    input: Entity<TextInput>,
    _input_sub: Subscription,
    _events_sub: Subscription,
    /// Query the current results are for.
    query: String,
    /// Hits in document order.
    hits: Vec<Hit>,
    current: Option<usize>,
    /// The positions searched so far.
    scanned: usize,
    done: bool,
    /// Dropping this cancels the worker-side search.
    _task: Option<Task<()>>,
}

impl DocView {
    pub fn new(
        path: PathBuf,
        password: Option<String>,
        timing: bool,
        launched: Instant,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::unopened(path, password, timing, launched, cx);
        this.open(cx);
        this
    }

    /// A view of the file at `path`, not opened yet: [`Self::new`] opens
    /// it, and a test gives it a document of its own.
    pub(crate) fn unopened(
        path: PathBuf,
        password: Option<String>,
        timing: bool,
        launched: Instant,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            password,
            path,
            timing,
            launched,
            state: State::Loading,
            search: None,
            pattern: None,
            initial_pattern: None,
            calc: None,
            pending_calc_focus: None,
            comments: None,
            ocr: None,
            job: None,
            planners: Vec::new(),
            blank_review: None,
            plugin_run: None,
            pending_comments_focus: false,
            measure_bar: None,
            pending_measure_focus: false,
            history: None,
            connector_label: None,
            attach_icon: "PushPin".into(),
            stamps: None,
            stamp_choice: None,
            pending_history_focus: false,
            initial_query: None,
            search_started: None,
            selftest_save: None,
            pending_focus: false,
            pending_outline_focus: false,
            selftest_insert: None,
            selftest_annot: None,
            selftest_clean: None,
            selftest_extract: None,
            selftest_flatten: None,
            selftest_ocr: None,
            pending_recovery: None,
            pending_ask: None,
            pending_edit: None,
            autosave: None,
            autosave_since: None,
            sidecar_of: None,
            sidecar_writing: false,
            tool: Tool::Select,
            tick_glyph: "✓".into(),
            text_hover: false,
            last_hover: None,
            region: None,
            presentation: Default::default(),
            saved_view: None,
            goto: None,
            wanted_panel: None,
            rulers: false,
            grid: false,
            guides: Vec::new(),
            guide_drag: None,
            #[cfg(feature = "plugins")]
            plugin_panels: HashMap::new(),
            #[cfg(feature = "plugins")]
            plugin_calls: Default::default(),
        }
    }

    /// The file's name, without the dirty marker.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string()
    }

    pub fn title(&self) -> SharedString {
        let name = self
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?");
        let dirty = matches!(&self.state, State::Ready(d) if d.model.is_dirty());
        if dirty {
            format!("{name} •").into()
        } else {
            name.to_string().into()
        }
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let password = self.password.clone();
        self.forget_planners();
        self.state = State::Loading;
        let timing = self.timing;
        let launched = self.launched;
        cx.spawn(async move |this, cx| {
            let opened = cx
                .background_executor()
                .spawn(async move {
                    // A save a crash cut short is undone first, so the file
                    // is the one its recovery sidecar was written against.
                    let torn = sheaf_doc::serialize::append::undo_torn(&path)
                        .inspect_err(|e| {
                            log::warn!("undoing the last save to {}: {e}", path.display())
                        })
                        .ok()
                        .flatten();
                    let recovery = sheaf_doc::recovery::find(&path);
                    let store = Store::new();
                    // The bytes, read once, and the geometry walk: the
                    // layout's sizes, before the first paint.
                    let t = Instant::now();
                    let (primary, stamp) = store.read_file(&path, password.as_deref())?;
                    let walk = t.elapsed();
                    let name = sheaf_doc::store::file_name(&path);
                    let came = Came::Read {
                        path: path.as_path().into(),
                        stamp,
                    };
                    anyhow::Ok((store, primary, name, came, walk, recovery, torn))
                })
                .await;
            this.update(cx, |viewer, cx| {
                match opened {
                    Ok((store, primary, name, came, walk, recovery, torn)) => {
                        if timing {
                            eprintln!(
                                "[timing] open: {walk:?} source, {:?} since launch, {} pages",
                                launched.elapsed(),
                                primary.page_count()
                            );
                        }
                        viewer.pending_recovery = recovery;
                        viewer.install(store, primary, name, came, cx);
                        if let (Some(torn), State::Ready(doc)) = (torn, &mut viewer.state) {
                            doc.notice = Some(torn.to_string());
                        }
                    }
                    Err(e) if needs_password(&e) => viewer.lock(cx),
                    Err(e) => viewer.state = State::Failed(format!("{e:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Show the password prompt; Enter retries the open with what was typed.
    fn lock(&mut self, cx: &mut Context<Self>) {
        let wrong = self.password.take().is_some();
        let input = cx.new(|cx| TextInput::new("Password", cx).masked());
        let sub = cx.subscribe(&input, |this, input, ev: &TextInputEvent, cx| {
            if *ev == TextInputEvent::Confirm {
                let pw = input.read(cx).text().to_string();
                if !pw.is_empty() {
                    this.password = Some(pw);
                    this.open(cx);
                }
            }
        });
        self.state = State::Locked(Locked {
            input,
            _sub: sub,
            wrong,
            focus_pending: true,
        });
        cx.notify();
    }

    /// Show a file as it opens, a source in `store`, as the primary source
    /// under `name`, and read the rest of it on a background thread (see
    /// [`Self::finish_open`]); its engine opens on the render worker,
    /// asked its form ([`Self::ask_form_type`]). How it `came` says what
    /// the document knows of its file, for its record: the file as it was
    /// read, or a restored document's base with what its recovery sidecar
    /// holds that the file hasn't got.
    fn install(
        &mut self,
        store: Arc<Store>,
        primary: Src,
        name: Arc<str>,
        came: Came,
        cx: &mut Context<Self>,
    ) {
        let src = primary.clone();
        let recovered = matches!(came, Came::Restored { .. });
        let who = crate::prefs::get(cx).doc_identity();
        let model = opening_model(store, primary, name, Some(came), who);
        self.sidecar_of = None;
        let opened = self.path.clone();
        cx.defer(move |cx| crate::recent::add(&opened, cx));
        let measure_scale = (crate::state::get(cx).measure_scale.clone()).unwrap_or_default();
        let doc = Doc::new(model, cx.focus_handle(), measure_scale);
        self.state = State::Ready(Box::new(doc));
        self.ask_form_type(cx);
        self.finish_open(src, recovered, cx);
        if let Some(src) = self.initial_pattern.take() {
            self.run_initial_pattern(src, cx);
        }
        if let Some(q) = self.initial_query.take() {
            self.new_search(cx);
            self.start_search(q, cx);
        }
    }

    /// A view of `session`'s document, made and saved to `path`, its file,
    /// elsewhere (Combine's, M10.5 §14.1): shown as it is, nothing read
    /// again, its history where the session left it. Its marks are made by
    /// the user from now on, measured at their scale, and the file is
    /// among the recent ones. `notice` is what it was told as it was made.
    /// [`Self::adopted`] tells the listeners it opened, once they listen.
    pub(crate) fn adopt(
        path: PathBuf,
        session: sheaf_ops::session::Session,
        notice: Option<String>,
        timing: bool,
        launched: Instant,
        cx: &mut Context<Self>,
    ) -> Self {
        let who = crate::prefs::get(cx).doc_identity();
        let scale = (crate::state::get(cx).measure_scale.clone()).unwrap_or_default();
        let opened = path.clone();
        cx.defer(move |cx| crate::recent::add(&opened, cx));
        Self::adopting(path, session, notice, who, scale, timing, launched, cx)
    }

    /// [`Self::adopt`] with the user `who` and the measure `scale` given.
    #[allow(clippy::too_many_arguments)]
    fn adopting(
        path: PathBuf,
        session: sheaf_ops::session::Session,
        notice: Option<String>,
        who: Identity,
        scale: sheaf_doc::measure::Scale,
        timing: bool,
        launched: Instant,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::unopened(path, None, timing, launched, cx);
        let sheaf_ops::session::Session { mut doc, view, .. } = session;
        doc.set_identity(who);
        let drawn = doc.primary().page_count() > 0;
        this.state = State::Ready(Box::new(Doc::new(doc, cx.focus_handle(), scale)));
        this.take_view(view);
        if let (Some(notice), State::Ready(doc)) = (notice, &mut this.state) {
            doc.notice = Some(notice);
        }
        // A primary of no pages, made in memory, draws nothing.
        if drawn {
            this.ask_form_type(cx);
        }
        this
    }

    /// An adopted document ([`Self::adopt`]) is open: the change that
    /// says so, with every flag set, which listeners hear as
    /// [`DocEvent::Opened`].
    pub(crate) fn adopted(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let model = &doc.model;
        let v = model.current_version();
        let label = (model.timeline().into_iter())
            .find(|version| version.current)
            .map_or_else(|| model.name().clone(), |version| version.label.into());
        self.on_change(&ChangeSet::opened(v, v, label, model.state()), cx);
    }

    /// Ask the primary file's base engine what form the file has, which
    /// opens it on the render worker before the first page asks: the form
    /// bar's guess until the form is read. That the engine didn't open is
    /// the document's notice: its pages then draw nothing, unless PDFium
    /// refused the file, whose pages are then drawn, and their text read,
    /// from sheaf's copy (`sheaf_ops::views`).
    fn ask_form_type(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let primary = doc.model.primary().id();
        let target = doc.views.base(doc.model.primary(), Budget::Screen);
        let rx = doc.views.pool().form_type(target);
        let (timing, launched, asked) = (self.timing, self.launched, Instant::now());
        cx.spawn(async move |this, cx| {
            let Ok(answer) = rx.await else { return };
            this.update(cx, |view, cx| {
                let State::Ready(doc) = &mut view.state else {
                    return;
                };
                if doc.model.primary().id() != primary {
                    return;
                }
                match answer {
                    Ok(form_type) => {
                        if timing {
                            eprintln!(
                                "[timing] engine: open in {:?}, {:?} since launch",
                                asked.elapsed(),
                                launched.elapsed()
                            );
                        }
                        doc.forms.form_type = form_type;
                    }
                    Err(e) if OpenError::is_refused(&e) => {
                        doc.notice = Some(
                            "PDFium can't read this file as saved, so its pages are drawn from \
                             sheaf's copy"
                                .into(),
                        )
                    }
                    Err(e) => doc.notice = Some(format!("The pages can't be drawn: {e:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Ask whether to apply recovery data found next to the file.
    fn offer_recovery(
        &mut self,
        rec: sheaf_doc::recovery::Recovery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.file_name();
        let rx = sheaf_ui::prompts::ask(
            window,
            cx,
            gpui::PromptLevel::Warning,
            &format!("Recover unsaved changes to \"{name}\"?"),
            Some(
                "sheaf found edits that weren't saved when it last closed. Recovered changes stay unsaved until you save.",
            ),
            &[
                gpui::PromptButton::ok("Recover"),
                gpui::PromptButton::new("Discard"),
            ],
        );
        let path = self.path.clone();
        cx.spawn(async move |this, cx| match rx.await {
            Ok(0) => {
                this.update(cx, |v, cx| v.recover(rec, cx)).ok();
            }
            Ok(_) => sheaf_doc::recovery::discard(&path),
            Err(_) => {}
        })
        .detach();
    }

    /// Open the document `rec` recovers, from memory, its record the file's
    /// with the sidecar's sections pending: it stays unsaved until its first
    /// save writes them, which checks that the file is still the one the
    /// sidecar was written against (M10.5 §11.10).
    fn recover(&mut self, rec: sheaf_doc::recovery::Recovery, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let password = self.password.clone();
        self.forget_planners();
        self.state = State::Loading;
        let timing = self.timing;
        cx.spawn(async move |this, cx| {
            let t = Instant::now();
            let opened = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move {
                        let store = Store::new();
                        let (primary, name, came) =
                            sheaf_doc::recovery::restore(&store, &path, &rec, password.as_deref())?;
                        anyhow::Ok((store, primary, name, came))
                    }
                })
                .await;
            this.update(cx, |viewer, cx| {
                match opened {
                    Ok((store, primary, name, came)) => {
                        viewer.install(store, primary, name, came, cx);
                        if timing {
                            eprintln!("[timing] recovered unsaved changes in {:?}", t.elapsed());
                        }
                    }
                    Err(e) => {
                        // Unusable recovery data: drop it and open the file as is.
                        log::error!("recovery failed: {e:#}");
                        sheaf_doc::recovery::discard(&path);
                        viewer.open(cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ----- crash-recovery autosave ------------------------------------------

    /// Write the recovery sidecar later: a few seconds after the last
    /// change, and at least every half minute while changes keep coming
    /// (at once when the ones waiting began half a minute ago).
    fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        let first = *self.autosave_since.get_or_insert_with(Instant::now);
        let delay = if first.elapsed() >= std::time::Duration::from_secs(30) {
            std::time::Duration::ZERO
        } else {
            std::time::Duration::from_secs(3)
        };
        self.autosave = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |v, cx| v.write_recovery(cx)).ok();
        }));
    }

    /// Bring the recovery sidecar up to date: gone when nothing is unsaved;
    /// otherwise what the current version changed, written beside the
    /// document's file against that file as sheaf last read or wrote it
    /// (its record's), unless the last sidecar holds it already. It waits
    /// for a save in flight, whose file it would be written against, and
    /// for the sidecar being written. A document no file of this path holds
    /// has none. A sidecar is encrypted as its file is.
    fn write_recovery(&mut self, cx: &mut Context<Self>) {
        self.autosave = None;
        self.autosave_since = None;
        let path = self.path.clone();
        let State::Ready(doc) = &self.state else {
            return;
        };
        if !doc.model.is_dirty() {
            sheaf_doc::recovery::discard(&path);
            self.sidecar_of = None;
            return;
        }
        if doc.saving || self.sidecar_writing {
            self.schedule_autosave(cx);
            return;
        }
        let of = doc.model.current_version();
        if self.sidecar_of == Some(of) {
            return;
        }
        let Some((file, base)) = (doc.model.file()).filter(|(file, _)| &***file == path.as_path())
        else {
            return;
        };
        let (file, base) = (file.clone(), *base);
        let input = doc.model.recovery_input();
        let timing = self.timing;
        self.sidecar_writing = true;
        cx.spawn(async move |this, cx| {
            let written = cx
                .background_executor()
                .spawn({
                    let file = file.clone();
                    async move {
                        let t = Instant::now();
                        let written = sheaf_doc::recovery::write(
                            &input,
                            &file,
                            &base,
                            &sheaf_ops::NoProgress,
                        );
                        (written, t.elapsed())
                    }
                })
                .await;
            this.update(cx, |v, cx| {
                v.sidecar_writing = false;
                let (n, took) = match written {
                    (Ok(n), took) => (n, took),
                    (Err(e), _) => {
                        log::warn!("writing recovery sidecar: {e:#}");
                        return;
                    }
                };
                if timing {
                    eprintln!("[timing] recovery sidecar: {n} bytes in {took:?}");
                }
                let State::Ready(doc) = &v.state else {
                    return;
                };
                if doc.model.file().is_some_and(|(_, now)| *now == base) {
                    v.sidecar_of = Some(of);
                } else {
                    // A save replaced the file it's written against
                    // meanwhile: it's stale, and the next one is due.
                    sheaf_doc::recovery::discard(&file);
                    v.write_recovery(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// [`Self::write_recovery`] on this thread, at once: what a save does
    /// as it ends. A sidecar already being written was begun against the
    /// file the save replaced, and, stale when it's done, writes this one
    /// again after it, in the background.
    fn write_recovery_now(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let State::Ready(doc) = &self.state else {
            return;
        };
        let file = (doc.model.file()).filter(|(file, _)| &***file == path.as_path());
        let (Some((file, base)), false, true) = (file, self.sidecar_writing, doc.model.is_dirty())
        else {
            return self.write_recovery(cx);
        };
        self.autosave = None;
        self.autosave_since = None;
        let of = doc.model.current_version();
        let t = Instant::now();
        let input = doc.model.recovery_input();
        match sheaf_doc::recovery::write(&input, file, base, &sheaf_ops::NoProgress) {
            Ok(n) => {
                if self.timing {
                    eprintln!("[timing] recovery sidecar: {n} bytes in {:?}", t.elapsed());
                }
                self.sidecar_of = Some(of);
            }
            Err(e) => log::warn!("writing recovery sidecar: {e:#}"),
        }
    }

    /// The user chose to drop unsaved changes: the sidecar goes with them.
    pub fn discard_recovery(&self) {
        sheaf_doc::recovery::discard(&self.path);
    }

    /// Read what `src`, the primary file, says besides its pages on a
    /// background thread, and make that the opened version
    /// ([`Self::ready`]). Until then the view shows the pages alone, with
    /// how many have been read, and edits are refused.
    fn finish_open(&mut self, src: sheaf_doc::Src, recovered: bool, cx: &mut Context<Self>) {
        use futures::FutureExt;
        let tracker = sheaf_ops::Tracker::new();
        if let State::Ready(doc) = &mut self.state {
            doc.reading = Some(tracker.clone());
        }
        cx.spawn(async move |this, cx| {
            let t = Instant::now();
            let blob = src.id();
            let reading = cx
                .background_executor()
                .spawn(async move { sheaf_doc::open::finish(&src, &tracker) });
            let mut reading = Box::pin(reading.fuse());
            let facts = loop {
                let mut tick = cx
                    .background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .fuse();
                futures::select_biased! {
                    facts = reading => break facts,
                    _ = tick => {}
                }
                // The status bar's count of the pages read.
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            };
            this.update(cx, |view, cx| view.ready(blob, facts, t, recovered, cx))
                .ok();
        })
        .detach();
    }

    /// `facts`, read from the file `blob` since `started`, are in. If the
    /// view still shows that file as it opens (a recovery, a reopen or a
    /// close can come between), they make its opened version, and the
    /// view takes what it shows beside the model; then listeners hear the
    /// file opened, and the dev hooks that edit or save run. A `recovered`
    /// file says it stays unsaved until it's saved.
    fn ready(
        &mut self,
        blob: sheaf_doc::BlobId,
        facts: sheaf_doc::open::Facts,
        started: Instant,
        recovered: bool,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let Some((view, opened)) = take_facts(&mut doc.model, blob, facts) else {
            return;
        };
        doc.reading = None;
        if doc.notice.as_deref() == Some(STILL_OPENING) {
            doc.notice = None;
        }
        let renamed = view.renamed.len();
        self.take_view(view);
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let st = doc.model.state();
        if self.timing {
            eprintln!(
                "[timing] ready: {} sheaf annotations, {} third-party, {} bookmarks, {} fields \
                 in {:?}, {:?} since launch",
                st.marks().count(),
                st.annots().filter(|(_, a)| a.foreign().is_some()).count(),
                st.bookmark_count(),
                st.terminals().count(),
                started.elapsed(),
                self.launched.elapsed()
            );
        }
        if doc.notice.is_none() {
            doc.notice = renamed_notice(renamed);
        }
        if recovered {
            doc.notice = Some("Recovered unsaved changes — Ctrl+S to keep them".into());
        }
        self.on_change(&opened, cx);
        if let Some(p) = self.selftest_save.take() {
            self.selftest_save(p, cx);
        }
        if let Some(p) = self.selftest_insert.take() {
            self.insert_pdf(p, cx);
        }
        if let Some(p) = self.selftest_annot.take() {
            self.selftest_annot(p, cx);
        }
        if let Some(p) = self.selftest_clean.take() {
            self.selftest_clean(p, cx);
        }
        if let Some(p) = self.selftest_extract.take() {
            let ids: Vec<PageId> = (0..2).filter_map(|i| self.doc_page_id(i)).collect();
            self.extract_to(ids, p, cx);
        }
        if let Some(p) = self.selftest_flatten.take() {
            self.selftest_flatten(p, cx);
        }
        if let Some(p) = self.selftest_ocr.take() {
            self.selftest_ocr(p, cx);
        }
        cx.notify();
    }

    /// What the view shows beside the document, as its primary file
    /// opened (`view`): its attachments, a portfolio's schema (whose panel
    /// it then wants shown), its layers and what its form says, a damaged
    /// one in the notice; and the outline's top level expanded where the
    /// file leaves it open.
    fn take_view(&mut self, view: sheaf_doc::open::ViewFacts) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        doc.attachments = view.attachments;
        doc.collection = view.collection;
        doc.set_layers(view.layers);
        if let Some(notice) = crate::form_bar::damaged_notice(&view.form) {
            doc.notice = Some(notice);
        }
        doc.forms.primary = Some(Arc::new(view.form));
        if doc.collection.is_some() {
            doc.open_portfolio();
            self.wanted_panel = Some("attachments");
        }
        let st = doc.model.state();
        doc.outline_expanded = (st.outline_roots().iter())
            .filter(|&&id| st.bookmark(id).is_some_and(|b| b.open))
            .copied()
            .collect();
    }

    pub fn go_to_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.scroll(ScrollTo::Page(page), cx);
    }

    /// Whether the file is still opening: being read, or shown while its
    /// facts are read. Edits, and what writes the document out, wait.
    pub(crate) fn is_opening(&self) -> bool {
        match &self.state {
            State::Loading => true,
            State::Ready(doc) => !doc.model.is_ready(),
            State::Locked(_) | State::Failed(_) => false,
        }
    }

    /// Whether something that reads the whole document (a save, an export,
    /// a print, a dialog over it) has to wait for the file to open, saying
    /// so if it does: keys run it even while the menus have it greyed out.
    pub(crate) fn still_opening(&mut self, cx: &mut Context<Self>) -> bool {
        let State::Ready(doc) = &mut self.state else {
            return false;
        };
        if doc.model.is_ready() {
            return false;
        }
        doc.notice = Some(STILL_OPENING.into());
        cx.notify();
        true
    }

    // ----- edits ------------------------------------------------------------

    /// A transaction of the user's on the current version: None while no
    /// document is open.
    pub(crate) fn user_tx(&self) -> Option<Tx> {
        match &self.state {
            State::Ready(doc) => Some(doc.model.tx(Actor::User)),
            _ => None,
        }
    }

    /// `ops` lowered in a transaction of the user's on the current version,
    /// for [`commit`](Self::commit) to make one version of, so one undo
    /// reverts them all: None while no document is open, or when one fails,
    /// which the notice says. Each sees what the ones before it did.
    pub(crate) fn lower(
        &mut self,
        ops: Vec<Op>,
        label: &str,
        cx: &mut Context<Self>,
    ) -> Option<Tx> {
        let tx = self.user_tx()?;
        self.lower_in(tx, ops, label, cx)
    }

    /// `ops` lowered in `tx`, as [`lower`](Self::lower) does: `tx`, or None
    /// when one fails, which the notice says.
    pub(crate) fn lower_in(
        &mut self,
        mut tx: Tx,
        ops: Vec<Op>,
        label: &str,
        cx: &mut Context<Self>,
    ) -> Option<Tx> {
        match tx.apply_batch(ops, &sheaf_ops::NoProgress) {
            Ok(_) => Some(tx),
            Err(e) => {
                self.set_notice(failed(label, &e), cx);
                None
            }
        }
    }

    /// `tx` committed as `opts` says: the way the view changes the
    /// document, but for OCR's flushes, which tell the reader in their
    /// run's own words (`flush_ocr`). The reader is told what it did, or
    /// why not, which the caller gets back too; the bookmarks it made come
    /// into view; and everything that follows the document catches up
    /// ([`on_change`](Self::on_change)).
    pub(crate) fn commit(
        &mut self,
        tx: Tx,
        opts: CommitOpts,
        cx: &mut Context<Self>,
    ) -> Result<Commit, EditError> {
        let State::Ready(doc) = &mut self.state else {
            return Err(EditError::NotReady);
        };
        let bookmarks = doc.model.state().bookmark_count();
        let what = opts.label.clone();
        let commit = match doc.model.commit(tx, opts) {
            Ok(commit) => commit,
            Err(e) => {
                doc.notice = Some(failed(what.as_deref().unwrap_or("Edit"), &e));
                cx.notify();
                return Err(e);
            }
        };
        let st = doc.model.state();
        let made = commit.created.bookmarks();
        reveal_bookmarks(st, made, &commit.change.keys, &mut doc.outline_expanded);
        let gone = bookmarks.saturating_sub(st.bookmark_count());
        doc.notice = Some(commit_notice(&commit, gone));
        self.on_change(&commit.change, cx);
        Ok(commit)
    }

    /// The one refresh: everything that follows the document catches up
    /// with `ch`, one change it went through (a commit, an amend, an undo,
    /// a redo, a jump, a save, a rollback, a revert, or the file's facts
    /// coming in). What's selected of what it removed goes, the panes lay
    /// the pages out again only when they go elsewhere, what's made from
    /// the document (the Comments panel's view, the History rows) is made
    /// again ([`Doc::follow`]), plugin calls read it as it is now, the
    /// recovery sidecar is written again later, and listeners hear of it
    /// once: as [`DocEvent::Opened`] when the file opened, and as
    /// [`DocEvent::Changed`] otherwise.
    pub(crate) fn on_change(&mut self, ch: &ChangeSet, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let selection = doc.follow(ch);
        #[cfg(feature = "plugins")]
        doc.plugin_cell.store(Arc::new(doc.model.published()));
        let change = Arc::new(ch.clone());
        doc.last_change = Some(change.clone());
        if selection {
            cx.emit(DocEvent::SelectionChanged);
        }
        self.prune_region(&ch.pages_removed, cx);
        // A save leaves nothing unsaved to write, and opening nothing new:
        // a sidecar found at open waits for the reader's answer.
        if !matches!(ch.cause, Cause::Saved | Cause::Opened) {
            self.schedule_autosave(cx);
        }
        cx.emit(match ch.cause {
            Cause::Opened => DocEvent::Opened,
            _ => DocEvent::Changed { change },
        });
        cx.notify();
    }

    /// The plugin calls to make now the file has opened: `opened` to each
    /// of `subscribers`, then the calls that waited for it.
    #[cfg(feature = "plugins")]
    pub(crate) fn release_plugin_calls(
        &mut self,
        subscribers: Vec<String>,
    ) -> Vec<(String, sheaf_plugin::Call)> {
        let State::Ready(doc) = &self.state else {
            return Vec::new();
        };
        self.plugin_calls.release(&doc.model, subscribers)
    }

    /// What a plugin command sees of this document; `ocr` is the OCR
    /// choices it runs with, and `committer` where what it commits as it
    /// runs lands.
    #[cfg(feature = "plugins")]
    pub(crate) fn plugin_context(
        &self,
        ocr: sheaf_ocr::OcrConfig,
        committer: Arc<dyn sheaf_plugin::Committer>,
    ) -> Option<sheaf_plugin::DocContext> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        let selection = doc
            .model
            .state()
            .order()
            .iter()
            .filter(|p| doc.selection.contains(p))
            .copied()
            .collect();
        Some(sheaf_plugin::DocContext {
            path: self.path.clone(),
            cell: doc.plugin_cell.clone(),
            selection,
            current_page: doc.page_id(doc.current_page()),
            text: crate::plugins::text_source(&doc.views),
            ocr: Some(crate::plugins::ocr_source(&doc.views, ocr)),
            render: Some(crate::plugins::render_source(&doc.views)),
            committer: Some(committer),
        })
    }

    /// Pages an operation applies to: the selection, or the current page.
    pub(crate) fn target_ids(&self) -> Vec<PageId> {
        let State::Ready(doc) = &self.state else {
            return Vec::new();
        };
        if doc.selection.is_empty() {
            return doc.page_id(doc.current_page()).into_iter().collect();
        }
        // Keep document order so the label and any insertion are predictable.
        doc.model
            .state()
            .order()
            .iter()
            .filter(|p| doc.selection.contains(p))
            .copied()
            .collect()
    }

    fn label(&self, verb: &str, n: usize) -> String {
        if n == 1 {
            format!("{verb} page")
        } else {
            format!("{verb} {n} pages")
        }
    }

    fn rotate_current(&mut self, by: Turn, cx: &mut Context<Self>) {
        let ids = self.target_ids();
        if !ids.is_empty() {
            let label = self.label("Rotate", ids.len());
            let pages = PageSel::ids(ids);
            if let Some(tx) = self.lower(vec![Op::RotatePages { pages, by }], &label, cx) {
                self.commit(tx, CommitOpts::labelled(label), cx).ok();
            }
        }
    }

    fn delete_current(&mut self, cx: &mut Context<Self>) {
        let ids = self.target_ids();
        if !ids.is_empty() {
            let label = self.label("Delete", ids.len());
            let pages = PageSel::ids(ids);
            if let Some(tx) = self.lower(vec![Op::DeletePages { pages }], &label, cx) {
                self.commit(tx, CommitOpts::labelled(label), cx).ok();
            }
        }
    }

    fn duplicate_current(&mut self, cx: &mut Context<Self>) {
        let ids = self.target_ids();
        if !ids.is_empty() {
            let label = self.label("Duplicate", ids.len());
            let pages = PageSel::ids(ids);
            let locals = Vec::new();
            if let Some(tx) = self.lower(vec![Op::DuplicatePages { pages, locals }], &label, cx) {
                self.commit(tx, CommitOpts::labelled(label), cx).ok();
            }
        }
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        doc.selection = doc.model.state().order().iter().copied().collect();
        cx.emit(DocEvent::SelectionChanged);
        cx.notify();
    }

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        if !doc.selection.is_empty() {
            doc.selection.clear();
            cx.emit(DocEvent::SelectionChanged);
            cx.notify();
        }
    }

    /// Thumbnail click: plain selects just that page, Ctrl toggles, Shift
    /// extends from the anchor. All of them navigate there.
    fn select_click(&mut self, index: usize, modifiers: gpui::Modifiers, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let refs: Vec<PageId> = doc.model.state().order().iter().copied().collect();
        let Some(&id) = refs.get(index) else { return };
        if modifiers.shift {
            let anchor = doc
                .sel_anchor
                .unwrap_or(doc.current_page())
                .min(refs.len() - 1);
            let (a, b) = if anchor <= index {
                (anchor, index)
            } else {
                (index, anchor)
            };
            if !modifiers.control {
                doc.selection.clear();
            }
            doc.selection.extend(refs[a..=b].iter().copied());
        } else if modifiers.control {
            if !doc.selection.remove(&id) {
                doc.selection.insert(id);
            }
            doc.sel_anchor = Some(index);
        } else {
            doc.selection.clear();
            doc.selection.insert(id);
            doc.sel_anchor = Some(index);
        }
        cx.emit(DocEvent::SelectionChanged);
        self.go_to_page(index, cx);
    }

    /// Finish a thumbnail drag: move `ids` to the recorded insert index.
    fn drop_pages(&mut self, ids: Vec<PageId>, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let Some(at) = doc.drop_target.take() else {
            return;
        };
        let moving: HashSet<PageId> = ids.iter().copied().collect();
        // "Before" must be a page that isn't moving; walk forward past any that are.
        let before = doc
            .model
            .state()
            .order()
            .iter()
            .skip(at)
            .find(|p| !moving.contains(p))
            .copied();
        let n = ids.len();
        let label = self.label("Move", n);
        let (pages, before) = (PageSel::ids(ids), before.map(PageRef::Id));
        if let Some(tx) = self.lower(vec![Op::MovePages { pages, before }], &label, cx) {
            self.commit(tx, CommitOpts::labelled(label), cx).ok();
        }
        if let State::Ready(doc) = &mut self.state {
            doc.selection = moving;
            cx.emit(DocEvent::SelectionChanged);
        }
    }

    /// Sign the versions and marks made from now on as `settings.toml`
    /// says who the user is.
    pub(crate) fn sync_author(&mut self, cx: &mut Context<Self>) {
        if let State::Ready(doc) = &mut self.state {
            doc.model.set_identity(crate::prefs::get(cx).doc_identity());
            // What a plugin's call lowers with knows who the user is.
            #[cfg(feature = "plugins")]
            doc.plugin_cell.store(Arc::new(doc.model.published()));
        }
    }

    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let change = match doc.model.undo() {
            Ok(Some(change)) => change,
            Ok(None) => {
                doc.notice = Some("Nothing to undo".into());
                cx.notify();
                return;
            }
            Err(e) => {
                doc.notice = Some(e.to_string());
                self.refresh_history();
                cx.notify();
                return;
            }
        };
        doc.notice = Some(format!("Undid: {}", change.label));
        self.on_change(&change, cx);
    }

    pub(crate) fn redo(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let change = match doc.model.redo() {
            Ok(Some(change)) => change,
            Ok(None) => {
                doc.notice = Some("Nothing to redo".into());
                cx.notify();
                return;
            }
            Err(e) => {
                doc.notice = Some(e.to_string());
                self.refresh_history();
                cx.notify();
                return;
            }
        };
        doc.notice = Some(format!("Redid: {}", change.label));
        self.on_change(&change, cx);
    }

    // ----- save --------------------------------------------------------------

    /// Save As: ask for a path, then save there.
    fn save_as(&mut self, cx: &mut Context<Self>) {
        let dir = self
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let suggested = self
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .map(String::from);
        let rx = crate::file_dialog::save(cx, &dir, suggested.as_deref());
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = rx.await {
                this.update(cx, |view, cx| view.save_to(path, cx).detach())
                    .ok();
            }
        })
        .detach();
    }

    /// Unsaved changes, or a save still in flight.
    pub fn is_dirty(&self) -> bool {
        matches!(&self.state, State::Ready(d) if d.model.is_dirty() || d.saving)
    }

    /// Plain Save: to the document's own path.
    pub fn save(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        let path = self.path.clone();
        self.save_to(path, cx)
    }

    /// Save to `path`: the document's own path for plain Save, any other
    /// for Save As, which is a save too, its file the document's from then
    /// on (M10.5 §9.1). The current version is sealed while it's saved, so
    /// an edit made meanwhile, an OCR flush among them, makes a version
    /// after it, which stays unsaved. The plan is made in the background,
    /// what it asks is asked ([`Self::ask_before_writing`]), the file is
    /// written in the background, and the version is recorded as the saved
    /// one ([`Self::finish_save`]). Nothing the screen shows changes.
    /// Resolves to whether it saved: false when it stopped to ask.
    pub fn save_to(&mut self, path: PathBuf, cx: &mut Context<Self>) -> Task<bool> {
        if self.still_opening(cx) {
            return Task::ready(false);
        }
        let State::Ready(doc) = &mut self.state else {
            return Task::ready(false);
        };
        // A second save would write after the first's file: the reader
        // saves again once it's written.
        if doc.saving {
            doc.notice = Some("Already saving…".into());
            cx.notify();
            return Task::ready(false);
        }
        let input = match doc.model.save_input(doc.model.current_version()) {
            Ok(input) => Arc::new(input),
            Err(e) => {
                doc.notice = Some(format!("Save failed: {e:#}"));
                cx.notify();
                return Task::ready(false);
            }
        };
        doc.saving = true;
        doc.notice = Some(format!("Saving {}…", path.display()));
        let target = doc.model.save_target(&path);
        // Agreed to once, for this window's session.
        let answers = materialize::AskAnswers {
            drop_xfa_values: doc.forms.xfa_values_dropped,
            ..Default::default()
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let plan = cx
                .background_executor()
                .spawn({
                    let (input, target) = (input.clone(), target.clone());
                    async move { materialize::plan(&input, &target) }
                })
                .await;
            let saving = Saving {
                input,
                target,
                plan,
                answers,
                started,
            };
            match this.update(cx, |view, cx| view.go_on_saving(saving, cx)) {
                Ok(written) => written.await,
                Err(_) => false,
            }
        })
    }

    /// Ask what `saving`'s plan asks that the reader hasn't answered, next
    /// frame; once nothing is left to ask, write its file in the
    /// background. Resolves to whether it saved.
    fn go_on_saving(&mut self, saving: Saving, cx: &mut Context<Self>) -> Task<bool> {
        if let Some(ask) = materialize::unanswered(&saving.plan, &saving.answers) {
            if let State::Ready(doc) = &mut self.state {
                doc.notice = None;
            }
            self.pending_ask = Some((Asking::Save(saving), ask));
            cx.notify();
            return Task::ready(false);
        }
        cx.spawn(async move |this, cx| {
            let written = cx
                .background_executor()
                .spawn({
                    let input = saving.input.clone();
                    let (plan, target) = (saving.plan.clone(), saving.target.clone());
                    let answers = saving.answers;
                    async move {
                        materialize::write_file(
                            &input,
                            &plan,
                            target,
                            &answers,
                            &sheaf_ops::NoProgress,
                        )
                    }
                })
                .await;
            this.update(cx, |view, cx| view.finish_save(saving, written, cx))
                .unwrap_or(false)
        })
    }

    /// Ask the reader what `ask`, from a save's plan or its write, or from
    /// an export, needs, and go on writing as they choose; any other answer
    /// ends it, nothing written. A signed file's ask offers what the plan
    /// allows: an Append anyway, which keeps the signed revision as it was
    /// signed and what the save removes readable in it, and a Rewrite, which
    /// breaks every signature; an export, a new file, only the second.
    fn ask_before_writing(
        &mut self,
        mut asking: Asking,
        ask: materialize::Ask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui::PromptButton;
        use materialize::Ask;
        /// What a button of the ask does: the last, Cancel, ends the write.
        #[derive(Clone, Copy)]
        enum Choice {
            SaveAs,
            Overwrite,
            AppendAnyway,
            Rewrite,
            DropXfa,
        }
        let name = self.file_name();
        let exports = matches!(asking, Asking::Export(_));
        let (message, detail, choices) = match &ask {
            Ask::FileChanged { .. } => (
                format!("\"{name}\" changed on disk"),
                "Another program changed it since sheaf read it.".to_string(),
                vec![
                    (PromptButton::ok("Save As…"), Choice::SaveAs),
                    (PromptButton::new("Overwrite"), Choice::Overwrite),
                ],
            ),
            Ask::Signatures {
                impact,
                append_anyway,
                reason,
            } => {
                // Those it breaks or flags: one broken already isn't asked
                // about.
                let n = impact.iter().filter(|i| i.verdict.asks()).count();
                let s = if n == 1 { "" } else { "s" };
                let mut detail = match reason {
                    _ if exports => {
                        format!("The export is a new file, which breaks its signature{s}.")
                    }
                    Some(why) => {
                        format!(
                            "Saving rewrites the whole file ({why}), which breaks its signature{s}."
                        )
                    }
                    None if n == 1 => "Its signature doesn't allow these changes after it: \
                                       an update would show as breaking it."
                        .to_string(),
                    None => "Its signatures don't allow these changes after them: \
                             an update would show as breaking them."
                        .to_string(),
                };
                let mut choices = Vec::new();
                if let Some(anyway) = append_anyway {
                    detail.push_str(
                        " Append Anyway adds the changes after the signed revision, \
                         which stays as it was signed",
                    );
                    match anyway.stays_readable.as_slice() {
                        [] => detail.push('.'),
                        kept => {
                            let kept: Vec<String> = kept.iter().map(|r| r.to_string()).collect();
                            detail.push_str(&format!(
                                ", but what the save removes stays readable in it: {}.",
                                kept.join(", ")
                            ));
                        }
                    }
                    choices.push((PromptButton::new("Append Anyway"), Choice::AppendAnyway));
                }
                let verb = if exports { "Export" } else { "Rewrite" };
                choices.push((
                    PromptButton::new(format!("{verb} (breaks {n} signature{s})")),
                    Choice::Rewrite,
                ));
                (format!("\"{name}\" is signed"), detail, choices)
            }
            Ask::XfaHoldsValues { .. } => {
                let (what, button) = match exports {
                    true => ("The export leaves it out", "Export Without It"),
                    false => (
                        "Saving the form as it is now removes it",
                        "Save and Remove It",
                    ),
                };
                (
                    "Remove the form's XFA part?".to_string(),
                    format!(
                        "It holds values sheaf doesn't show, which Adobe Acrobat or Reader \
                         shows. {what}, and them."
                    ),
                    vec![(PromptButton::new(button), Choice::DropXfa)],
                )
            }
        };
        let offer = match &ask {
            Ask::Signatures { append_anyway, .. } => append_anyway.clone(),
            _ => None,
        };
        let mut buttons: Vec<PromptButton> = choices.iter().map(|(b, _)| b.clone()).collect();
        buttons.push(PromptButton::cancel("Cancel"));
        let rx = sheaf_ui::prompts::ask(
            window,
            cx,
            gpui::PromptLevel::Warning,
            &message,
            Some(&detail),
            &buttons,
        );
        cx.spawn(async move |this, cx| {
            let answer = rx.await.ok();
            this.update(cx, |view, cx| {
                let choice = answer.and_then(|i| choices.get(i)).map(|(_, c)| *c);
                let answers = match &mut asking {
                    Asking::Save(saving) => &mut saving.answers,
                    Asking::Export(exporting) => &mut exporting.answers,
                };
                match choice {
                    Some(Choice::Overwrite) => answers.overwrite = true,
                    Some(Choice::AppendAnyway) => {
                        if let Some(offer) = &offer {
                            answers.take(offer);
                        }
                    }
                    Some(Choice::Rewrite) => answers.break_signatures = true,
                    Some(Choice::DropXfa) => {
                        answers.drop_xfa_values = true;
                        if let (Asking::Save(_), State::Ready(doc)) = (&asking, &mut view.state) {
                            doc.forms.xfa_values_dropped = true;
                        }
                    }
                    Some(Choice::SaveAs) | None => {}
                }
                match (asking, choice) {
                    (Asking::Save(saving), Some(Choice::SaveAs)) => {
                        view.abandon_save(saving, cx);
                        view.save_as(cx);
                    }
                    (Asking::Save(saving), None) => view.abandon_save(saving, cx),
                    (Asking::Save(saving), _) => view.go_on_saving(saving, cx).detach(),
                    (Asking::Export(_), Some(Choice::SaveAs) | None) => cx.notify(),
                    (Asking::Export(exporting), _) => {
                        (exporting.again)(view, exporting.answers, cx)
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// `export`, written as a job (one at a time, its pages counted in the
    /// status line, Escape stopping it) as `answers` answer it, the notice
    /// saying how it went: `done`'s once it's written, else what it is
    /// (`what`, "Extract") and why it stopped. What it asks is asked next
    /// frame, and it's written again with the answer
    /// ([`Self::ask_before_writing`]).
    pub(crate) fn write_export(
        &mut self,
        export: Export,
        answers: materialize::AskAnswers,
        what: &'static str,
        done: impl FnOnce(&Exported) -> String + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(tracker) = self.start_job(what, cx) else {
            return;
        };
        let work = {
            let export = export.clone();
            move |p: &sheaf_ops::Tracker| Ok(sheaf_doc::export::run(export, &answers, p)?)
        };
        let status = move |s: &sheaf_ops::ProgressSnapshot| {
            Some(format!("{what}: page {} of {}…", s.done, s.total?))
        };
        let end =
            move |v: &mut DocView, result: anyhow::Result<Exported>, _: &mut Context<DocView>| {
                v.job = None;
                let e = match result {
                    Ok(made) => return Some(done(&made)),
                    Err(e) => e,
                };
                let Some(ExportError::Ask(ask)) = e.downcast_ref::<ExportError>() else {
                    return Some(crate::jobs::ended(what, &e));
                };
                let again = move |v: &mut DocView, answers, cx: &mut Context<DocView>| {
                    v.write_export(export, answers, what, done, cx)
                };
                let exporting = Exporting {
                    answers,
                    again: Box::new(again),
                };
                v.pending_ask = Some((Asking::Export(exporting), ask.clone()));
                None
            };
        self.drive(tracker, work, status, end, cx);
    }

    /// `saving` ends with nothing written: its version is unsealed.
    fn abandon_save(&mut self, saving: Saving, cx: &mut Context<Self>) {
        if let State::Ready(doc) = &mut self.state {
            doc.model.abandon_save(saving.input.version);
            doc.saving = false;
        }
        cx.notify();
    }

    /// `saving`'s write is done, `written` saying how. The file written,
    /// its version is the saved one, with the file's record; a stop asks
    /// the reader again (another program changed the file just before the
    /// rename) or ends the save. Then the recovery sidecar follows the new
    /// file, and what follows the document hears of the save
    /// ([`Self::on_change`]). Whether it saved.
    ///
    /// The screen keeps what it shows: the pages, their renders and what
    /// the engines hide are the document's, which a save doesn't change.
    fn finish_save(
        &mut self,
        saving: Saving,
        written: Result<materialize::Written, materialize::SaveStop>,
        cx: &mut Context<Self>,
    ) -> bool {
        let State::Ready(doc) = &mut self.state else {
            return false;
        };
        let written = match written {
            Ok(written) => written,
            Err(materialize::SaveStop::Ask(ask)) => {
                // Asked next frame.
                doc.notice = None;
                self.pending_ask = Some((Asking::Save(saving), ask));
                cx.notify();
                return false;
            }
            Err(stop) => {
                doc.notice = Some(match stop {
                    materialize::SaveStop::Cancelled(_) => "Save cancelled".into(),
                    stop => {
                        log::error!("save: {stop}");
                        format!("Save failed: {stop}")
                    }
                });
                self.abandon_save(saving, cx);
                return false;
            }
        };
        let (report, saved) = match doc.take_save(written) {
            Ok(taken) => taken,
            Err(e) => {
                doc.notice = Some(format!("Save failed: {e:#}"));
                cx.notify();
                return false;
            }
        };
        let path = report.path.to_path_buf();
        let took = saving.started.elapsed();
        let notes: String = (report.notes.iter()).map(|n| format!(" — {n}")).collect();
        doc.notice = Some(match report.strategy {
            // A Rewrite says why (M10.5 §7.3).
            Some(strategy) if !report.why.is_empty() => {
                let why: Vec<String> = report.why.iter().map(|w| w.to_string()).collect();
                format!(
                    "Saved {} ({strategy:?}: {}) in {took:?}{notes}",
                    path.display(),
                    why.join("; ")
                )
            }
            Some(strategy) => format!("Saved {} ({strategy:?}) in {took:?}{notes}", path.display()),
            // The file holds the document already: nothing was written.
            None => format!("Nothing to save{notes}"),
        });
        if self.timing {
            eprintln!(
                "[timing] saved {} ({} pages, {:?}) in {took:?}",
                path.display(),
                doc.model.page_count(),
                report.strategy,
            );
        }
        if path != self.path {
            // Save As: the file the document was is no longer its file, and
            // keeps no sidecar of it.
            sheaf_doc::recovery::discard(&self.path);
            self.path = path.clone();
        }
        let saved_path = path.clone();
        cx.defer(move |cx| crate::recent::add(&saved_path, cx));
        // Against the file just written: none while the version saved is
        // the current one, or else one at once, with the edits made since,
        // written before the save is done (M10.5 D39): the sidecar from
        // before is stale against the new file, and a crash before a
        // sidecar of it would lose those edits.
        self.sidecar_of = None;
        self.write_recovery_now(cx);
        self.on_change(&saved, cx);
        cx.emit(DocEvent::Saved { path });
        true
    }

    /// Dev hook: rotate the first page and save to `path`, so the save path
    /// can be exercised without a keyboard.
    pub fn selftest_save(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        if let Some(p) = doc.page_id(0) {
            let (pages, by, label) = (PageSel::ids([p]), Turn::Quarter, "Rotate page");
            if let Some(tx) = self.lower(vec![Op::RotatePages { pages, by }], label, cx) {
                self.commit(tx, CommitOpts::labelled(label), cx).ok();
            }
        }
        self.save_to(path, cx).detach();
    }

    pub fn set_selftest_save(&mut self, path: PathBuf) {
        self.selftest_save = Some(path);
    }

    // ----- markup -------------------------------------------------------------

    /// A new mark with the document's defaults, built in a page's edit
    /// frame: its turn is R0, so it reads as it was drawn there. Who made
    /// it and when are the host's to say as it's made (M10.5 §6.8).
    pub(crate) fn new_annot(&self, kind: AnnotKind, color: [u8; 3]) -> Annot {
        Annot {
            id: sheaf_doc::AnnotId(0),
            kind,
            color,
            opacity: 1.0,
            stroke: 1.5,
            author: String::new(),
            contents: String::new(),
            modified: String::new(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        }
    }

    /// Add `annot`, a mark built in page `page`'s edit frame, as one undo
    /// step called `label`; it lands in PageSpace through
    /// [`Annot::to_page`](sheaf_doc::Annot::to_page). The mark it made.
    pub(crate) fn add_annot(
        &mut self,
        page: PageId,
        annot: Annot,
        label: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnnotId> {
        let edit = self.edit_geom_of(page)?;
        let annot = annot.to_page(&edit);
        self.add_page_annot(page, annot, label, cx)
    }

    /// Add `annot`, a mark already in PageSpace (text markups, which the
    /// page's text places), as one undo step called `label`. The mark it
    /// made, which comes in selected, so its properties can be set
    /// straight away.
    pub(crate) fn add_page_annot(
        &mut self,
        page: PageId,
        annot: Annot,
        label: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnnotId> {
        self.add_mark(page, annot, None, label, cx)
    }

    /// Add `annot`, a reply or a status, in the thread of comment `to` on
    /// page `page`, as one undo step called `label`. The mark it made.
    pub(crate) fn add_reply(
        &mut self,
        page: PageId,
        annot: Annot,
        to: AnnotId,
        label: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnnotId> {
        self.add_mark(page, annot, Some(to), label, cx)
    }

    /// `annot`, in PageSpace, made on `page` in the thread of `reply_to`,
    /// and selected.
    fn add_mark(
        &mut self,
        page: PageId,
        annot: Annot,
        reply_to: Option<AnnotId>,
        label: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnnotId> {
        let mut tx = self.user_tx()?;
        let asset = match self.held_asset(&mut tx, &annot) {
            Ok(asset) => asset,
            Err(e) => {
                self.set_notice(failed(label, &e), cx);
                return None;
            }
        };
        let op = Op::AddMark {
            page: page.into(),
            mark: new_mark(&annot, asset),
            reply_to: reply_to.map(AnnotRef::Id),
            local: None,
            imported: None,
        };
        let tx = self.lower_in(tx, vec![op], label, cx)?;
        let commit = self.commit(tx, CommitOpts::labelled(label), cx).ok()?;
        let id = commit.created.annots().next()?;
        if let State::Ready(doc) = &mut self.state {
            doc.select_mark(Some((page, id)));
        }
        Some(id)
    }

    /// What mark `a` holds by its stem, its page stamp's page or its
    /// attached file, granted in `tx` for its op to name: as the document
    /// finds it (its store's, else a source's).
    pub(crate) fn held_asset(&self, tx: &mut Tx, a: &Annot) -> Result<Option<BlobSel>, EditError> {
        let State::Ready(doc) = &self.state else {
            return Err(EditError::NotReady);
        };
        let missing = |what: String| EditError::NotFound {
            op: "add-mark",
            what,
        };
        let grant = match &a.kind {
            AnnotKind::PageStamp { stamp, name, .. } => Grant::Stamp {
                stem: stamp.clone(),
                form: (doc.model.find_stamp(stamp))
                    .ok_or_else(|| missing(format!("the stamp {name}")))?,
            },
            AnnotKind::FileAttachment {
                file, name, size, ..
            } => Grant::Blob(
                (doc.model.find_file(file, *size))
                    .ok_or_else(|| missing(format!("the attached file {name}")))?,
            ),
            _ => return Ok(None),
        };
        Ok(Some(tx.grant(grant)))
    }

    /// Dev hook: drop one of each mark on page 1 and save to `path`.
    pub fn selftest_annot(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let Some(p0) = doc.page_id(0) else {
            return;
        };
        let mut hl = self.new_annot(
            AnnotKind::Highlight {
                rects: vec![Box2 {
                    x: 72.0,
                    y: 60.0,
                    w: 300.0,
                    h: 30.0,
                }],
            },
            [255, 230, 0],
        );
        hl.opacity = 0.6;
        self.add_annot(p0, hl, "Highlight", cx);
        let mut note = self.new_annot(
            AnnotKind::Note {
                pos: Pt { x: 400.0, y: 60.0 },
            },
            [255, 200, 0],
        );
        note.contents = "A sticky note".into();
        self.add_annot(p0, note, "Note", cx);
        let mut text = self.new_annot(
            AnnotKind::FreeText {
                rect: Box2 {
                    x: 72.0,
                    y: 120.0,
                    w: 240.0,
                    h: 48.0,
                },
                size: 12.0,
            },
            [200, 0, 0],
        );
        text.contents = "Free text written by sheaf, wrapped onto a second line.".into();
        self.add_annot(p0, text, "Text", cx);
        let sq = self.new_annot(
            AnnotKind::Square {
                rect: Box2 {
                    x: 350.0,
                    y: 120.0,
                    w: 120.0,
                    h: 48.0,
                },
            },
            [0, 120, 255],
        );
        self.add_annot(p0, sq, "Square", cx);
        let tick = self.new_annot(
            AnnotKind::Stamp {
                rect: Box2 {
                    x: 500.0,
                    y: 120.0,
                    w: 28.0,
                    h: 28.0,
                },
                glyph: "✓".into(),
            },
            [0, 150, 0],
        );
        self.add_annot(p0, tick, "Tickmark", cx);
        let tick2 = self.new_annot(
            AnnotKind::Stamp {
                rect: Box2 {
                    x: 540.0,
                    y: 120.0,
                    w: 28.0,
                    h: 28.0,
                },
                glyph: "F".into(),
            },
            [200, 0, 0],
        );
        self.add_annot(p0, tick2, "Tickmark", cx);
        let arrow = self.new_annot(
            AnnotKind::Line {
                a: Pt { x: 80.0, y: 220.0 },
                b: Pt { x: 300.0, y: 260.0 },
                arrow: true,
                arrow_start: false,
            },
            [0, 120, 255],
        );
        self.add_annot(p0, arrow, "Arrow", cx);
        let mut ink = self.new_annot(
            AnnotKind::Ink {
                paths: vec![
                    (0..40)
                        .map(|i| Pt {
                            x: 350.0 + i as f32 * 5.0,
                            y: 240.0 + (i as f32 * 0.5).sin() * 20.0,
                        })
                        .collect(),
                ],
            },
            [160, 60, 220],
        );
        ink.stroke = 2.5;
        self.add_annot(p0, ink, "Pencil", cx);
        let State::Ready(doc) = &self.state else {
            return;
        };
        let p1 = doc.page_id(1).unwrap_or(p0);
        let mk = |rect: Box2| {
            self.new_annot(
                AnnotKind::Connector {
                    rect,
                    label: "1".into(),
                    partner: None,
                },
                [220, 40, 40],
            )
        };
        let a = mk(Box2 {
            x: 500.0,
            y: 220.0,
            w: 22.0,
            h: 22.0,
        });
        let b = mk(Box2 {
            x: 100.0,
            y: 100.0,
            w: 22.0,
            h: 22.0,
        });
        let (Some(at_a), Some(at_b)) = (self.edit_geom_of(p0), self.edit_geom_of(p1)) else {
            return;
        };
        let (a, b) = (a.to_page(&at_a), b.to_page(&at_b));
        let (AnnotKind::Connector { rect, .. }, AnnotKind::Connector { rect: rect_b, .. }) =
            (&a.kind, &b.kind)
        else {
            return;
        };
        let pair = Op::AddConnector {
            a: Anchor {
                page: p0.into(),
                rect: *rect,
            },
            b: Anchor {
                page: p1.into(),
                rect: *rect_b,
            },
            label: "1".into(),
            style: mark_style(&a),
            locals: Vec::new(),
        };
        let label = "Connector 1";
        if let Some(tx) = self.lower(vec![pair], label, cx) {
            self.commit(tx, CommitOpts::labelled(label), cx).ok();
        }
        let mut tape = self.new_annot(
            AnnotKind::Tape {
                rect: Box2 {
                    x: 360.0,
                    y: 300.0,
                    w: 110.0,
                    h: 60.0,
                },
                size: 10.0,
                tape: Default::default(),
                look: Default::default(),
            },
            [20, 20, 20],
        );
        tape.contents = "1,234.50 +\n250.00 +\n84.50 -\n1,400.00 T".into();
        let tape = self.add_annot(p0, tape, "Place tape", cx);
        self.save_to(path, cx).detach();
        // Leave the multi-line editor open on the tape so it gets exercised.
        self.pending_edit = tape.map(|id| (p0, id));
    }

    pub fn set_selftest_annot(&mut self, path: PathBuf) {
        self.selftest_annot = Some(path);
    }

    pub fn set_selftest_clean(&mut self, path: PathBuf) {
        self.selftest_clean = Some(path);
    }

    /// Dev hook: drop a couple of marks, then export a clean copy to `path`
    /// (the document itself is left dirty, as the real command would).
    pub fn selftest_clean(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let Some(p0) = doc.page_id(0) else {
            return;
        };
        let mut note = self.new_annot(
            AnnotKind::Note {
                pos: Pt { x: 400.0, y: 60.0 },
            },
            [255, 200, 0],
        );
        note.contents = "should not survive".into();
        self.add_annot(p0, note, "Note", cx);
        let tick = self.new_annot(
            AnnotKind::Stamp {
                rect: Box2 {
                    x: 500.0,
                    y: 120.0,
                    w: 28.0,
                    h: 28.0,
                },
                glyph: "✓".into(),
            },
            [0, 150, 0],
        );
        self.add_annot(p0, tick, "Tickmark", cx);
        self.export_clean_to(path, cx);
    }

    /// Every page's id, in document order.
    #[cfg_attr(not(feature = "plugins"), allow(dead_code))]
    pub(crate) fn page_ids(&self) -> Vec<PageId> {
        match &self.state {
            State::Ready(doc) => doc.model.state().order().iter().copied().collect(),
            _ => Vec::new(),
        }
    }

    fn doc_page_id(&self, index: usize) -> Option<PageId> {
        match &self.state {
            State::Ready(doc) => doc.page_id(index),
            _ => None,
        }
    }

    /// Dev hook: a few marks, then a flattened export to `path`.
    pub fn selftest_flatten(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(p0) = self.doc_page_id(0) else {
            return;
        };
        let mut text = self.new_annot(
            AnnotKind::FreeText {
                rect: Box2 {
                    x: 72.0,
                    y: 200.0,
                    w: 240.0,
                    h: 40.0,
                },
                size: 12.0,
            },
            [200, 0, 0],
        );
        text.contents = "Flattened free text".into();
        self.add_annot(p0, text, "Text", cx);
        let sq = self.new_annot(
            AnnotKind::Square {
                rect: Box2 {
                    x: 350.0,
                    y: 200.0,
                    w: 120.0,
                    h: 48.0,
                },
            },
            [0, 120, 255],
        );
        self.add_annot(p0, sq, "Square", cx);
        let tick = self.new_annot(
            AnnotKind::Stamp {
                rect: Box2 {
                    x: 500.0,
                    y: 200.0,
                    w: 28.0,
                    h: 28.0,
                },
                glyph: "✓".into(),
            },
            [0, 150, 0],
        );
        self.add_annot(p0, tick, "Tickmark", cx);
        self.flattened_copy_to(path, cx);
    }

    // ----- extract pages -------------------------------------------------------

    /// Document > Extract Pages: the selection (or current page) to a new file.
    fn extract_pages_prompt(&mut self, cx: &mut Context<Self>) {
        let ids = self.target_ids();
        if ids.is_empty() {
            return;
        }
        let dir = self
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let suggested = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| format!("{s} - pages.pdf"));
        let rx = crate::file_dialog::save(cx, &dir, suggested.as_deref());
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = rx.await {
                this.update(cx, |view, cx| view.extract_to(ids, path, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub fn extract_to(&mut self, ids: Vec<PageId>, path: PathBuf, cx: &mut Context<Self>) {
        if self.still_opening(cx) {
            return;
        }
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        // The save dialog asked before it named a file there.
        let out = Out::File {
            path: path.clone(),
            overwrite: true,
        };
        let export = Export::extract(doc.model.export_from(Actor::User), &ids, out);
        cx.notify();
        let done = move |made: &Exported| {
            let n = made.pages;
            let s = if n == 1 { "" } else { "s" };
            format!("Extracted {n} page{s} to {}", path.display())
        };
        let answers = materialize::AskAnswers::default();
        self.write_export(export, answers, "Extract", done, cx);
    }

    // ----- export flattened copy ----------------------------------------------

    fn flattened_copy_prompt(&mut self, cx: &mut Context<Self>) {
        let dir = self
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let suggested = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| format!("{s} - flattened.pdf"));
        let rx = crate::file_dialog::save(cx, &dir, suggested.as_deref());
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = rx.await {
                this.update(cx, |view, cx| view.flattened_copy_to(path, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub fn flattened_copy_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.still_opening(cx) {
            return;
        }
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        // Flattening keeps an attachment's icon and drops its file, as
        // Acrobat does: say so.
        let files = (doc.model.state().marks())
            .filter(|(_, a)| matches!(a.kind, AnnotKind::FileAttachment { .. }))
            .count()
            + doc
                .attachments
                .iter()
                .filter(|a| matches!(a.at, sheaf_doc::attachments::AttachmentRef::Annot { .. }))
                .count();
        let out = Out::File {
            path: path.clone(),
            overwrite: true,
        };
        let export = Export::flattened(doc.model.export_from(Actor::User), out);
        let (timing, t) = (self.timing, Instant::now());
        cx.notify();
        let done = move |made: &Exported| {
            let n = made.flattened;
            if timing {
                eprintln!(
                    "[timing] exported flattened copy ({n} annotations) in {:?}",
                    t.elapsed()
                );
            }
            format!(
                "Exported flattened copy to {} ({n} annotation{}){}",
                path.display(),
                if n == 1 { "" } else { "s" },
                match files {
                    0 => String::new(),
                    1 => " — the attachment's icon is kept, its file isn't".into(),
                    _ => " — attachments' icons are kept, their files aren't".into(),
                }
            )
        };
        let answers = materialize::AskAnswers::default();
        self.write_export(export, answers, "Export", done, cx);
    }

    // ----- export clean copy ---------------------------------------------------

    /// File > Export Clean Copy: ask for a path, then write the document
    /// there with all markup removed.
    fn clean_copy_prompt(&mut self, cx: &mut Context<Self>) {
        let dir = self
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let suggested = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| format!("{s} - clean.pdf"));
        let rx = crate::file_dialog::save(cx, &dir, suggested.as_deref());
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(path))) = rx.await {
                this.update(cx, |view, cx| view.export_clean_to(path, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub fn export_clean_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.still_opening(cx) {
            return;
        }
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let out = Out::File {
            path: path.clone(),
            overwrite: true,
        };
        let export = Export::clean_copy(doc.model.export_from(Actor::User), out);
        let (timing, t) = (self.timing, Instant::now());
        cx.notify();
        let done = move |_: &Exported| {
            if timing {
                eprintln!(
                    "[timing] exported clean copy {} in {:?}",
                    path.display(),
                    t.elapsed()
                );
            }
            format!("Exported clean copy to {}", path.display())
        };
        let answers = materialize::AskAnswers::default();
        self.write_export(export, answers, "Export", done, cx);
    }

    /// Open the inline text field for annotation `id` on `page`.
    pub(crate) fn start_annot_editor_for(
        &mut self,
        page: PageId,
        id: AnnotId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let Some((_, annot)) = (doc.model.state().find_annot(id)).filter(|(on, _)| *on == page)
        else {
            return;
        };
        let annot = annot.clone();
        let hint = match annot.kind {
            AnnotKind::Replace { .. } => "Type the replacement; Ctrl+Enter or click away when done",
            AnnotKind::Insert { .. } => {
                "Type the text to insert; Ctrl+Enter or click away when done"
            }
            _ => "Type a note; Ctrl+Enter or click away when done",
        };
        let input = cx.new(|cx| {
            let mut i = TextArea::new(hint, cx).rows(3, 10);
            i.set_text(annot.contents.clone(), cx);
            i
        });
        // Enter, Escape and clicking elsewhere all keep the text, as in
        // Acrobat; there's no way to lose what was typed except Undo.
        let sub = cx.subscribe(&input, move |this, _, _: &TextAreaEvent, cx| {
            this.finish_annot_editor(cx);
        });
        let focus = input.read(cx).focus_handle(cx);
        let blur = cx.on_focus_out(&focus, window, move |this, _, _, cx| {
            this.finish_annot_editor(cx);
        });
        window.focus(&focus, cx);
        doc.annot_editor = Some(AnnotEditor {
            page,
            id,
            input,
            _sub: sub,
            _blur: blur,
        });
        doc.select_mark(Some((page, id)));
        cx.notify();
    }

    /// Close the inline editor, saving its text if it changed.
    fn finish_annot_editor(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let Some(ed) = doc.annot_editor.take() else {
            return;
        };
        let text = ed.input.read(cx).text().to_string();
        // An insertion with nothing to insert isn't one.
        if text.trim().is_empty()
            && let Some((_, cur)) = doc.model.state().find_annot(ed.id)
            && matches!(cur.kind, AnnotKind::Insert { .. })
        {
            let annots = vec![AnnotRef::Id(ed.id)];
            let label = "Delete text insertion";
            if let Some(tx) = self.lower(vec![Op::DeleteAnnotations { annots }], label, cx) {
                self.commit(tx, CommitOpts::labelled(label), cx).ok();
            }
            self.pending_focus = true;
            return;
        }
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        if let Some((_, cur)) = doc.model.state().find_annot(ed.id)
            && cur.contents != text
        {
            let patch = MarkPatch {
                contents: Some(text),
                ..MarkPatch::default()
            };
            let marks = vec![AnnotRef::Id(ed.id)];
            let op = Op::PatchMarks {
                marks,
                patch,
                imported: None,
            };
            if let Some(tx) = self.lower(vec![op], "Edit text", cx) {
                self.commit(tx, CommitOpts::labelled("Edit text"), cx).ok();
            }
        }
        self.pending_focus = true;
        cx.notify();
    }

    // ----- insert pdf ---------------------------------------------------------

    fn insert_pdf_prompt(&mut self, cx: &mut Context<Self>) {
        let rx = crate::file_dialog::open(
            cx,
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: Some("Insert".into()),
            },
        );
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                this.update(cx, |view, cx| {
                    for p in paths {
                        view.insert_pdf(p, cx);
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    /// Where inserted pages go by default: after the current page, or
    /// after the last selected page. The page they go before (None: the
    /// end).
    pub(crate) fn insert_point(&self) -> Option<PageId> {
        let targets = self.target_ids();
        let State::Ready(doc) = &self.state else {
            return None;
        };
        let last_target = targets.last().and_then(|id| doc.model.index_of(*id));
        last_target.and_then(|i| doc.page_id(i + 1))
    }

    /// Insert every page of `path` after the current page (or after the
    /// selection), merging its bookmarks PDFlyer-style.
    pub fn insert_pdf(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let before = self.insert_point();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();
        self.insert_file(path, before, name, true, cx);
    }

    pub fn set_selftest_insert(&mut self, path: PathBuf) {
        self.selftest_insert = Some(path);
    }

    // ----- search -----------------------------------------------------------

    fn new_search(&mut self, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("Find in document", cx));
        let sub = cx.observe(&input, |this, _, cx| this.on_search_input_changed(cx));
        let events = cx.subscribe(&input, |this, _, ev: &TextInputEvent, cx| match ev {
            TextInputEvent::Confirm => this.search_step(1, cx),
            TextInputEvent::Cancel => this.close_search_keep_focus(cx),
        });
        self.search = Some(Search {
            input,
            _input_sub: sub,
            _events_sub: events,
            query: String::new(),
            hits: Vec::new(),
            current: None,
            scanned: 0,
            done: true,
            _task: None,
        });
    }

    fn close_search_keep_focus(&mut self, cx: &mut Context<Self>) {
        self.search = None;
        self.pending_focus = true;
        cx.notify();
    }

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.is_none() {
            self.new_search(cx);
        }
        let input = self.search.as_ref().unwrap().input.clone();
        input.update(cx, |i, cx| i.select_all(cx));
        window.focus(&input.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// `--find`: run a query as soon as the document is ready, with the bar open.
    pub fn set_initial_query(&mut self, query: String, cx: &mut Context<Self>) {
        self.initial_query = Some(query);
        cx.notify();
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn on_search_input_changed(&mut self, cx: &mut Context<Self>) {
        let Some(search) = &self.search else { return };
        let query = search.input.read(cx).text().to_string();
        if query == search.query {
            return;
        }
        self.start_search(query, cx);
    }

    fn start_search(&mut self, query: String, cx: &mut Context<Self>) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let Some(search) = &mut self.search else {
            return;
        };
        search.query = query.clone();
        search.hits.clear();
        search.current = None;
        search.scanned = 0;
        search._task = None; // cancels the previous search
        if query.trim().is_empty() {
            search.done = true;
            cx.notify();
            return;
        }
        search.done = false;
        self.search_started = Some(Instant::now());
        let Some(search) = &mut self.search else {
            return;
        };
        let Ok(query) = sheaf_doc::Query::literal(&query, false) else {
            search.done = true;
            cx.notify();
            return;
        };
        let mut rx = doc.text.search(doc.model.state(), query, &doc.inputs);
        search._task = Some(cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.next().await {
                let stop = this
                    .update(cx, |view, cx| {
                        let Some(search) = &mut view.search else {
                            return true;
                        };
                        match ev {
                            Found::Hits {
                                position,
                                page,
                                hits,
                            } => {
                                let first = search.hits.is_empty();
                                search.hits.extend(Hit::on(position, page, hits));
                                if first && !search.hits.is_empty() {
                                    view.go_to_hit(0, cx);
                                }
                            }
                            Found::Scanned(n) => search.scanned = n,
                            Found::Done => {
                                search.done = true;
                                if view.timing
                                    && let Some(t) = view.search_started.take()
                                {
                                    eprintln!(
                                        "[timing] search {:?}: {} hits across {} pages in {:?}",
                                        search.query,
                                        search.hits.len(),
                                        search.scanned,
                                        t.elapsed()
                                    );
                                }
                            }
                        }
                        cx.notify();
                        false
                    })
                    .unwrap_or(true);
                if stop {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn go_to_hit(&mut self, ix: usize, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let Some(search) = &mut self.search else {
            return;
        };
        if search.hits.is_empty() {
            return;
        }
        let ix = ix % search.hits.len();
        search.current = Some(ix);
        let hit = &search.hits[ix];
        let Some(index) = doc.model.index_of(hit.page) else {
            return;
        };
        if let Some(r) = hit.rects.first() {
            doc.reveal(index, *r);
        }
        cx.notify();
    }

    fn search_step(&mut self, delta: i64, cx: &mut Context<Self>) {
        let Some(search) = &self.search else { return };
        let n = search.hits.len();
        if n == 0 {
            return;
        }
        let cur = search.current.unwrap_or(0) as i64;
        let next = (cur + delta).rem_euclid(n as i64) as usize;
        self.go_to_hit(next, cx);
    }

    fn render_search_bar(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let search = self.search.as_ref()?;
        let total = search.hits.len();
        let status = if search.query.trim().is_empty() {
            String::new()
        } else if total == 0 {
            if search.done {
                "No matches".to_string()
            } else {
                format!("Searching… {}", search.scanned)
            }
        } else {
            let cur = search.current.map(|c| c + 1).unwrap_or(0);
            if search.done {
                format!("{cur} of {total}")
            } else {
                format!("{cur} of {total}+ (searching…)")
            }
        };
        let btn = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px_2()
                .py_1()
                .rounded_sm()
                .text_sm()
                .cursor_pointer()
                .bg(theme::CONTROL)
                .hover(|s| s.bg(theme::CONTROL_HOVER))
                .child(label)
        };
        Some(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .bg(theme::PANEL)
                .border_b_1()
                .border_color(theme::DIVIDER)
                .child(div().w(px(320.0)).child(search.input.clone()))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::TEXT_MUTED)
                        .min_w(px(140.0))
                        .child(status),
                )
                .child(
                    btn("search-prev", "▲")
                        .on_click(cx.listener(|v, _, _, cx| v.search_step(-1, cx))),
                )
                .child(
                    btn("search-next", "▼")
                        .on_click(cx.listener(|v, _, _, cx| v.search_step(1, cx))),
                )
                .child(
                    btn("search-close", "×")
                        .on_click(cx.listener(|v, _, window, cx| v.close_search(window, cx))),
                )
                .into_any_element(),
        )
    }

    // ----- scrolling and zoom ------------------------------------------------

    pub(crate) fn scroll(&mut self, to: ScrollTo, cx: &mut Context<Self>) {
        if let State::Ready(doc) = &mut self.state {
            doc.scroll_to(to);
            cx.notify();
        }
    }

    /// Next/previous page, or spread in a two-up mode.
    fn turn_page(&mut self, delta: isize, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let n = doc.model.page_count();
        let mode = doc.pane().mode;
        let rows = mode.row_count(n);
        if rows == 0 {
            return;
        }
        let row = mode.row_of(doc.current_page()) as isize + delta;
        let row = row.clamp(0, rows as isize - 1) as usize;
        doc.scroll_to(ScrollTo::Page(mode.row_pages(row, n).start));
        cx.notify();
    }

    /// Window > Split: a second pane on the same document, or back to one
    /// (keeping the active pane's view).
    pub(crate) fn toggle_split(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        if doc.panes.len() > 1 {
            let keep = doc.panes.swap_remove(doc.active);
            doc.panes = vec![keep];
            doc.active = 0;
            doc.notice = Some("Split view off".into());
        } else {
            let second = doc.pane().split();
            doc.panes.push(second);
            doc.active = 1;
            doc.notice = Some(
                "Split view: each pane scrolls and zooms on its own; Ctrl+\\ again to close".into(),
            );
        }
        cx.notify();
    }

    /// Make pane `ix` the one keys, menus and the status bar act on.
    fn activate_pane(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let State::Ready(doc) = &mut self.state
            && ix < doc.panes.len()
            && doc.active != ix
        {
            doc.active = ix;
            cx.notify();
        }
    }

    /// F6-style: the other pane becomes active.
    fn cycle_pane(&mut self, cx: &mut Context<Self>) {
        if let State::Ready(doc) = &self.state {
            let next = (doc.active + 1) % doc.panes.len();
            self.activate_pane(next, cx);
        }
    }

    fn drag_split(&mut self, y: gpui::Pixels, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        if !doc.split_drag || doc.panes.len() < 2 {
            return;
        }
        let (a, b) = (doc.panes[0].scroll.bounds(), doc.panes[1].scroll.bounds());
        let top = f32::from(a.origin.y);
        let bottom = f32::from(b.origin.y + b.size.height);
        if bottom - top < 1.0 {
            return;
        }
        doc.split = ((f32::from(y) - top) / (bottom - top)).clamp(0.1, 0.9);
        cx.notify();
    }

    fn end_split_drag(&mut self, cx: &mut Context<Self>) {
        if let State::Ready(doc) = &mut self.state
            && std::mem::take(&mut doc.split_drag)
        {
            cx.notify();
        }
    }

    /// Two-Up or Two-Up Continuous, keeping the cover page setting.
    fn set_two_up(&mut self, continuous: bool, cx: &mut Context<Self>) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let spread = match doc.pane().mode.spread {
            Spread::TwoUpCover => Spread::TwoUpCover,
            _ => Spread::TwoUp,
        };
        self.set_page_mode(PageMode { spread, continuous }, cx);
    }

    /// Height of the active pane's viewport, for page-at-a-time scrolling.
    fn pane_view_h(&self) -> f32 {
        match &self.state {
            State::Ready(doc) => doc
                .pane()
                .layout
                .as_ref()
                .map(|l| l.view_h)
                .unwrap_or(600.0),
            _ => 600.0,
        }
    }

    /// Change zoom, keeping the document point under `anchor` (viewport-local
    /// logical px; defaults to the viewport centre) where it is.
    fn set_zoom(&mut self, zoom: Zoom, anchor: Option<(f32, f32)>, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let pane = doc.pane_mut();
        if pane.zoom == zoom {
            return;
        }
        pane.pending_scroll = pane.layout.as_ref().and_then(|layout| {
            let (vx, vy) = anchor.unwrap_or((layout.view_w / 2.0, layout.view_h / 2.0));
            let sx = -f32::from(pane.scroll.offset().x);
            let sy = -f32::from(pane.scroll.offset().y);
            let (dx, dy) = (sx + vx, sy + vy);
            let page = layout.page_at(dx, dy)?;
            let r = layout.rect(page)?;
            Some(ScrollTo::Anchor {
                page,
                fx: (dx - r.left) / r.w,
                fy: (dy - r.top) / r.h,
                vx,
                vy,
            })
        });
        pane.zoom = zoom;
        cx.notify();
    }

    fn zoom_by(&mut self, factor: f32, anchor: Option<(f32, f32)>, cx: &mut Context<Self>) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let current = doc
            .pane()
            .layout
            .as_ref()
            .map(|l| l.zoom_percent() / 100.0)
            .unwrap_or(1.0);
        let z = (current * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        self.set_zoom(Zoom::Custom(z), anchor, cx);
    }

    /// View > Page Display: the active pane's page mode. The page in view
    /// stays in view.
    pub(crate) fn set_page_mode(&mut self, mode: PageMode, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let page = doc.current_page();
        let pane = doc.pane_mut();
        if pane.mode == mode {
            return;
        }
        pane.mode = mode;
        pane.row = mode.row_of(page);
        pane.pending_scroll = Some(ScrollTo::Page(page));
        doc.notice = Some(format!("Page display: {}", mode.label()));
        cx.notify();
    }

    /// Show Cover Page in Two-Up: toggles between the two two-up spreads
    /// (switching to two-up from single page).
    fn toggle_cover_page(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let mut mode = doc.pane().mode;
        mode.spread = match mode.spread {
            Spread::TwoUpCover => Spread::TwoUp,
            Spread::TwoUp | Spread::Single => Spread::TwoUpCover,
        };
        self.set_page_mode(mode, cx);
    }

    /// Rotate View: turn the active pane's display, not the pages.
    fn rotate_view(&mut self, by: Rotation, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let page = doc.current_page();
        let pane = doc.pane_mut();
        pane.rotation = pane.rotation.plus(by);
        pane.pending_scroll = Some(ScrollTo::Page(page));
        let deg = pane.rotation.degrees();
        doc.notice = Some(if deg == 0 {
            "Rotate view: off".into()
        } else {
            format!("Rotate view: {deg}° (display only; Document > Rotate turns pages)")
        });
        cx.notify();
    }

    fn on_wheel(
        &mut self,
        ix: usize,
        ev: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        if ix >= doc.panes.len() {
            return;
        }
        let dy = f32::from(ev.delta.pixel_delta(window.line_height()).y);
        if dy == 0.0 {
            return;
        }
        if !ev.modifiers.control {
            // One spread at a time: scrolling on past either end turns the page.
            let pane = &mut doc.panes[ix];
            if pane.mode.continuous {
                return;
            }
            let Some(layout) = &pane.layout else { return };
            let max_y = (layout.total_h - layout.view_h).max(0.0);
            let sy = -f32::from(pane.scroll.offset().y);
            let turn = if dy < 0.0 && sy >= max_y - 0.5 {
                1
            } else if dy > 0.0 && sy <= 0.5 {
                -1
            } else {
                return;
            };
            if pane
                .last_turn
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(350))
            {
                return;
            }
            pane.last_turn = Some(Instant::now());
            let to = if turn > 0 {
                ScrollTo::NextRow
            } else {
                ScrollTo::PrevRowEnd
            };
            pane.pending_scroll = Some(to);
            cx.notify();
            return;
        }
        // Zoom the pane under the mouse, anchored on the mouse: position
        // relative to its scroll container.
        doc.active = ix;
        let b = doc.pane().scroll.bounds();
        let anchor = (
            f32::from(ev.position.x - b.origin.x),
            f32::from(ev.position.y - b.origin.y),
        );
        let factor = if dy > 0.0 { 1.1 } else { 1.0 / 1.1 };
        self.zoom_by(factor, Some(anchor), cx);
        cx.stop_propagation();
    }

    // ----- rendering pipeline -------------------------------------------------

    /// Ask the pool for `want`'s image, unless it's had or asked for
    /// already: under its key, whichever page asked, so duplicates share
    /// it. It's taken in when it lands ([`Self::on_rendered`]).
    pub(crate) fn ensure_rendered(&mut self, want: Want, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let key = (want.view.key, want.job);
        if doc.tiles.contains_key(&key) || doc.inflight.contains_key(&key) {
            return;
        }
        if let Job::Page { rotation, .. } = want.job
            && doc.previews.contains_key(&(want.view.key, rotation))
        {
            return;
        }
        let Some(rx) = doc.request(want) else {
            return;
        };
        let task = cx.spawn(async move |this, cx| {
            let result = rx.await;
            this.update(cx, |viewer, cx| viewer.on_rendered(key, result, cx))
                .ok();
        });
        doc.inflight.insert(key, task);
    }

    fn on_rendered(
        &mut self,
        key: TileKey,
        result: Result<anyhow::Result<Rendered>, oneshot::Canceled>,
        cx: &mut Context<Self>,
    ) {
        let timing = self.timing;
        let launched = self.launched;
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        // We're running inside this task; detach rather than drop it.
        if let Some(task) = doc.inflight.remove(&key) {
            task.detach();
        }
        match result {
            // Only when the pool's worker is gone, which sending logged.
            Err(_canceled) => {}
            Ok(Err(e)) => log::error!("render {:?}: {e:#}", key.1),
            Ok(Ok(rendered)) => {
                if timing {
                    let kind = match key.1 {
                        Job::Page { .. } => "preview",
                        Job::Tile { .. } => "tile",
                    };
                    eprintln!(
                        "[timing] page {} {kind} {}x{} rendered in {:?}",
                        rendered.request.page.0,
                        rendered.raster.width,
                        rendered.raster.height,
                        rendered.render_time
                    );
                    if doc.first_paint.is_none() {
                        eprintln!(
                            "[timing] first pixels ready {:?} since launch",
                            launched.elapsed()
                        );
                    }
                }
                doc.first_paint.get_or_insert_with(Instant::now);
                let image = Arc::new(to_render_image(rendered.raster));
                let old = match key.1 {
                    Job::Page { rotation, .. } => doc.previews.insert((key.0, rotation), image),
                    Job::Tile { .. } => doc.tiles.insert(key, image),
                };
                if let Some(old) = old {
                    doc.retired.push(old);
                }
                cx.notify();
            }
        }
    }

    /// What File > Print needs from the view.
    pub(crate) fn print_context(
        &self,
        prefs: crate::print_dialog::PrintPrefs,
    ) -> Option<crate::print_dialog::PrintContext> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        let st = doc.model.state();
        let selected: Vec<usize> = st
            .order()
            .iter()
            .enumerate()
            .filter(|(_, p)| doc.selection.contains(p))
            .map(|(i, _)| i)
            .collect();
        let bookmark = doc.outline_selected.and_then(|id| {
            let title = st.bookmark(id)?.title.to_string();
            let pages = st
                .bookmark_pages(id)?
                .into_iter()
                .filter_map(|p| st.pos_of(p))
                .collect();
            Some((title, pages))
        });
        Some(crate::print_dialog::PrintContext {
            title: self.file_name(),
            from: doc.model.export_from(Actor::User),
            views: doc.views.clone(),
            current: doc.current_page(),
            selected,
            bookmark,
            prefs,
            printed: Default::default(),
        })
    }

    /// Run a print job: make the print file in the background and hand it
    /// to the printer from memory ([`send_print`]), or, for Save as PDF,
    /// ask where to save it.
    pub(crate) fn print(&mut self, req: crate::print_dialog::PrintRequest, cx: &mut Context<Self>) {
        if self.still_opening(cx) {
            return;
        }
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let from = doc.model.export_from(Actor::User);
        let n = req.pages.len();
        let pages = format!("{n} page{}", if n == 1 { "" } else { "s" });
        if req.save_pdf {
            let dir = self
                .path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default();
            let suggested = self
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| format!("{s} - print.pdf"));
            let rx = crate::file_dialog::save(cx, &dir, suggested.as_deref());
            cx.spawn(async move |this, cx| {
                let Ok(Ok(Some(path))) = rx.await else { return };
                // The save dialog asked before it named a file there.
                let out = Out::File {
                    path: path.clone(),
                    overwrite: true,
                };
                let export = Export {
                    out,
                    ..Export::print(from, &req.pages, req.markup)
                };
                let done = move |_: &Exported| format!("Saved {pages} to {}", path.display());
                let answers = sheaf_doc::export::PRINT_ANSWERS;
                this.update(cx, |v, cx| {
                    v.write_export(export, answers, "Save as PDF", done, cx)
                })
                .ok();
            })
            .detach();
            return;
        }
        let printer = req.job.printer.clone();
        doc.notice = Some(format!("Printing {pages} to {printer}…"));
        cx.notify();
        let timing = self.timing;
        cx.spawn(async move |this, cx| {
            let t = Instant::now();
            let result = cx
                .background_executor()
                .spawn(async move { send_print(from, &req) })
                .await;
            this.update(cx, |v, cx| {
                if timing {
                    eprintln!(
                        "[timing] print job of {pages} built and sent in {:?}",
                        t.elapsed()
                    );
                }
                if let State::Ready(doc) = &mut v.state {
                    doc.notice = Some(match result {
                        Ok(what) => format!("Sent {pages} to {printer} ({what})"),
                        Err(e) => format!("Printing failed: {e:#}"),
                    });
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// What File > Properties needs from the view.
    pub(crate) fn properties_context(&self) -> Option<crate::properties_dialog::PropertiesContext> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        Some(crate::properties_dialog::PropertiesContext {
            path: self.path.clone(),
            source: doc.model.primary().clone(),
            page_size: doc
                .shown_ref(doc.current_page())
                .map(|p| p.geom.shown())
                .map(|s| (s.width, s.height)),
            dirty: doc.model.is_dirty(),
        })
    }

    pub fn current_page(&self) -> usize {
        match &self.state {
            State::Ready(doc) => doc.current_page(),
            _ => 0,
        }
    }
}

/// Prints `req`'s pages of `from`'s version: their print file
/// ([`Export::print`]), made in memory and handed to the printer from
/// there, so no file is written (M10.5 §10.6). What the spooler said.
fn send_print(from: ExportFrom, req: &crate::print_dialog::PrintRequest) -> anyhow::Result<String> {
    let bytes = sheaf_doc::export::printed(from, &req.pages, req.markup, &sheaf_ops::NoProgress)?;
    sheaf_print::submit(&req.job, bytes.into())
}

/// Whether opening failed for a wrong or missing password, as the source's
/// reader found: a file opens there, and PDFium opens it only to draw it
/// (a password PDFium alone refused would be the document's notice).
pub(crate) fn needs_password(e: &anyhow::Error) -> bool {
    sheaf_doc::source::OpenError::is_password(e)
}

/// The notice for marks the open gave new ids, when there are any.
fn renamed_notice(n: usize) -> Option<String> {
    match n {
        0 => None,
        1 => Some(
            "1 annotation shared an id with another (copied by another program); it gets a new \
             one when you save"
                .into(),
        ),
        n => Some(format!(
            "{n} annotations shared an id with another (copied by another program); they get \
             new ones when you save"
        )),
    }
}

/// The model a file opens as in the app while its facts are read: its
/// pages, as [`sheaf_doc::open::start`] makes them, under `name`, its
/// marks made by `who`. [`take_facts`] opens it.
pub(crate) fn opening_model(
    store: Arc<Store>,
    primary: Src,
    name: Arc<str>,
    came: Option<Came>,
    who: Identity,
) -> Document {
    let policy = sheaf_doc::history::default_policy();
    let mut model = sheaf_doc::open::start(store, primary, name, came, policy);
    model.set_identity(who);
    model
}

/// `facts`, read from the file `blob`, made `model`'s opened version: what
/// the view shows beside it, and the change, which is
/// [`Cause::Opened`](sheaf_doc::Cause::Opened). None when `model` no
/// longer shows that file, or has opened already: another open of the
/// same bytes had its facts first.
pub(crate) fn take_facts(
    model: &mut Document,
    blob: sheaf_doc::BlobId,
    facts: sheaf_doc::open::Facts,
) -> Option<(sheaf_doc::open::ViewFacts, ChangeSet)> {
    if model.primary().id() != blob {
        return None;
    }
    model.ready(facts).ok()
}

/// The status line while a file's facts are read: the page its first
/// pass is on, while it's on one before the last.
fn opening_line(reading: Option<&sheaf_ops::Tracker>) -> String {
    let s = reading
        .map(sheaf_ops::Tracker::snapshot)
        .unwrap_or_default();
    match s.total {
        Some(total) if (1..total).contains(&s.done) => {
            format!("Opening: page {} of {total}…", s.done)
        }
        _ => "Opening…".into(),
    }
}

/// `a`, a mark the user drew, as a mark to make, holding `asset`. Who
/// made it and when are the host's (M10.5 §6.8).
pub(crate) fn new_mark(a: &Annot, asset: Option<BlobSel>) -> NewMark {
    NewMark {
        kind: mark_kind(&a.kind),
        style: mark_style(a),
        contents: a.contents.clone(),
        author: None,
        turn: a.turn,
        asset,
    }
}

/// The notice for an edit the model refused: [`STILL_OPENING`] while the
/// file opens, or else that `what` failed, and why.
pub(crate) fn failed(what: &str, e: &EditError) -> String {
    match e {
        EditError::NotReady => STILL_OPENING.into(),
        e => format!("{what} failed: {e}"),
    }
}

/// What the reader is told of `commit`, which left the document
/// `bookmarks` fewer bookmarks: its label, and what went with it (pages
/// and bookmarks, the ops' own or the cascades', and the form fields and
/// replies the cascades took); then what the commit tells besides.
fn commit_notice(commit: &Commit, bookmarks: usize) -> String {
    let cascaded = |rule: Rule| {
        (commit.cascades.iter())
            .filter(|c| c.rule == rule)
            .map(|c| c.objects)
            .sum()
    };
    let parts = [
        counted(commit.change.pages_removed.len(), "page", "pages"),
        counted(bookmarks, "bookmark", "bookmarks"),
        counted(cascaded(Rule::WidgetField), "form field", "form fields"),
        counted(cascaded(Rule::AnnotThread), "reply", "replies"),
    ];
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    let label = &commit.change.label;
    let notice = match parts.split_last() {
        None => label.to_string(),
        Some((last, [])) => format!("{label}: removed {last} — Ctrl+Z to undo"),
        Some((last, rest)) => format!(
            "{label}: removed {} and {last} — Ctrl+Z to undo",
            rest.join(", ")
        ),
    };
    // A form's copies renamed apart, scripts turned off, text drawn as `?`.
    match sheaf_doc::notice_text(&commit.notices) {
        Some(told) => format!("{notice}. {told}"),
        None => notice,
    }
}

/// `n` of something, `one` of it or `many`: None for none.
fn counted(n: usize, one: &str, many: &str) -> Option<String> {
    match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    }
}

/// Wrap a BGRA raster as a GPUI image without copying the pixel data.
/// `RenderImage` is documented as BGRA; `RgbaImage` here is just the container.
pub(crate) fn to_render_image(raster: sheaf_image::raster::Raster) -> RenderImage {
    debug_assert_eq!(raster.format, PixelFormat::Bgra);
    let buf = image::RgbaImage::from_raw(raster.width, raster.height, raster.data)
        .expect("raster size matches buffer");
    RenderImage::new(smallvec::smallvec![image::Frame::new(buf)])
}

impl Focusable for DocView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Everything `render` decides for one page: where it goes, what renders
/// it, and which tiles cover the visible part of it.
struct PagePlan {
    index: usize,
    /// As the pane shows it (rotate view included).
    pref: Shown,
    /// Where it renders from, and what it draws (its key).
    view: PageView,
    /// The turn its images are drawn with: its own from its engine's page,
    /// then the pane's.
    rotation: Rotation,
    /// The key its images are found by ([`Doc::image_key`]).
    shows: RenderKey,
    left: f32,
    top: f32,
    w: f32,
    h: f32,
    dev_w: u32,
    /// Tiles intersecting the (overdrawn) viewport, as its engine draws
    /// them, with priority.
    tiles: Vec<(Job, u8)>,
}

/// One pane's frame: its pages and the renders they want.
struct PaneFrame {
    plans: Vec<PagePlan>,
    /// Pages near the viewport whose images stay cached, with their keys
    /// and the rotation this pane draws them at.
    keep: Vec<(PageId, RenderKey, Rotation)>,
    wants: Vec<Want>,
}

/// In a non-continuous mode, pick the row a queued scroll lands on before
/// the pane is laid out, and turn scrolling past either end into a page
/// turn.
fn resolve_row(pane: &mut Pane, pages: usize) {
    let mode = pane.mode;
    let rows = mode.row_count(pages);
    if mode.continuous || rows == 0 {
        return;
    }
    let last = rows - 1;
    let at = |pane: &Pane| {
        pane.layout.as_ref().map(|l| {
            let max_y = (l.total_h - l.view_h).max(0.0);
            (-f32::from(pane.scroll.offset().y), max_y)
        })
    };
    match pane.pending_scroll {
        Some(
            ScrollTo::Page(p)
            | ScrollTo::Reveal { page: p, .. }
            | ScrollTo::Point { page: p, .. }
            | ScrollTo::Anchor { page: p, .. },
        ) => pane.row = mode.row_of(p.min(pages - 1)),
        Some(ScrollTo::End) => pane.row = last,
        Some(ScrollTo::NextRow) => {
            pane.pending_scroll = (pane.row < last).then(|| {
                pane.row += 1;
                ScrollTo::Top
            });
        }
        Some(ScrollTo::PrevRowEnd) => {
            pane.pending_scroll = (pane.row > 0).then(|| {
                pane.row -= 1;
                ScrollTo::Bottom
            });
        }
        Some(ScrollTo::By(dy)) => {
            if let Some((y, max_y)) = at(pane) {
                if dy > 0.0 && y >= max_y - 0.5 && pane.row < last {
                    pane.row += 1;
                    pane.pending_scroll = Some(ScrollTo::Top);
                } else if dy < 0.0 && y <= 0.5 && pane.row > 0 {
                    pane.row -= 1;
                    pane.pending_scroll = Some(ScrollTo::Bottom);
                }
            }
        }
        Some(ScrollTo::Top | ScrollTo::Bottom) | None => {}
    }
}

/// Lay out pane `ix`, apply its queued scroll, plan the renders its
/// visible pages need, and pin the engines they render from.
fn plan_pane(
    doc: &mut Doc,
    ix: usize,
    view: (f32, f32),
    bounds_known: bool,
    scale: f32,
) -> PaneFrame {
    let (view_w, view_h) = view;
    let pages = doc.model.page_count();
    let layout = doc.layout;
    let pane = &mut doc.panes[ix];
    resolve_row(pane, pages);
    let stale = pane.layout.as_ref().is_none_or(|l| {
        l.zoom != pane.zoom
            || l.mode != pane.mode
            || l.rotation != pane.rotation
            || (!pane.mode.continuous && l.row != pane.row)
            || l.layout != layout
            || (l.view_w - view_w).abs() > 0.5
            || (l.view_h - view_h).abs() > 0.5
    });
    if stale {
        // A resize (or zoom to fit following one) keeps the point at the
        // top centre of the pane where it is, unless a scroll is queued.
        if pane.pending_scroll.is_none()
            && let Some(old) = &pane.layout
            && ((old.view_w - view_w).abs() > 0.5 || (old.view_h - view_h).abs() > 0.5)
        {
            let sx = -f32::from(pane.scroll.offset().x);
            let sy = -f32::from(pane.scroll.offset().y);
            let (vx, vy) = (old.view_w.min(view_w) / 2.0, 0.0);
            if let Some(page) = old.page_at(sx + vx, sy + vy)
                && let Some(r) = old.rect(page)
            {
                pane.pending_scroll = Some(ScrollTo::Anchor {
                    page,
                    fx: (sx + vx - r.left) / r.w,
                    fy: (sy + vy - r.top) / r.h,
                    vx,
                    vy,
                });
            }
        }
        let layout = Layout::build(
            LayoutSpec {
                zoom: pane.zoom,
                mode: pane.mode,
                rotation: pane.rotation,
                row: pane.row,
                view_w,
                view_h,
                layout,
            },
            &doc.model,
        );
        pane.row = layout.row;
        pane.layout = Some(layout);
    }
    let layout = pane.layout.as_ref().expect("built above");
    let count = layout.len();

    // Apply any queued scroll now that we know geometry.
    let max_x = (layout.total_w - view_w).max(0.0);
    let max_y = (layout.total_h - view_h).max(0.0);
    let mut scroll_x = (-f32::from(pane.scroll.offset().x)).clamp(0.0, max_x);
    let mut scroll_y = (-f32::from(pane.scroll.offset().y)).clamp(0.0, max_y);
    if let Some(to) = pane.pending_scroll.take()
        && count > 0
    {
        let last = count - 1;
        let rect = |p: usize| layout.rect(p.min(last));
        match to {
            ScrollTo::Page(p) => {
                if let Some(top) = layout.row_top_of(p.min(last)) {
                    scroll_y = top - MARGIN;
                }
            }
            ScrollTo::By(dy) => scroll_y += dy,
            ScrollTo::End | ScrollTo::Bottom => scroll_y = max_y,
            ScrollTo::Top => scroll_y = 0.0,
            ScrollTo::NextRow | ScrollTo::PrevRowEnd => {}
            ScrollTo::Reveal { page, fx, fy } => {
                if let Some(r) = rect(page) {
                    let (x, y) = (r.left + fx * r.w, r.top + fy * r.h);
                    if y < scroll_y + 8.0 || y > scroll_y + view_h - 8.0 {
                        scroll_y = y - view_h / 3.0;
                    }
                    if x < scroll_x || x > scroll_x + view_w {
                        scroll_x = x - view_w / 2.0;
                    }
                }
            }
            ScrollTo::Point { page, fx, fy } => {
                if let Some(r) = rect(page) {
                    scroll_y = r.top + fy * r.h;
                    let x = r.left + fx * r.w;
                    if x < scroll_x || x > scroll_x + view_w {
                        scroll_x = x;
                    }
                }
            }
            ScrollTo::Anchor {
                page,
                fx,
                fy,
                vx,
                vy,
            } => {
                if let Some(r) = rect(page) {
                    scroll_x = r.left + fx * r.w - vx;
                    scroll_y = r.top + fy * r.h - vy;
                }
            }
        }
        scroll_x = scroll_x.clamp(0.0, max_x);
        scroll_y = scroll_y.clamp(0.0, max_y);
        pane.scroll.set_offset(point(px(-scroll_x), px(-scroll_y)));
    }

    let mut frame = PaneFrame {
        plans: Vec::new(),
        keep: Vec::new(),
        wants: Vec::new(),
    };
    if count == 0 {
        pane.current_page = 0;
        return frame;
    }
    let visible = layout.visible(scroll_y, scroll_y + view_h);
    pane.current_page = layout
        .page_at(scroll_x + view_w / 2.0, scroll_y + view_h * 0.5)
        .unwrap_or(0)
        .min(count - 1);

    // What renders the pages near the viewport, and their keys.
    let st = doc.model.state();
    let near = visible.start.saturating_sub(KEEP)..(visible.end + KEEP).min(count);
    let views: Vec<Option<PageView>> = (near.clone())
        .map(|i| doc.views.view(st, layout.refs[i].id, &doc.inputs).ok())
        .collect();
    let view_of = |i: usize| views[i - near.start];
    let turned = |view: &PageView| layout.rotation.plus(view.job.rotation);
    for i in near.clone() {
        if let Some(view) = view_of(i) {
            frame
                .keep
                .push((layout.refs[i].id, view.key, turned(&view)));
        }
    }
    let pins = pane.pins.get_or_insert_with(|| Pins::new(doc.views.pool()));
    pins.set(visible.clone().filter_map(view_of).map(|v| v.engine));

    // Plan tiles for pages near the viewport.
    let overdraw = view_h * TILE_OVERDRAW;
    let fetch = visible.start.saturating_sub(PREFETCH)..(visible.end + PREFETCH).min(count);
    for i in fetch {
        let (Some(r), Some(view)) = (layout.rect(i), view_of(i)) else {
            continue;
        };
        let pref = layout.refs[i];
        let rotation = turned(&view);
        let (left, top, w, h) = (r.left, r.top, r.w, r.h);
        let dev_w = (w * scale).round().max(1.0) as u32;
        let dev_h = (h * scale).round().max(1.0) as u32;
        // Priorities: visible previews, visible tiles, then the
        // same for prefetch pages — so an expensive off-screen
        // preview never delays on-screen tiles.
        let on_screen_page = visible.contains(&i);
        frame.wants.push(Want {
            id: pref.id,
            view,
            job: Job::Page {
                width: PREVIEW_W,
                rotation,
            },
            priority: if on_screen_page { 0 } else { 2 },
        });
        // Visible part of this page, in page-local device px.
        let vis_x0 = ((scroll_x - left) * scale).max(0.0);
        let vis_x1 = ((scroll_x + view_w - left) * scale).min(dev_w as f32);
        let vis_y0 = ((scroll_y - overdraw - top) * scale).max(0.0);
        let vis_y1 = ((scroll_y + view_h + overdraw - top) * scale).min(dev_h as f32);
        let strict_y0 = ((scroll_y - top) * scale).max(0.0);
        let strict_y1 = ((scroll_y + view_h - top) * scale).min(dev_h as f32);
        let mut tiles = Vec::new();
        // Until the container has been laid out once, the width
        // is a guess; don't rasterize tiles at the wrong zoom.
        if bounds_known && vis_x1 > vis_x0 && vis_y1 > vis_y0 {
            let (c0, c1) = (
                (vis_x0 / TILE as f32) as u32,
                ((vis_x1 / TILE as f32).ceil() as u32).max(1),
            );
            let (r0, r1) = (
                (vis_y0 / TILE as f32) as u32,
                ((vis_y1 / TILE as f32).ceil() as u32).max(1),
            );
            for row in r0..r1 {
                for col in c0..c1 {
                    let (x, y) = (col * TILE, row * TILE);
                    let (tw, th) = (TILE.min(dev_w - x), TILE.min(dev_h - y));
                    let on_screen = (y as f32) < strict_y1 && ((y + th) as f32) > strict_y0;
                    tiles.push((
                        Job::Tile {
                            page_width: dev_w,
                            page_height: dev_h,
                            x,
                            y,
                            w: tw,
                            h: th,
                            rotation,
                        },
                        if on_screen { 1 } else { 3 },
                    ));
                }
            }
        }
        for &(job, priority) in &tiles {
            frame.wants.push(Want {
                id: pref.id,
                view,
                job,
                priority,
            });
        }
        frame.plans.push(PagePlan {
            index: i,
            pref,
            view,
            rotation,
            shows: view.key,
            left,
            top,
            w,
            h,
            dev_w,
            tiles,
        });
    }
    for pl in &mut frame.plans {
        pl.shows = doc.image_key(pl.pref.id, pl.view.key, pl.rotation);
    }
    frame
}

/// Drop cached images no pane needs, by the keys they're found by: those
/// of every pane's pages near the viewport, of the thumbnails' and the
/// blank-page review's previews, and the keys each was last drawn by
/// ([`Doc::image_key`]), until its own has an image. Out go anything
/// outside those; tiles at a rotation no pane draws their key at; and, per
/// key, tiles at a stale zoom once its tiles at the current one are all
/// there. A key last drawn by keeps all its tiles until it gives way.
fn evict(doc: &mut Doc, frames: &[PaneFrame]) {
    let wanted: HashSet<TileKey> = frames
        .iter()
        .flat_map(|f| &f.plans)
        .flat_map(|pl| pl.tiles.iter().map(move |(j, _)| (pl.view.key, *j)))
        .collect();
    let panes: Vec<(PageId, RenderKey, Rotation)> =
        frames.iter().flat_map(|f| f.keep.iter().copied()).collect();
    let pages: HashSet<(PageId, Rotation)> = (panes.iter().chain(&doc.thumb_keep))
        .chain(&doc.review_keep)
        .map(|&(p, _, r)| (p, r))
        .collect();
    doc.drawn.retain(|at, _| pages.contains(at));
    // Each page's key, and the one it was last drawn by.
    let both = |&(p, k, r): &(PageId, RenderKey, Rotation)| {
        let last = doc.drawn.get(&(p, r)).filter(|&&d| d != k);
        [Some((k, r, false)), last.map(|&d| (d, r, true))]
    };
    let mut kept: HashMap<RenderKey, smallvec::SmallVec<[Rotation; 2]>> = HashMap::new();
    let mut complete: HashMap<RenderKey, bool> = HashMap::new();
    for pl in frames.iter().flat_map(|f| &f.plans) {
        let done = (pl.tiles.iter()).all(|(j, _)| doc.tiles.contains_key(&(pl.view.key, *j)));
        *complete.entry(pl.view.key).or_insert(true) &= done;
    }
    for (k, r, last) in panes.iter().flat_map(both).flatten() {
        let rots = kept.entry(k).or_default();
        if !rots.contains(&r) {
            rots.push(r);
        }
        if last {
            complete.entry(k).or_insert(false);
        }
    }
    let previews: HashSet<(RenderKey, Rotation)> = (panes.iter().chain(&doc.thumb_keep))
        .chain(&doc.review_keep)
        .flat_map(both)
        .flatten()
        .map(|(k, r, _)| (k, r))
        .collect();
    let evict: Vec<TileKey> = doc
        .tiles
        .keys()
        .copied()
        .filter(|t @ (k, j)| {
            let Some(rots) = kept.get(k) else {
                return true;
            };
            !rots.contains(&j.rotation())
                || (!wanted.contains(t) && complete.get(k).copied().unwrap_or(true))
        })
        .collect();
    for k in evict {
        if let Some(im) = doc.tiles.remove(&k) {
            doc.retired.push(im);
        }
    }
    let evict: Vec<(RenderKey, Rotation)> = (doc.previews.keys())
        .filter(|k| !previews.contains(k))
        .copied()
        .collect();
    for k in evict {
        if let Some(im) = doc.previews.remove(&k) {
            doc.retired.push(im);
        }
    }
    doc.inflight.retain(|t, _| {
        wanted.contains(t)
            || matches!(t.1, Job::Page { rotation, .. } if previews.contains(&(t.0, rotation)))
    });
}

/// What's drawn over pages besides the document's own marks.
struct Overlays<'a> {
    search: Option<&'a Search>,
    pattern: Option<&'a crate::pattern_panel::PatternSearch>,
    region: Option<&'a crate::region::Region>,
    tool: Tool,
    tool_color: [u8; 3],
    grid: bool,
    guides: &'a [crate::rulers::Guide],
}

/// One page as drawn in a pane: the preview underneath, any tiles we have
/// for it (stale-zoom ones scaled to fit, current on top), then search and
/// pattern hits, markup, and the editing overlays.
fn page_element(doc: &Doc, pl: &PagePlan, ov: &Overlays) -> gpui::Div {
    let mut page_div = div()
        .absolute()
        .top(px(pl.top))
        .left(px(pl.left))
        .w(px(pl.w))
        .h(px(pl.h))
        .bg(theme::PAGE)
        .shadow_md();
    // The key its images are found by, and the turn they were drawn with.
    let (key, rot) = (pl.shows, pl.rotation);
    if let Some(prev) = doc.previews.get(&(key, rot)) {
        page_div = page_div.child(
            div()
                .absolute()
                .inset_0()
                .child(img(prev.clone()).size_full().object_fit(ObjectFit::Fill)),
        );
    }
    let mut page_tiles: Vec<(&TileKey, &Arc<RenderImage>)> = doc
        .tiles
        .iter()
        .filter(|((k, j), _)| *k == key && j.rotation() == rot)
        .collect();
    // Stale zoom levels first so current ones paint over them.
    page_tiles.sort_by_key(|((_, j), _)| match j {
        Job::Tile { page_width, .. } => *page_width == pl.dev_w,
        _ => false,
    });
    for ((_, job), im) in page_tiles {
        let Job::Tile {
            page_width,
            page_height,
            x,
            y,
            w,
            h,
            ..
        } = job
        else {
            continue;
        };
        // Logical px per device px of the zoom this tile was rendered at.
        let sx = pl.w / *page_width as f32;
        let sy = pl.h / *page_height as f32;
        page_div = page_div.child(
            div()
                .absolute()
                .left(px(*x as f32 * sx))
                .top(px(*y as f32 * sy))
                .w(px(*w as f32 * sx))
                .h(px(*h as f32 * sy))
                .child(img(im.clone()).size_full().object_fit(ObjectFit::Fill)),
        );
    }
    // Hit rects, text and marks are in PageSpace; the pane frame puts them
    // on the page as drawn.
    let pane = pl.pref.geom;
    let to_pane = pane.to_view();
    let size = pane.shown();
    let k = pl.w / size.width.max(1.0);
    // A box in the pane frame, in page-local logical px.
    let place = |r: Box2| {
        div()
            .absolute()
            .left(px(r.x * k))
            .top(px(r.y * k))
            .w(px(r.w * k))
            .h(px(r.h * k))
    };
    let hit_rects = |hits: &[Hit], current: Option<usize>, color, current_color| {
        let mut out = Vec::new();
        for (ix, hit) in hits.iter().enumerate() {
            if hit.page != pl.pref.id {
                continue;
            }
            for r in &hit.rects {
                out.push(place(to_pane.apply_box(*r)).bg(if current == Some(ix) {
                    current_color
                } else {
                    color
                }));
            }
        }
        out
    };
    if ov.grid {
        page_div = page_div.child(crate::rulers::grid(pl.w, pl.h, k));
    }
    page_div = page_div.children(crate::rulers::guide_lines(ov.guides, pl.w, pl.h, k));
    // Search highlights on top of everything; pattern hits in their own color.
    if let Some(search) = ov.search {
        page_div = page_div.children(hit_rects(
            &search.hits,
            search.current,
            theme::SEARCH_HIT,
            theme::SEARCH_HIT_CURRENT,
        ));
    }
    if let Some(ps) = ov.pattern {
        page_div = page_div.children(hit_rects(
            &ps.hits,
            ps.current,
            theme::PATTERN_HIT,
            theme::PATTERN_HIT_CURRENT,
        ));
    }
    // Selected text.
    if let Some(sel) = doc.text_sel.as_ref().filter(|s| s.page == pl.pref.id) {
        let (a, b) = sel.range();
        page_div = page_div.children(
            (sel.chars)
                .rects(a, b)
                .into_iter()
                .map(|r| place(to_pane.apply_box(r)).bg(theme::SELECTION)),
        );
    }
    // Our markup, drawn over the page as the pane shows it, less what the
    // comment filter or View > Hide Comments hides.
    if !doc.comments_hidden {
        let marks = doc.model.state().mark_records_on(pl.pref.id);
        for m in marks.filter(|m| !doc.hidden_marks.contains(&m.annot.id)) {
            let elements = annot_elements(&m.annot, m.stamp.as_ref(), &to_pane, size, k);
            page_div = page_div.children(elements);
        }
    }
    // Selection outlines; handles when one mark is selected and nothing is
    // being dragged.
    let selected: Vec<sheaf_doc::Annot> = doc
        .selected_mark_list()
        .into_iter()
        .filter(|(pid, a)| {
            *pid == pl.pref.id
                && a.thread_parent().is_none()
                && !doc.comments_hidden
                && !doc.hidden_marks.contains(&a.id)
        })
        .map(|(_, a)| a)
        .collect();
    for a in &selected {
        let r = to_pane.apply_box(a.page_bounds());
        page_div = page_div.child(
            place(Box2 {
                x: r.x - 2.0,
                y: r.y - 2.0,
                w: r.w + 4.0,
                h: r.h + 4.0,
            })
            .border_1()
            .border_dashed()
            .border_color(theme::ACCENT),
        );
    }
    if let [a] = selected.as_slice()
        && doc.selected_marks.len() == 1
        && doc.drag.is_none()
    {
        const HANDLE: f32 = 7.0;
        // Where the edit frame's handles are, as the pane shows them.
        for (_, q) in crate::markup_tools::handle_spots(&a.mapped(&to_pane)) {
            page_div = page_div.child(
                div()
                    .absolute()
                    .left(px(q.x * k - HANDLE / 2.0))
                    .top(px(q.y * k - HANDLE / 2.0))
                    .w(px(HANDLE))
                    .h(px(HANDLE))
                    .bg(theme::PAGE)
                    .border_1()
                    .border_color(theme::ACCENT),
            );
        }
    }
    if let Some(ed) = &doc.annot_editor
        && ed.page == pl.pref.id
        && let Some((_, a)) = doc.model.state().find_annot(ed.id)
    {
        // Under the mark as shown (a callout's: under its box).
        let b = match &a.kind {
            AnnotKind::Callout { rect, .. } => *rect,
            _ => a.page_bounds(),
        };
        let r = to_pane.apply_box(b);
        page_div = page_div.child(
            div()
                .absolute()
                .left(px(r.x * k))
                .top(px((r.bottom() + 2.0) * k))
                .w(px(300.0))
                .child(ed.input.clone()),
        );
    }
    if let Some(a) = doc
        .selected_foreign
        .and_then(|id| doc.model.state().annot(id))
        && a.page == pl.pref.id
        && let Some(f) = a.foreign()
    {
        let outline = crate::comments_panel::foreign_outline(&f.summary, &to_pane, k);
        page_div = page_div.child(outline);
    }
    if let Some(overlay) = crate::region::overlay(ov.region, pl.pref.id, &to_pane, k) {
        page_div = page_div.child(overlay);
    }
    // Drags and drafts are in the edit frame.
    let edit = doc.edit_geom(pl.index).unwrap_or(pane);
    if let Some(preview) =
        DocView::drag_preview(doc, ov.tool, ov.tool_color, pl.index, &edit, &pane, k)
    {
        page_div = page_div.child(preview);
    }
    let [r, g, b] = ov.tool_color;
    let tool_rgb = rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32);
    if let Some(draft) = crate::shapes::draft_preview(doc, tool_rgb, pl.index, &edit, &pane, k) {
        page_div = page_div.child(draft);
    }
    page_div
}

impl DocView {
    /// The document's panes, laid out and drawn, with the status line for
    /// the active one.
    fn render_ready(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, SharedString, Option<SharedString>) {
        let viewport = window.viewport_size();
        let scale = window.scale_factor();
        let page_cursor = self.page_cursor();
        let State::Ready(doc) = &mut self.state else {
            unreachable!("render_ready on a document that isn't ready");
        };
        let full = self.presentation == crate::presentation::Presentation::FullScreen;
        // Full screen shows only the active pane.
        let shown: Vec<usize> = if full {
            vec![doc.active]
        } else {
            (0..doc.panes.len()).collect()
        };
        let panes = shown.len();
        let mut frames = Vec::with_capacity(panes);
        for &ix in &shown {
            // The scroll container's bounds lag a frame behind on first
            // layout; fall back to the window size until we have them.
            let bounds = doc.panes[ix].scroll.bounds();
            let bounds_known = f32::from(bounds.size.width) > 0.0;
            let view = if bounds_known {
                (f32::from(bounds.size.width), f32::from(bounds.size.height))
            } else {
                (
                    f32::from(viewport.width),
                    (f32::from(viewport.height) - STATUS_H) / panes as f32,
                )
            };
            frames.push(plan_pane(doc, ix, view, bounds_known, scale));
        }
        evict(doc, &frames);

        let ov = Overlays {
            search: self.search.as_ref().filter(|s| !s.hits.is_empty()),
            pattern: self.pattern.as_ref().filter(|p| !p.hits.is_empty()),
            region: self.region.as_ref(),
            tool: self.tool,
            tool_color: crate::markup_tools::color_of(self.tool, cx),
            grid: self.grid && !full,
            guides: if full { &[] } else { &self.guides },
        };
        let mut pane_els = Vec::with_capacity(panes);
        for (&ix, frame) in shown.iter().zip(&frames) {
            let pane = &doc.panes[ix];
            let layout = pane.layout.as_ref().expect("planned");
            let pages: Vec<_> = frame
                .plans
                .iter()
                .map(|pl| page_element(doc, pl, &ov))
                .collect();
            let preview = self.guide_drag.filter(|d| d.pane == ix).and_then(|d| {
                let pos = d.pos?;
                let b = pane.scroll.bounds();
                Some((
                    d.horizontal,
                    if d.horizontal {
                        pos.y - b.origin.y
                    } else {
                        pos.x - b.origin.x
                    },
                ))
            });
            let scroll_el = div()
                .id(("pages", ix))
                .track_scroll(&pane.scroll)
                .overflow_scroll()
                .size_full()
                .on_scroll_wheel(
                    cx.listener(move |v, ev, window, cx| v.on_wheel(ix, ev, window, cx)),
                )
                .cursor(page_cursor)
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |v, ev: &gpui::MouseDownEvent, window, cx| {
                        if v.presentation == crate::presentation::Presentation::FullScreen {
                            // Acrobat: click for the next page, Shift+click back.
                            v.turn_page(if ev.modifiers.shift { -1 } else { 1 }, cx);
                            return;
                        }
                        v.activate_pane(ix, cx);
                        v.on_page_mouse_down(ev, window, cx)
                    }),
                )
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |v, ev: &gpui::MouseDownEvent, _, cx| {
                        if v.presentation == crate::presentation::Presentation::FullScreen {
                            v.turn_page(-1, cx);
                            return;
                        }
                        v.activate_pane(ix, cx);
                        // On a mark: its menu.
                        v.open_mark_menu(ev.position, cx);
                    }),
                )
                .on_mouse_move(
                    cx.listener(|v, ev, window, cx| v.on_page_mouse_move(ev, window, cx)),
                )
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(|v, ev, window, cx| v.on_page_mouse_up(ev, window, cx)),
                )
                .on_mouse_up_out(
                    gpui::MouseButton::Left,
                    cx.listener(|v, ev, window, cx| v.on_page_mouse_up(ev, window, cx)),
                )
                .child(
                    div()
                        .relative()
                        .w(px(layout.total_w))
                        .h(px(layout.total_h))
                        .children(pages),
                )
                .into_any_element();
            pane_els.push(crate::rulers::with_rulers(
                ix,
                self.rulers && !full,
                preview,
                pane,
                scroll_el,
                cx,
            ));
        }
        let body = if pane_els.len() == 1 {
            pane_els.pop().expect("one pane")
        } else {
            // Split: the panes stacked, with a divider to drag. The active
            // pane gets an accent edge.
            let frac = doc.split;
            let active = doc.active;
            let mut els = pane_els.into_iter().enumerate();
            let (_, top) = els.next().expect("two panes");
            let (_, bottom) = els.next().expect("two panes");
            let edge = |on: bool| {
                div()
                    .h(px(2.0))
                    .flex_shrink_0()
                    .bg(if on { theme::ACCENT } else { theme::RAISED })
            };
            div()
                .id("split")
                .size_full()
                .flex()
                .flex_col()
                .on_mouse_move(cx.listener(|v, ev: &gpui::MouseMoveEvent, _, cx| {
                    v.drag_split(ev.position.y, cx)
                }))
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(|v, _, _, cx| v.end_split_drag(cx)),
                )
                .on_mouse_up_out(
                    gpui::MouseButton::Left,
                    cx.listener(|v, _, _, cx| v.end_split_drag(cx)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .h(relative(frac))
                        .min_h(px(0.0))
                        .child(edge(active == 0))
                        .child(div().flex_1().min_h(px(0.0)).child(top)),
                )
                .child(
                    div()
                        .id("split-divider")
                        .h(px(SPLITTER_H))
                        .flex_shrink_0()
                        .bg(theme::DIVIDER)
                        .cursor(gpui::CursorStyle::ResizeRow)
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(|v, _, _, cx| {
                                if let State::Ready(doc) = &mut v.state {
                                    doc.split_drag = true;
                                }
                                cx.stop_propagation();
                            }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h(px(0.0))
                        .child(edge(active == 1))
                        .child(div().flex_1().min_h(px(0.0)).child(bottom)),
                )
                .into_any_element()
        };

        let pane = doc.pane();
        let layout = pane.layout.as_ref().expect("planned");
        let count = layout.len();
        let zoom_mode = match pane.zoom {
            Zoom::FitWidth => "fit width",
            Zoom::FitPage => "fit page",
            Zoom::Custom(_) => "custom",
        };
        let size = doc
            .shown_ref(doc.current_page())
            .map(|p| p.geom.shown())
            .map(|s| format!("{:.0}×{:.0} pt", s.width, s.height))
            .unwrap_or_default();
        let mut view_mode = String::new();
        if pane.mode != PageMode::default() {
            view_mode = format!("  —  {}", pane.mode.label());
        }
        if pane.rotation != Rotation::R0 {
            view_mode.push_str(&format!("  —  view rotated {}°", pane.rotation.degrees()));
        }
        let status = format!(
            "page {}  —  {size}  —  {:.0}% ({zoom_mode}){view_mode}",
            doc.page_position(doc.current_page(), count),
            layout.zoom_percent(),
        );
        // Until the file's facts are read, it says it's still opening,
        // and how far it has read.
        let notice = doc
            .notice
            .clone()
            .or_else(|| (!doc.model.is_ready()).then(|| opening_line(doc.reading.as_ref())))
            .map(Into::into);
        let wants: Vec<_> = frames.into_iter().flat_map(|f| f.wants).collect();
        for want in wants {
            self.ensure_rendered(want, cx);
        }
        (body, status.into(), notice)
    }
}

impl Render for DocView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(rec) = self.pending_recovery.take() {
            self.offer_recovery(rec, window, cx);
        }
        if let Some((asking, ask)) = self.pending_ask.take() {
            self.ask_before_writing(asking, ask, window, cx);
        }
        if let Some((page, id)) = self.pending_edit.take() {
            self.start_annot_editor_for(page, id, window, cx);
        }
        // What the comment filter hides on the page, kept current whether
        // or not the panel is open (cached; cheap when nothing changed).
        self.comment_view(cx);
        // Stamp pictures the last frame asked for.
        crate::stamp_preview::make_wanted(cx);
        if std::mem::take(&mut self.pending_focus) {
            window.focus(&self.focus_handle, cx);
        }

        // Release textures for images we've replaced or evicted since last frame.
        if let State::Ready(doc) = &mut self.state {
            for old in doc.retired.drain(..) {
                if let Err(e) = window.drop_image(old) {
                    log::warn!("drop_image: {e:#}");
                }
            }
        }

        use crate::presentation::Presentation;
        let presentation = self.presentation;
        let full = presentation == Presentation::FullScreen;
        let form_bar = (!full).then(|| self.render_form_bar(cx)).flatten();
        let search_bar = (!full).then(|| self.render_search_bar(cx)).flatten();
        let goto_bar = (!full).then(|| self.render_goto()).flatten();
        let props_bar = (!full).then(|| self.render_mark_properties(cx)).flatten();
        let connector_bar = (!full).then(|| self.render_connector_bar(cx)).flatten();
        let measure_bar = (!full)
            .then(|| self.render_measure_bar(window, cx))
            .flatten();
        let group_list = self.render_group_list(cx);
        let mark_menu = self.render_mark_menu(cx);
        let mut body: Option<AnyElement> = None;
        // The status bar: where you are, then the latest notice, which
        // gives way (ellipsis, full text on hover) when space is short.
        let mut status_notice: Option<SharedString> = None;
        let status: SharedString = if matches!(self.state, State::Ready(_)) {
            let (el, status, notice) = self.render_ready(window, cx);
            body = Some(el);
            status_notice = notice;
            status
        } else {
            match &mut self.state {
                State::Loading => "Opening…".into(),
                State::Failed(e) => format!("Failed: {e}").into(),
                State::Locked(l) => {
                    if std::mem::take(&mut l.focus_pending) {
                        window.focus(&l.input.read(cx).focus_handle(cx), cx);
                    }
                    body = Some(
                        div()
                            .id("locked")
                            .size_full()
                            .child(locked_card(&self.path, l, cx))
                            .into_any_element(),
                    );
                    "Password required".into()
                }
                State::Ready(_) => unreachable!(),
            }
        };
        let review = self.render_blank_review(cx);
        let ocr_progress = self
            .ocr_progress()
            .or_else(|| self.job_progress())
            .or_else(|| {
                let (_, t) = self.plugin_run.as_ref()?;
                t.snapshot().fraction
            });

        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|v, _: &NextPage, _, cx| v.turn_page(1, cx)))
            .on_action(cx.listener(|v, _: &PrevPage, _, cx| v.turn_page(-1, cx)))
            .on_action(cx.listener(|v, _: &FirstPage, _, cx| v.scroll(ScrollTo::Page(0), cx)))
            .on_action(cx.listener(|v, _: &LastPage, _, cx| v.scroll(ScrollTo::End, cx)))
            .on_action(cx.listener(|v, _: &LineDown, _, cx| v.scroll(ScrollTo::By(LINE_STEP), cx)))
            .on_action(cx.listener(|v, _: &LineUp, _, cx| v.scroll(ScrollTo::By(-LINE_STEP), cx)))
            .on_action(cx.listener(|v, _: &ScreenDown, _, cx| {
                let h = v.pane_view_h();
                v.scroll(ScrollTo::By(h * 0.9), cx)
            }))
            .on_action(cx.listener(|v, _: &ScreenUp, _, cx| {
                let h = v.pane_view_h();
                v.scroll(ScrollTo::By(-h * 0.9), cx)
            }))
            .on_action(cx.listener(|v, _: &ZoomIn, _, cx| v.zoom_by(ZOOM_STEP, None, cx)))
            .on_action(cx.listener(|v, _: &ZoomOut, _, cx| v.zoom_by(1.0 / ZOOM_STEP, None, cx)))
            .on_action(
                cx.listener(|v, _: &ZoomActual, _, cx| v.set_zoom(Zoom::Custom(1.0), None, cx)),
            )
            .on_action(
                cx.listener(|v, _: &ZoomFitWidth, _, cx| v.set_zoom(Zoom::FitWidth, None, cx)),
            )
            .on_action(cx.listener(|v, _: &ZoomFitPage, _, cx| v.set_zoom(Zoom::FitPage, None, cx)))
            .on_action(cx.listener(|v, _: &SinglePage, _, cx| {
                v.set_page_mode(
                    PageMode {
                        spread: Spread::Single,
                        continuous: false,
                    },
                    cx,
                )
            }))
            .on_action(cx.listener(|v, _: &SinglePageContinuous, _, cx| {
                v.set_page_mode(PageMode::default(), cx)
            }))
            .on_action(cx.listener(|v, _: &TwoUp, _, cx| v.set_two_up(false, cx)))
            .on_action(cx.listener(|v, _: &TwoUpContinuous, _, cx| v.set_two_up(true, cx)))
            .on_action(cx.listener(|v, _: &ToggleCoverPage, _, cx| v.toggle_cover_page(cx)))
            .on_action(cx.listener(|v, _: &SplitView, _, cx| v.toggle_split(cx)))
            .on_action(cx.listener(|v, _: &NextPane, _, cx| v.cycle_pane(cx)))
            .on_action(cx.listener(|v, _: &RotateViewCw, _, cx| v.rotate_view(Rotation::R90, cx)))
            .on_action(cx.listener(|v, _: &RotateViewCcw, _, cx| v.rotate_view(Rotation::R270, cx)))
            .on_action(cx.listener(|v, _: &Find, window, cx| v.open_search(window, cx)))
            .on_action(
                cx.listener(|v, _: &FullScreen, window, cx| v.toggle_full_screen(window, cx)),
            )
            .on_action(cx.listener(|v, _: &ReadMode, window, cx| v.toggle_read_mode(window, cx)))
            .on_action(cx.listener(|v, _: &ToggleRulers, _, cx| v.toggle_rulers(cx)))
            .on_action(cx.listener(|v, _: &GoToPage, window, cx| v.open_goto(window, cx)))
            .on_action(cx.listener(|v, _: &ToggleGrid, _, cx| v.toggle_grid(cx)))
            .on_action(cx.listener(|v, _: &ClearGuides, _, cx| v.clear_guides(cx)))
            .when(self.guide_drag.is_some(), |d| {
                d.on_mouse_move(
                    cx.listener(|v, ev: &gpui::MouseMoveEvent, _, cx| {
                        v.move_guide(ev.position, cx)
                    }),
                )
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(|v, _, _, cx| v.finish_guide(cx)),
                )
                .on_mouse_up_out(
                    gpui::MouseButton::Left,
                    cx.listener(|v, _, _, cx| v.finish_guide(cx)),
                )
            })
            .on_action(cx.listener(|v, _: &CloseSearch, window, cx| {
                if v.presentation == crate::presentation::Presentation::FullScreen
                    && v.leave_presentation(window, cx)
                {
                    // Esc leaves full screen before anything else.
                } else if v.clear_region(cx) {
                    // Esc drops a selected region first.
                } else if v.search.is_some() {
                    v.close_search(window, cx)
                } else if v.cancel_ocr(cx) {
                    // Esc during a Recognize Text run stops it.
                } else if v.close_blank_review(cx) {
                    // Esc closes the blank-page review.
                } else if v.cancel_planner(cx) {
                    // Likewise the newest insert, Trim White, watermark
                    // removal, header and footer, comment import or
                    // blank-page search.
                } else if v.cancel_job(cx) {
                    // And an export or a split.
                } else if v.cancel_plugin_run(cx) {
                    // And a plugin command that reports progress.
                } else if v.cancel_tape_placing(cx) || v.cancel_shape(cx) || v.close_group_list(cx)
                {
                    // Esc with a tape half-placed drops it; then the pages
                    // list closes.
                } else if v.clear_text_sel(cx) {
                    // Then selected text.
                } else {
                    if let State::Ready(doc) = &mut v.state {
                        doc.selected_marks.clear();
                        doc.selected_foreign = None;
                        doc.drag = None;
                    }
                    if v.tool != Tool::Select {
                        v.set_tool(Tool::Select, cx);
                    }
                    v.clear_selection(cx);
                    v.leave_presentation(window, cx);
                }
            }))
            .on_action(cx.listener(|v, _: &SearchNext, _, cx| {
                // Enter finishes a shape being drawn, else finds the next.
                if !v.finish_shape(cx) {
                    v.search_step(1, cx)
                }
            }))
            .on_action(cx.listener(|v, _: &crate::UndoVertex, _, cx| {
                v.undo_vertex(cx);
            }))
            .on_action(
                cx.listener(|v, _: &crate::ToolPolygon, _, cx| v.set_tool(Tool::Polygon, cx)),
            )
            .on_action(
                cx.listener(|v, _: &crate::ToolPolyline, _, cx| v.set_tool(Tool::Polyline, cx)),
            )
            .on_action(cx.listener(|v, _: &crate::ToolCloud, _, cx| v.set_tool(Tool::Cloud, cx)))
            .on_action(
                cx.listener(|v, _: &crate::ToolDistance, _, cx| v.set_tool(Tool::Distance, cx)),
            )
            .on_action(
                cx.listener(|v, _: &crate::ToolPerimeter, _, cx| v.set_tool(Tool::Perimeter, cx)),
            )
            .on_action(cx.listener(|v, _: &crate::ToolArea, _, cx| v.set_tool(Tool::Area, cx)))
            .on_action(cx.listener(|v, _: &crate::CalibrateScale, _, cx| v.start_calibrate(cx)))
            .on_action(
                cx.listener(|v, _: &crate::ToolCallout, _, cx| v.set_tool(Tool::Callout, cx)),
            )
            .on_action(
                cx.listener(|v, _: &crate::ToolAttachFile, _, cx| v.set_tool(Tool::AttachFile, cx)),
            )
            .on_action(cx.listener(|v, _: &crate::CycleStampTools, _, cx| v.cycle_stamp_tools(cx)))
            .on_action(
                cx.listener(|v, _: &crate::CycleDrawingTools, _, cx| v.cycle_drawing_tools(cx)),
            )
            .on_action(cx.listener(|v, _: &crate::CycleTextTools, _, cx| v.cycle_text_tools(cx)))
            .on_action(cx.listener(|v, _: &SearchPrev, _, cx| v.search_step(-1, cx)))
            .on_action(
                cx.listener(|v, _: &RotatePageCw, _, cx| v.rotate_current(Turn::Quarter, cx)),
            )
            .on_action(cx.listener(|v, _: &RotatePageCcw, _, cx| v.rotate_current(Turn::Back, cx)))
            .on_action(cx.listener(|v, _: &DeletePage, _, cx| {
                if !v.delete_selected_annot(cx) && !v.delete_selected_foreign(cx) {
                    v.delete_current(cx)
                }
            }))
            .on_action(cx.listener(|v, _: &ToggleComments, _, cx| v.toggle_comments_shown(cx)))
            .on_action(cx.listener(|v, _: &CopySelection, _, cx| {
                if !v.copy_text(cx) {
                    v.copy_marks(cx);
                }
            }))
            .on_action(cx.listener(|v, _: &PasteMarks, _, cx| {
                v.paste_marks(false, cx);
            }))
            .on_action(cx.listener(|v, _: &PasteMarksInPlace, _, cx| {
                v.paste_marks(true, cx);
            }))
            .on_action(cx.listener(|v, _: &DuplicatePages, _, cx| v.duplicate_current(cx)))
            .on_action(cx.listener(|v, _: &SelectAllPages, _, cx| v.select_all(cx)))
            .on_action(cx.listener(|v, _: &Save, _, cx| v.save(cx).detach()))
            .on_action(cx.listener(|v, _: &SaveAs, _, cx| v.save_as(cx)))
            .on_action(cx.listener(|v, _: &ExportCleanCopy, _, cx| v.clean_copy_prompt(cx)))
            .on_action(cx.listener(|v, _: &ExportFlattened, _, cx| v.flattened_copy_prompt(cx)))
            .on_action(cx.listener(|v, _: &crate::SummarizeComments, _, cx| {
                v.export_comments_prompt(
                    ExportKind::Summary(sheaf_ops::comments::Layout::CommentsOnly),
                    cx,
                )
            }))
            .on_action(
                cx.listener(|v, _: &crate::SummarizeCommentsWithPages, _, cx| {
                    v.export_comments_prompt(
                        ExportKind::Summary(sheaf_ops::comments::Layout::Pages),
                        cx,
                    )
                }),
            )
            .on_action(cx.listener(|v, _: &crate::ExportCommentsCsv, _, cx| {
                v.export_comments_prompt(ExportKind::Csv, cx)
            }))
            .on_action(cx.listener(|v, _: &crate::ExportCommentsText, _, cx| {
                v.export_comments_prompt(ExportKind::Text, cx)
            }))
            .on_action(cx.listener(|v, _: &crate::ExportCommentsXfdf, _, cx| {
                v.export_comments_prompt(
                    ExportKind::Data(sheaf_ops::comments::DataFormat::Xfdf, false),
                    cx,
                )
            }))
            .on_action(cx.listener(|v, _: &crate::ExportCommentsFdf, _, cx| {
                v.export_comments_prompt(
                    ExportKind::Data(sheaf_ops::comments::DataFormat::Fdf, false),
                    cx,
                )
            }))
            .on_action(
                cx.listener(|v, _: &crate::ImportComments, _, cx| v.import_comments_prompt(cx)),
            )
            .on_action(cx.listener(|v, _: &ExtractPages, _, cx| v.extract_pages_prompt(cx)))
            .on_action(cx.listener(|v, _: &InsertPdf, _, cx| v.insert_pdf_prompt(cx)))
            .on_action(cx.listener(|v, _: &ToolSelect, _, cx| v.set_tool(Tool::Select, cx)))
            .on_action(cx.listener(|v, _: &ToolNote, _, cx| v.set_tool(Tool::Note, cx)))
            .on_action(
                cx.listener(|v, _: &ToolHighlight, _, cx| v.markup_tool(Tool::Highlight, cx)),
            )
            .on_action(
                cx.listener(|v, _: &ToolUnderline, _, cx| v.markup_tool(Tool::Underline, cx)),
            )
            .on_action(
                cx.listener(|v, _: &ToolStrikeOut, _, cx| v.markup_tool(Tool::StrikeOut, cx)),
            )
            .on_action(cx.listener(|v, _: &ToolSquiggly, _, cx| v.markup_tool(Tool::Squiggly, cx)))
            .on_action(cx.listener(|v, _: &ToolTextEdit, _, cx| v.markup_tool(Tool::TextEdit, cx)))
            .on_action(cx.listener(|v, _: &CycleTextMarkup, _, cx| v.cycle_text_markup(cx)))
            .on_action(cx.listener(|v, _: &ToggleCheckmark, _, cx| v.toggle_checkmark(cx)))
            .on_action(
                cx.listener(|v, _: &crate::NextConnectorLabel, _, cx| v.next_unused_label(cx)),
            )
            .on_action(cx.listener(|v, _: &crate::GroupNext, _, cx| v.group_step(1, cx)))
            .on_action(cx.listener(|v, _: &crate::GroupPrev, _, cx| v.group_step(-1, cx)))
            .on_action(cx.listener(|v, _: &crate::GroupPagesWith, _, cx| v.group_pages(true, cx)))
            .on_action(
                cx.listener(|v, _: &crate::GroupPagesWithout, _, cx| v.group_pages(false, cx)),
            )
            .on_action(cx.listener(|v, _: &ToolFreeText, _, cx| v.set_tool(Tool::FreeText, cx)))
            .on_action(cx.listener(|v, _: &ToolSquare, _, cx| v.set_tool(Tool::Square, cx)))
            .on_action(cx.listener(|v, _: &ToolEllipse, _, cx| v.set_tool(Tool::Ellipse, cx)))
            .on_action(cx.listener(|v, _: &ToolLine, _, cx| v.set_tool(Tool::Line, cx)))
            .on_action(cx.listener(|v, _: &ToolArrow, _, cx| v.set_tool(Tool::Arrow, cx)))
            .on_action(cx.listener(|v, _: &ToolInk, _, cx| v.set_tool(Tool::Ink, cx)))
            .on_action(cx.listener(|v, _: &ToolTick, _, cx| v.set_tool(Tool::Tick, cx)))
            .on_action(cx.listener(|v, _: &ToolStamp, _, cx| {
                v.set_tool(Tool::Stamp, cx);
                // The workspace shows the Stamps panel too.
                cx.propagate();
            }))
            .on_action(cx.listener(|v, _: &ToolConnector, _, cx| v.set_tool(Tool::Connector, cx)))
            .on_action(cx.listener(|v, _: &NewBookmark, window, cx| v.new_bookmark(window, cx)))
            .on_action(cx.listener(|v, _: &Undo, _, cx| v.undo(cx)))
            .on_action(cx.listener(|v, _: &Redo, _, cx| v.redo(cx)))
            .on_action(cx.listener(|v, _: &RemoveRecognizedText, window, cx| {
                v.remove_recognized_text_prompt(window, cx)
            }))
            // Only while a run exists, so the menu item greys out otherwise.
            .when(self.ocr_running(), |d| {
                d.on_action(cx.listener(|v, _: &CancelOcr, _, cx| {
                    v.cancel_ocr(cx);
                }))
            })
            .flex()
            .flex_col()
            .size_full()
            .bg(if full {
                theme::FULL_SCREEN
            } else {
                theme::RAISED
            })
            .text_color(theme::TEXT_SOFT)
            .children(form_bar)
            .children(search_bar)
            .children(goto_bar)
            .children(props_bar)
            .children(connector_bar)
            .children(measure_bar)
            .child(
                div()
                    .relative()
                    .flex_grow(1.0)
                    .min_h(px(0.0))
                    .children(body)
                    .children(review)
                    .children(group_list)
                    .children(mark_menu),
            )
            .when_some(ocr_progress, |d, frac| {
                d.child(
                    div()
                        .h(px(3.0))
                        .flex_shrink_0()
                        .bg(theme::BACKGROUND)
                        .child(div().h_full().w(relative(frac)).bg(theme::ACCENT)),
                )
            })
            .when(!full, |d| {
                d.child(
                    div()
                        .h(px(STATUS_H))
                        .flex_shrink_0()
                        .px_2()
                        .flex()
                        .items_center()
                        .bg(theme::BACKGROUND)
                        .text_sm()
                        .whitespace_nowrap()
                        .child(div().flex_shrink_0().child(status))
                        .when_some(status_notice, |d, notice| {
                            d.child(
                                div()
                                    .id("status-notice")
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .tooltip(sheaf_ui::tooltip::Tooltip::text(notice.clone()))
                                    .child(format!("  —  {notice}")),
                            )
                        }),
                )
            })
    }
}

// ---------------------------------------------------------------------------
// Sidebar panels. Rendered on behalf of the workspace via `update`, so they
// can share this view's caches and views.

impl DocView {
    /// The thumbnail strip isn't shown: the engines its pages render from
    /// are no longer pinned for it.
    pub fn unpin_strip(&mut self) {
        if let State::Ready(doc) = &mut self.state {
            doc.strip_pins.set([]);
        }
    }

    /// Thumbnail strip: virtualized, follows the current page, and is the
    /// page-organizer surface: click/Ctrl/Shift to select, drag to reorder.
    pub fn render_thumbnails(
        &mut self,
        panel_w: f32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let State::Ready(doc) = &mut self.state else {
            return div().p_2().text_sm().child("…").into_any_element();
        };
        if !cx.has_active_drag() {
            doc.drop_target = None;
        }
        let refs: Vec<Shown> = (doc.model.state().pages())
            .map(|(id, _, geom)| Shown::new(id, geom, Rotation::R0))
            .collect();
        let count = refs.len();
        if count == 0 {
            doc.thumb_keep.clear();
            doc.strip_pins.set([]);
            return div()
                .p_2()
                .text_sm()
                .text_color(theme::TEXT_MUTED)
                .child("No pages")
                .into_any_element();
        }
        let last = count - 1;
        let thumb_w = (panel_w - 2.0 * THUMB_PAD).clamp(40.0, THUMB_W);

        // Item geometry. Cumulative tops are cheap enough per frame.
        let sizes: Vec<PageSize> = refs.iter().map(|r| r.geom.shown()).collect();
        let item_h = |i: usize| {
            thumb_w * sizes[i].height / sizes[i].width.max(1.0) + THUMB_LABEL_H + THUMB_PAD
        };
        let layout = ListLayout::variable((0..count).map(item_h)).padded(THUMB_PAD, 0.0);
        // Midpoints decide which gap a drag is over.
        let mids: Arc<Vec<f32>> = Arc::new(
            (0..count)
                .map(|i| layout.top(i) + (layout.height(i) - THUMB_PAD) / 2.0)
                .collect(),
        );

        let bounds = doc.thumb_scroll.bounds();
        let view_h = f32::from(bounds.size.height).max(1.0);
        let max_y = (layout.total_height() - view_h).max(0.0);
        let mut scroll_y = (-f32::from(doc.thumb_scroll.offset().y)).clamp(0.0, max_y);

        // Follow the main view: when the current page changes, make sure its
        // thumbnail is visible.
        let cur = doc.current_page().min(last);
        let cur_id = refs[cur].id;
        if doc.thumb_followed != Some(cur_id) {
            if let Some(y) = layout.reveal_in(cur, THUMB_PAD, scroll_y, view_h) {
                scroll_y = y;
            }
            doc.thumb_scroll.set_offset(point(px(0.0), px(-scroll_y)));
            doc.thumb_followed = Some(cur_id);
        }

        let visible = layout.visible_in(scroll_y, view_h);
        // What renders the thumbnails near the visible ones, and their keys:
        // the strip has no rotate view.
        let st = doc.model.state();
        let near = visible.start.saturating_sub(2)..(visible.end + 2).min(count);
        let views: Vec<Option<PageView>> = (near.clone())
            .map(|i| doc.views.view(st, refs[i].id, &doc.inputs).ok())
            .collect();
        let view_of = |i: usize| views[i - near.start];
        doc.thumb_keep = (near.clone())
            .filter_map(|i| Some((refs[i].id, view_of(i)?)))
            .map(|(id, v)| (id, v.key, v.job.rotation))
            .collect();
        (doc.strip_pins).set(visible.clone().filter_map(view_of).map(|v| v.engine));

        // Pages that are bookmark targets get a marker (a delete would cascade).
        let targets: HashSet<PageId> = doc
            .model
            .state()
            .bookmarks_preorder()
            .iter()
            .filter_map(|(_, _, b)| b.page())
            .collect();
        let selection = doc.selection.clone();
        let labels: Vec<String> = visible.clone().map(|i| doc.page_label(i)).collect();
        let visible_start = visible.start;

        let mut wants = Vec::new();
        let mut items = Vec::new();
        for i in visible {
            let pref = &refs[i];
            let image = view_of(i).and_then(|view| {
                let rotation = view.job.rotation;
                wants.push(Want {
                    id: pref.id,
                    view,
                    job: Job::Page {
                        width: PREVIEW_W,
                        rotation,
                    },
                    priority: 2,
                });
                let key = doc.image_key(pref.id, view.key, rotation);
                doc.previews.get(&(key, rotation)).cloned()
            });
            let h = thumb_w * sizes[i].height / sizes[i].width.max(1.0);
            let current = i == cur;
            let selected = selection.contains(&pref.id);
            let bookmarked = targets.contains(&pref.id);
            // What a drag from this thumb carries: the selection if it's part
            // of it, otherwise just this page.
            let drag_ids: Vec<PageId> = if selected {
                refs.iter()
                    .filter(|p| selection.contains(&p.id))
                    .map(|p| p.id)
                    .collect()
            } else {
                vec![pref.id]
            };
            items.push(
                div()
                    .id(("thumb", pref.id.0 as usize))
                    .absolute()
                    .top(px(layout.top(i)))
                    .left(px(THUMB_PAD))
                    .w(px(thumb_w))
                    .flex()
                    .flex_col()
                    .items_center()
                    .cursor_pointer()
                    .on_click(cx.listener(move |v, ev: &gpui::ClickEvent, _, cx| {
                        v.select_click(i, ev.modifiers(), cx)
                    }))
                    .on_drag(DragPages { ids: drag_ids }, |d: &DragPages, _, _, cx| {
                        let n = d.ids.len();
                        cx.new(|_| DragPreview { count: n })
                    })
                    .child(
                        div()
                            .relative()
                            .w(px(thumb_w))
                            .h(px(h))
                            .bg(theme::PAGE)
                            .border_2()
                            .border_color(if current {
                                theme::ACCENT
                            } else if selected {
                                theme::SELECTED_BORDER
                            } else {
                                theme::BORDER
                            })
                            .when_some(image, |d, im| {
                                d.child(img(im).size_full().object_fit(ObjectFit::Fill))
                            })
                            .when(selected, |d| {
                                d.child(div().absolute().inset_0().bg(theme::SELECTION_WASH))
                            })
                            .when(bookmarked, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .top(px(0.0))
                                        .right(px(4.0))
                                        .w(px(10.0))
                                        .h(px(14.0))
                                        .bg(theme::BOOKMARK),
                                )
                            }),
                    )
                    .child(
                        div()
                            .h(px(THUMB_LABEL_H))
                            .text_xs()
                            .text_color(if current || selected {
                                theme::TEXT_BRIGHT
                            } else {
                                theme::TEXT_MUTED
                            })
                            .child(labels[i - visible_start].clone()),
                    ),
            );
        }

        // Drop indicator: a line in the gap the drag is over.
        let indicator = doc.drop_target.map(|at| {
            let y = if at >= count {
                layout.top(last) + layout.height(last) - THUMB_PAD / 2.0
            } else {
                layout.top(at) - THUMB_PAD / 2.0
            };
            div()
                .absolute()
                .left(px(THUMB_PAD / 2.0))
                .top(px(y - 1.0))
                .w(px(thumb_w + THUMB_PAD))
                .h(px(2.0))
                .bg(theme::ACCENT)
        });

        let mids_for_move = mids.clone();
        let el = div()
            .id("thumbs")
            .track_scroll(&doc.thumb_scroll)
            .overflow_y_scroll()
            .size_full()
            .on_drag_move(cx.listener(move |v, ev: &DragMoveEvent<DragPages>, _, cx| {
                let State::Ready(doc) = &mut v.state else {
                    return;
                };
                let local_y = f32::from(ev.event.position.y - ev.bounds.origin.y)
                    - f32::from(doc.thumb_scroll.offset().y);
                let at = mids_for_move.partition_point(|&m| m <= local_y);
                if doc.drop_target != Some(at) {
                    doc.drop_target = Some(at);
                    cx.notify();
                }
            }))
            .on_drop(cx.listener(|v, d: &DragPages, _, cx| v.drop_pages(d.ids.clone(), cx)))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(layout.total_height()))
                    .children(items)
                    .children(indicator),
            )
            .into_any_element();
        for want in wants {
            self.ensure_rendered(want, cx);
        }
        el
    }
}

/// Payload of a thumbnail drag.
#[derive(Clone)]
struct DragPages {
    ids: Vec<PageId>,
}

/// What follows the cursor while dragging pages.
struct DragPreview {
    count: usize,
}

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(theme::ACCENT)
            .text_color(theme::TEXT_BRIGHT)
            .text_sm()
            .child(if self.count == 1 {
                "1 page".to_string()
            } else {
                format!("{} pages", self.count)
            })
    }
}

/// A shape's outline (a cloud's scallops when it's cloudy), filled when
/// it's closed, as all but a polyline are, and has a fill: `shape`, `a`'s
/// laid out upright, with `m` taking it onto the page as drawn, `shown` in
/// size, at `k` logical px a point.
fn outline_elements(
    a: &Annot,
    shape: &AnnotKind,
    m: &Affine,
    shown: PageSize,
    k: f32,
    stroke: gpui::Rgba,
    alpha: u32,
) -> Vec<AnyElement> {
    let px_of = |p: Pt| {
        let q = m.apply(p);
        (q.x * k, q.y * k)
    };
    let closed = !matches!(shape, AnnotKind::Polyline { .. });
    let mut path: Vec<(f32, f32)> = if closed && a.cloudy {
        let outline = sheaf_doc::annot::outline_of(shape).unwrap_or_default();
        let curves =
            sheaf_doc::annot::cloud_curves(&outline, sheaf_doc::annot::cloud_step(a.stroke));
        let mut pts = Vec::new();
        for [p0, c1, c2, p1] in curves {
            for i in 0..8 {
                let t = i as f32 / 8.0;
                let u = 1.0 - t;
                pts.push(px_of(Pt {
                    x: u * u * u * p0.x
                        + 3.0 * u * u * t * c1.x
                        + 3.0 * u * t * t * c2.x
                        + t * t * t * p1.x,
                    y: u * u * u * p0.y
                        + 3.0 * u * u * t * c1.y
                        + 3.0 * u * t * t * c2.y
                        + t * t * t * p1.y,
                }));
            }
        }
        pts
    } else {
        match shape {
            AnnotKind::Polygon { points } | AnnotKind::Polyline { points } => {
                points.iter().map(|p| px_of(*p)).collect()
            }
            other => sheaf_doc::annot::outline_of(other)
                .unwrap_or_default()
                .into_iter()
                .map(px_of)
                .collect(),
        }
    };
    if closed && let Some(&first) = path.first() {
        path.push(first);
    }
    let mut out = Vec::new();
    if closed && let Some(f) = a.fill {
        let u = ((f[0] as u32) << 16) | ((f[1] as u32) << 8) | f[2] as u32;
        out.push(crate::markup_tools::fill_canvas(
            vec![path.clone()],
            rgba((u << 8) | alpha),
            shown,
            k,
        ));
    }
    out.push(stroke_canvas_dashed(
        vec![path],
        stroke,
        a.stroke * k,
        a.dashed.then_some(k),
        shown,
        k,
    ));
    out
}

/// A text edit's caret, drawn as the file draws it: `at` upright, and `m`
/// taking it onto the page (see [`outline_elements`]).
fn caret_element(at: Box2, color: gpui::Rgba, m: &Affine, shown: PageSize, k: f32) -> AnyElement {
    let tri = sheaf_doc::annot::caret_points(at)
        .into_iter()
        .map(|p| {
            let q = m.apply(p);
            (q.x * k, q.y * k)
        })
        .collect();
    crate::markup_tools::fill_canvas(vec![tri], color, shown, k)
}

/// The page mark `id` of `st` holds, if it's a page stamp.
pub(crate) fn stamp_of(st: &DocState, id: AnnotId) -> Option<&FormRef> {
    st.annot(id)?.mark_record()?.stamp.as_ref()
}

/// GPUI elements for one mark, positioned in page-local logical px:
/// `to_pane` takes PageSpace onto the page as drawn, `shown` in size, at
/// `k` logical px a point. The mark is laid out as it reads (upright),
/// then turned onto the page, as the file draws it; a page stamp draws
/// `held`, the page it holds.
pub(crate) fn annot_elements(
    a: &Annot,
    held: Option<&FormRef>,
    to_pane: &Affine,
    shown: PageSize,
    k: f32,
) -> Vec<AnyElement> {
    let up = a.upright();
    let m = Affine::upright(a.turn).inverse().then(*to_pane);
    let rgb_u32 = ((a.color[0] as u32) << 16) | ((a.color[1] as u32) << 8) | a.color[2] as u32;
    let color = rgb(rgb_u32);
    let alpha = (a.opacity.clamp(0.0, 1.0) * 255.0) as u32;
    let fill = rgba((rgb_u32 << 8) | alpha);
    let place = |b: Box2| {
        let r = m.apply_box(b);
        div()
            .absolute()
            .left(px(r.x * k))
            .top(px(r.y * k))
            .w(px(r.w * k))
            .h(px(r.h * k))
    };
    let mut out = Vec::new();
    match &up {
        AnnotKind::Highlight { rects } => {
            for r in rects {
                out.push(place(*r).bg(fill).into_any_element());
            }
        }
        AnnotKind::Note { pos } => {
            let b = Box2 {
                x: pos.x,
                y: pos.y,
                w: sheaf_doc::annot::NOTE_SIZE,
                h: sheaf_doc::annot::NOTE_SIZE,
            };
            out.push(
                place(b)
                    .bg(fill)
                    .border_1()
                    .border_color(theme::INK)
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme::INK)
                    .text_size(px((NOTE_GLYPH * k).max(6.0)))
                    .child("…")
                    .into_any_element(),
            );
        }
        AnnotKind::FreeText { rect, size } => {
            out.push(
                place(*rect)
                    .p(px(2.0 * k))
                    .overflow_hidden()
                    .text_color(color)
                    .text_size(px(*size * k))
                    .line_height(px(*size * 1.2 * k))
                    .child(a.contents.clone())
                    .into_any_element(),
            );
        }
        AnnotKind::Square { .. } | AnnotKind::Ellipse { .. } | AnnotKind::Polygon { .. }
            if a.cloudy =>
        {
            out.extend(outline_elements(a, &up, &m, shown, k, fill, alpha));
        }
        AnnotKind::Polygon { .. } | AnnotKind::Polyline { .. } => {
            out.extend(outline_elements(a, &up, &m, shown, k, fill, alpha));
        }
        AnnotKind::Measure {
            what,
            points,
            scale,
        } => {
            use sheaf_doc::measure::Measured;
            let px_of = |p: Pt| {
                let q = m.apply(p);
                (q.x * k, q.y * k)
            };
            let mut line: Vec<(f32, f32)> = points.iter().map(|p| px_of(*p)).collect();
            if *what == Measured::Area && points.len() > 2 {
                line.push(line[0]);
            }
            let mut paths = vec![line];
            if *what == Measured::Distance
                && let [first, .., last] = points.as_slice()
            {
                let (p0, p1) = (px_of(*first), px_of(*last));
                let head = (6.0 + a.stroke * 2.0) * k;
                paths.push(arrow_head(p0, p1, head));
                paths.push(arrow_head(p1, p0, head));
            }
            out.push(stroke_canvas_dashed(
                paths,
                fill,
                a.stroke * k,
                a.dashed.then_some(k),
                shown,
                k,
            ));
            if let Some(b) = sheaf_doc::annot::measure_label_box(*what, points, scale) {
                let size = sheaf_doc::annot::MEASURE_TEXT;
                out.push(
                    place(b)
                        .px(px(2.0 * k))
                        .whitespace_nowrap()
                        .text_color(color)
                        .text_size(px(size * k))
                        .line_height(px((size + 3.0) * k))
                        .child(sheaf_doc::measure::label(*what, points, scale))
                        .into_any_element(),
                );
            }
        }
        AnnotKind::FileAttachment { pos, icon, .. } => {
            let at = Box2 {
                x: pos.x,
                y: pos.y,
                w: sheaf_doc::annot::NOTE_SIZE,
                h: sheaf_doc::annot::NOTE_SIZE,
            };
            let paths = sheaf_doc::annot::attachment_icon(icon, at)
                .into_iter()
                .map(|path| {
                    path.into_iter()
                        .map(|p| {
                            let q = m.apply(p);
                            (q.x * k, q.y * k)
                        })
                        .collect()
                })
                .collect();
            out.push(stroke_canvas(paths, fill, 1.5 * k, shown, k));
        }
        AnnotKind::Callout {
            rect,
            knee,
            point,
            size,
        } => {
            let px_of = |p: Pt| {
                let q = m.apply(p);
                (q.x * k, q.y * k)
            };
            let end = sheaf_doc::annot::callout_end(*rect, *knee);
            let (e, kn, pt) = (px_of(end), px_of(*knee), px_of(*point));
            out.push(stroke_canvas(
                vec![
                    vec![e, kn, pt],
                    arrow_head(kn, pt, (6.0 + a.stroke * 2.0) * k),
                ],
                fill,
                a.stroke * k,
                shown,
                k,
            ));
            out.push(
                place(*rect)
                    .bg(theme::PAGE)
                    .border(px((a.stroke * k).max(1.0)))
                    .border_color(fill)
                    .p(px(2.0 * k))
                    .overflow_hidden()
                    .text_color(color)
                    .text_size(px(*size * k))
                    .line_height(px(*size * 1.2 * k))
                    .child(a.contents.clone())
                    .into_any_element(),
            );
        }
        AnnotKind::Square { rect } | AnnotKind::Ellipse { rect } => {
            let inside = a.fill.map(|f| {
                let u = ((f[0] as u32) << 16) | ((f[1] as u32) << 8) | f[2] as u32;
                rgba((u << 8) | alpha)
            });
            out.push(
                place(*rect)
                    .border(px((a.stroke * k).max(1.0)))
                    .border_color(fill)
                    .when(a.dashed, |d| d.border_dashed())
                    .when_some(inside, |d, c| d.bg(c))
                    .when(matches!(up, AnnotKind::Ellipse { .. }), |d| {
                        d.rounded_full()
                    })
                    .into_any_element(),
            );
        }
        AnnotKind::Stamp { rect, glyph } => {
            out.push(
                place(*rect)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(color)
                    .text_size(px(rect.h * 0.8 * k))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(glyph.clone())
                    .into_any_element(),
            );
        }
        AnnotKind::Tape {
            rect, size, look, ..
        } => {
            use sheaf_doc::annot::{TAPE_LINE, TAPE_PAD, tape_columns};
            use sheaf_doc::tape::{Row, fmt_amount};
            let [pr, pg, pb] = look.paper;
            let paper = rgb(((pr as u32) << 16) | ((pg as u32) << 8) | pb as u32);
            if look.rotation % 360 != 0 {
                // GPUI can't turn text: a picture of the tape as the file
                // draws it.
                let el = match crate::stamp_preview::tape_picture(a) {
                    Some(image) => {
                        place(*rect).child(img(image).size_full().object_fit(ObjectFit::Fill))
                    }
                    None => place(*rect)
                        .bg(paper)
                        .border_1()
                        .border_color(theme::TAPE_BORDER),
                };
                out.push(el.into_any_element());
            } else {
                let tape = a.tape().unwrap_or_default();
                let (number_w, amount_w, symbol_w, _) = tape_columns(&tape, *size, look);
                let fs = *size * k;
                let line_h = *size * TAPE_LINE * k;
                let rows: Vec<AnyElement> =
                    look.shown(&tape)
                        .into_iter()
                        .map(|(number, l)| {
                            let total = l.row.is_total();
                            let body: AnyElement = match &l.row {
                                Row::Title(t) => div()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .whitespace_nowrap()
                                    .child(t.clone())
                                    .into_any_element(),
                                row => div()
                                    .flex()
                                    .child(div().w(px(amount_w * k)).flex().justify_end().child(
                                        fmt_amount(row.value().unwrap_or(0.0), look.decimals),
                                    ))
                                    .child(
                                        div()
                                            .w(px(symbol_w * k))
                                            .pl(px(*size * 0.3 * k))
                                            .child(row.symbol()),
                                    )
                                    .into_any_element(),
                            };
                            div()
                                .flex()
                                .items_center()
                                .h(px(line_h))
                                .whitespace_nowrap()
                                .when(total, |d| {
                                    d.border_t_1()
                                        .border_color(theme::TAPE_RULE)
                                        .font_weight(gpui::FontWeight::BOLD)
                                })
                                .when(look.numbers, |d| {
                                    d.child(
                                        div()
                                            .w(px(number_w * k))
                                            .flex_shrink_0()
                                            .text_color(theme::TAPE_RULE)
                                            .font_weight(gpui::FontWeight::NORMAL)
                                            .child(number),
                                    )
                                })
                                .child(body)
                                .when(!l.note.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .pl(px(*size * 0.4 * k))
                                            .font_weight(gpui::FontWeight::NORMAL)
                                            .child(l.note.clone()),
                                    )
                                })
                                .into_any_element()
                        })
                        .collect();
                out.push(
                    place(*rect)
                        .bg(paper)
                        .border_1()
                        .border_color(theme::TAPE_BORDER)
                        .p(px(TAPE_PAD * k))
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .text_color(color)
                        .text_size(px(fs))
                        .line_height(px(line_h))
                        .children(rows)
                        .into_any_element(),
                );
            }
        }
        AnnotKind::Connector { rect, label, .. } => {
            out.push(
                place(*rect)
                    .rounded_full()
                    .bg(fill)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme::TEXT_BRIGHT)
                    .text_size(px(rect.h * 0.55 * k))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(label.clone())
                    .into_any_element(),
            );
        }
        AnnotKind::Underline { rects }
        | AnnotKind::StrikeOut { rects }
        | AnnotKind::Squiggly { rects }
        | AnnotKind::Replace { rects, .. } => {
            // A replacement crosses its text out, then shows its caret.
            let lines = match &up {
                AnnotKind::Replace { .. } => AnnotKind::StrikeOut {
                    rects: rects.clone(),
                },
                other => other.clone(),
            };
            let paths = sheaf_doc::annot::text_line_paths(&lines)
                .into_iter()
                .map(|path| {
                    path.into_iter()
                        .map(|p| {
                            let q = m.apply(p);
                            (q.x * k, q.y * k)
                        })
                        .collect()
                })
                .collect();
            let width = sheaf_doc::annot::text_line_width(rects);
            out.push(stroke_canvas(paths, fill, width * k, shown, k));
            if let AnnotKind::Replace { at, .. } = &up {
                out.push(caret_element(*at, fill, &m, shown, k));
            }
        }
        AnnotKind::Insert { at } => out.push(caret_element(*at, fill, &m, shown, k)),
        // In their parent's thread, not on the page.
        AnnotKind::Reply { .. } | AnnotKind::Status { .. } => {}
        AnnotKind::PageStamp {
            rect,
            stamp,
            name,
            text,
        } => {
            let el = match crate::stamp_preview::picture(stamp, held) {
                Some(image) => place(*rect)
                    .opacity(a.opacity.clamp(0.0, 1.0))
                    .child(img(image).size_full().object_fit(ObjectFit::Fill)),
                // Until its picture is made (a moment), its name in a box.
                None => place(*rect)
                    .border_1()
                    .border_dashed()
                    .border_color(color)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(color)
                    .text_size(px((10.0 * k).max(6.0)))
                    .child(name.clone()),
            };
            let el = match text.as_ref().filter(|t| !t.is_empty()) {
                Some(t) => {
                    let size = (rect.h * 0.16).clamp(5.0, 24.0);
                    el.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom(px(rect.h * 0.08 * k))
                            .flex()
                            .justify_center()
                            .text_color(color)
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_size(px(size * k))
                            .line_height(px(size * k))
                            .child(t.clone()),
                    )
                }
                None => el,
            };
            out.push(el.into_any_element());
        }
        AnnotKind::PageBorder { rect, .. } => out.push(
            place(*rect)
                .border(px((a.stroke * k).max(1.0)))
                .border_color(fill)
                .into_any_element(),
        ),
        AnnotKind::TextStamp { rect, text, .. } => {
            let lw = sheaf_doc::annot::text_stamp_line(*rect);
            let mut el = place(*rect)
                .border(px(lw * k))
                .border_color(fill)
                .rounded(px(rect.h * 0.18 * k))
                .text_color(fill)
                .font_weight(gpui::FontWeight::BOLD);
            for (line, size, baseline) in sheaf_doc::annot::text_stamp_lines(*rect, text) {
                el = el.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px((baseline - rect.y - size * 0.9) * k))
                        .flex()
                        .justify_center()
                        .text_size(px(size * k))
                        .line_height(px(size * k))
                        .child(line),
                );
            }
            out.push(el.into_any_element());
        }
        AnnotKind::Line {
            a: pa,
            b: pb,
            arrow,
            arrow_start,
        } => {
            let (qa, qb) = (m.apply(*pa), m.apply(*pb));
            let (a2, b2) = ((qa.x * k, qa.y * k), (qb.x * k, qb.y * k));
            let head = (6.0 + a.stroke * 2.0) * k;
            out.push(stroke_canvas_dashed(
                vec![vec![a2, b2]],
                fill,
                a.stroke * k,
                a.dashed.then_some(k),
                shown,
                k,
            ));
            let mut heads = Vec::new();
            if *arrow {
                heads.push(arrow_head(a2, b2, head));
            }
            if *arrow_start {
                heads.push(arrow_head(b2, a2, head));
            }
            if !heads.is_empty() {
                out.push(stroke_canvas(heads, fill, a.stroke * k, shown, k));
            }
        }
        AnnotKind::Ink { paths } => {
            let polylines: Vec<Vec<(f32, f32)>> = paths
                .iter()
                .map(|p| {
                    p.iter()
                        .map(|q| {
                            let r = m.apply(*q);
                            (r.x * k, r.y * k)
                        })
                        .collect()
                })
                .collect();
            out.push(stroke_canvas_dashed(
                polylines,
                fill,
                a.stroke * k,
                a.dashed.then_some(k),
                shown,
                k,
            ));
        }
    }
    out
}

const NOTE_GLYPH: f32 = 12.0;

/// The password prompt shown in place of the pages for an encrypted file.
fn locked_card(path: &std::path::Path, l: &Locked, cx: &mut Context<DocView>) -> AnyElement {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("This file")
        .to_string();
    let input = l.input.clone();
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(380.0))
                .p_4()
                .flex()
                .flex_col()
                .gap_2()
                .bg(theme::CARD)
                .border_1()
                .border_color(theme::BORDER_CARD)
                .rounded_md()
                .child(
                    div()
                        .text_sm()
                        .child(format!("\"{name}\" is password protected.")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(if l.wrong {
                            theme::ERROR
                        } else {
                            theme::TEXT_MUTED
                        })
                        .child(if l.wrong {
                            "That password didn't work. Try again."
                        } else {
                            "Enter the password to open it."
                        }),
                )
                .child(input.clone())
                .child(
                    div().flex().justify_end().child(
                        Button::new("unlock", "Open")
                            .primary()
                            .on_click(cx.listener(move |v, _, _, cx| {
                                let pw = input.read(cx).text().to_string();
                                if !pw.is_empty() {
                                    v.password = Some(pw);
                                    v.open(cx);
                                }
                            })),
                    ),
                ),
        )
        .into_any_element()
}

/// Expand what's needed to see the bookmarks `made` in `st`: each one's
/// ancestors, walking up from it, and a made one with kids of its own (a
/// new heading, perhaps over bookmarks that were there). Everything else
/// keeps its expanded or collapsed state. A made bookmark's parent is one
/// of the bookmarks its commit wrote, in `keys`, whose kids changed; an
/// older ancestor is looked up in the outline, each once.
fn reveal_bookmarks(
    st: &DocState,
    made: impl IntoIterator<Item = OutlineId>,
    keys: &BTreeSet<Key>,
    expanded: &mut HashSet<OutlineId>,
) {
    let mut made = made.into_iter().peekable();
    if made.peek().is_none() {
        return;
    }
    let written: HashMap<OutlineId, OutlineId> = (keys.iter())
        .filter_map(|k| match *k {
            Key::Bookmark(id) => Some((id, st.bookmark(id)?)),
            _ => None,
        })
        .flat_map(|(id, b)| b.kids.iter().map(move |&kid| (kid, id)))
        .collect();
    let mut walked = HashSet::new();
    for id in made {
        if st.bookmark(id).is_some_and(|b| !b.kids.is_empty()) {
            expanded.insert(id);
        }
        let mut at = id;
        while walked.insert(at) {
            let up = match written.get(&at) {
                Some(&parent) => Some(parent),
                None => st.bookmark_place(at).and_then(|(parent, _)| parent),
            };
            let Some(parent) = up else {
                break;
            };
            expanded.insert(parent);
            at = parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context as _;
    use sheaf_edit::{BookmarkRef, Local, Sibling};

    /// The bookmarks a commit made come into view, walking up from each:
    /// their ancestors open, older ones and made ones alike, and so does a
    /// new heading over bookmarks that were there; everything else keeps
    /// its state.
    #[test]
    fn new_bookmarks_are_revealed() {
        // 1 (old, collapsed) > 2 (old) > 3 (new); 6 (old, collapsed) > 7
        // (old), which moves under 8 (new); 4 (new heading) > 5 (new).
        let item = |title: &str, depth: u16| sheaf_doc::FlatOutlineItem {
            title: title.into(),
            page: Some(0),
            depth,
            view: None,
            color: None,
            style: Default::default(),
            open: false,
            object: None,
            kept: false,
        };
        let store = Store::new();
        let primary = sheaf_doc::store::blank(&store, 1);
        let items = [item("1", 0), item("2", 1), item("6", 0), item("7", 1)];
        let policy = Box::new(sheaf_doc::FixedCount(10));
        let mut d = Document::new(store, primary, "a.pdf".into(), None, &items, policy);
        let st = d.state();
        let (one, six) = (st.outline_roots()[0], st.outline_roots()[1]);
        let two = st.bookmark(one).unwrap().kids[0];
        let seven = st.bookmark(six).unwrap().kids[0];
        let add = |parent: Option<BookmarkRef>, title: &str, local| Op::AddBookmark {
            parent,
            at: Sibling::Last,
            title: title.into(),
            target: None,
            view: None,
            color: None,
            style: Default::default(),
            local,
        };
        let ops = vec![
            add(Some(BookmarkRef::Id(two)), "3", None),
            add(None, "4", Some(Local(1))),
            add(Some(BookmarkRef::New(Local(1))), "5", None),
            add(None, "8", Some(Local(2))),
            Op::MoveBookmarks {
                bookmarks: vec![BookmarkRef::Id(seven)],
                parent: Some(BookmarkRef::New(Local(2))),
                at: Sibling::Last,
                with_pages: false,
            },
        ];
        let c = d.commit_ops(ops, "Bookmarks").unwrap();
        let made = |n| match c.created.locals[&Local(n)] {
            sheaf_doc::tx::Made::Bookmark(id) => id,
            _ => panic!("a bookmark"),
        };
        let mut expanded = HashSet::new();
        let bookmarks = c.created.bookmarks();
        reveal_bookmarks(d.state(), bookmarks, &c.change.keys, &mut expanded);
        assert_eq!(expanded, HashSet::from([one, two, made(1), made(2)]));
    }

    /// `path` opened as the view opens it, with no window: the model
    /// [`DocView::install`] makes, and the facts [`DocView::ready`] takes
    /// in, read as `finish_open` reads them.
    fn as_the_view_opens(
        path: &std::path::Path,
        recovered: bool,
    ) -> anyhow::Result<(Document, sheaf_doc::open::ViewFacts)> {
        let store = Store::new();
        let (primary, stamp) = store.read_file(path, None)?;
        let (src, blob) = (primary.clone(), primary.id());
        // As the app opens it: a recovered file's record has its sidecar's
        // sections pending (none, here).
        let came = match recovered {
            false => Came::Read {
                path: path.into(),
                stamp,
            },
            true => Came::Restored {
                path: path.into(),
                base: stamp,
                pending: materialize::PendingTail {
                    bytes: Some(Arc::from([])),
                    flags: materialize::TailFlags::default(),
                },
            },
        };
        let name = sheaf_doc::store::file_name(path);
        let mut model = opening_model(store, primary, name, Some(came), Identity::default());
        let facts = sheaf_doc::open::finish(&src, &sheaf_ops::NoProgress);
        let (view, _) = take_facts(&mut model, blob, facts).context("not taken")?;
        Ok((model, view))
    }

    /// Whether the view's open of `path` and the command line's both open
    /// it (false: neither does) as one document, or how they differ:
    /// whether it opens at all, the opened state, ids included, the
    /// view's facts, the version, or the ids the next edit draws.
    fn opens_alike(path: &std::path::Path) -> Result<bool, &'static str> {
        let (app, headless) = match (
            as_the_view_opens(path, false),
            sheaf_doc::open::file(path, None),
        ) {
            (Ok(app), Ok(headless)) => (app, headless),
            (Err(_), Err(_)) => return Ok(false),
            _ => return Err("it opens one way only"),
        };
        let ((mut a, view), mut h) = (app, headless.doc);
        if a.state() != h.state() {
            return Err("the opened state");
        }
        if view != headless.view {
            return Err("the view's facts");
        }
        let version = |d: &Document| (d.current_version(), d.is_dirty(), d.can_undo());
        if version(&a) != version(&h) {
            return Err("the version");
        }
        // A copy of a page draws an annotation id for each annotation on
        // it from the thread's rng, seeded alike for both, and a page id
        // and a field id for each field from counters that start alike.
        let duplicate = |d: &mut Document| {
            fastrand::seed(8);
            let first = [d.page(0).expect("a source has pages")];
            let duplicate = sheaf_doc::tx::fixture::duplicate(&first);
            d.commit_ops(vec![duplicate], "Duplicate")
                .map(|_| ())
                .map_err(|e| format!("{e:#}"))
        };
        if duplicate(&mut a) != duplicate(&mut h) || a.state() != h.state() {
            return Err("the ids the next edit draws");
        }
        Ok(true)
    }

    /// The view's open and the command line's are one open: on a file
    /// that says every fact, on one with every kind of field, and over
    /// Greg's corpus (skipped without it), they give one document. A
    /// recovered file opens the same, unsaved, and facts read from other
    /// bytes, or that come again, aren't taken.
    #[test]
    fn the_app_opens_as_the_command_line_does() {
        let dir = tempfile::tempdir().unwrap();
        let fixtures = [
            ("every-fact.pdf", sheaf_doc::forms::fixture::every_fact()),
            ("every-kind.pdf", sheaf_doc::forms::fixture::every_kind()),
        ];
        for (name, mut f) in fixtures {
            let path = dir.path().join(name);
            std::fs::write(&path, f.bytes()).unwrap();
            assert_eq!(opens_alike(&path), Ok(true), "{name}");
        }
        let path = dir.path().join("every-fact.pdf");
        let (recovered, _) = as_the_view_opens(&path, true).unwrap();
        let headless = sheaf_doc::open::file(&path, None).unwrap().doc;
        assert!(recovered.state() == headless.state());
        assert!(recovered.is_dirty() && !recovered.can_undo());
        let store = Store::new();
        let (primary, stamp) = store.read_file(&path, None).unwrap();
        let src = primary.clone();
        let came = Came::Read {
            path: path.as_path().into(),
            stamp,
        };
        let name = sheaf_doc::store::file_name(&path);
        let mut model = opening_model(store, primary, name, Some(came), Identity::default());
        let other = sheaf_doc::forms::fixture::every_kind().source();
        let facts = || sheaf_doc::open::finish(&src, &sheaf_ops::NoProgress);
        assert!(take_facts(&mut model, other.id(), facts()).is_none());
        assert!(!model.is_ready());
        assert!(take_facts(&mut model, src.id(), facts()).is_some());
        assert!(take_facts(&mut model, src.id(), facts()).is_none());

        let Some(corpus) = sheaf_doc::forms::corpus::Corpus::or_skip() else {
            return;
        };
        let entries = corpus.forms().unwrap();
        let outcomes = sheaf_doc::forms::corpus::each_entry(&entries, |entry| {
            opens_alike(&corpus.path(entry))
        });
        let differ: Vec<(&str, &str)> = entries
            .iter()
            .zip(&outcomes)
            .filter_map(|(entry, alike)| Some((entry.path.as_str(), alike.err()?)))
            .collect();
        eprintln!(
            "{} files: {} open neither way, {} otherwise in the app",
            entries.len(),
            outcomes.iter().filter(|o| **o == Ok(false)).count(),
            differ.len()
        );
        assert!(differ.is_empty(), "{differ:#?}");
    }

    /// One notice counts the marks given new ids at open (the screen
    /// leaves the file's copies of them out, as it does every mark the
    /// file lists: sheaf draws its own).
    #[test]
    fn one_notice_counts_the_renamed_marks() {
        assert_eq!(renamed_notice(0), None);
        assert!(
            renamed_notice(1)
                .unwrap()
                .starts_with("1 annotation shared an id")
        );
        assert!(
            renamed_notice(3)
                .unwrap()
                .starts_with("3 annotations shared an id")
        );
    }

    /// A save of `doc`'s current version to `path`, as the view makes one:
    /// sealed, planned and written, for the view to end
    /// ([`DocView::finish_save`]), which records it.
    #[cfg(target_os = "linux")]
    fn written(
        doc: &mut Doc,
        path: &std::path::Path,
    ) -> (Saving, Result<materialize::Written, materialize::SaveStop>) {
        let v = doc.model.current_version();
        let input = doc.model.save_input(v).unwrap();
        doc.saving = true;
        let target = doc.model.save_target(path);
        let plan = materialize::plan(&input, &target);
        let answers = materialize::AskAnswers::default();
        let written = materialize::write_file(
            &input,
            &plan,
            target.clone(),
            &answers,
            &sheaf_ops::NoProgress,
        );
        let saving = Saving {
            input: Arc::new(input),
            target,
            plan,
            answers,
            started: Instant::now(),
        };
        (saving, written)
    }

    /// A file of `pages` blank pages, in a `sheaf-` temporary directory,
    /// which the recent files leave out.
    #[cfg(target_os = "linux")]
    fn blank_file(pages: u32) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("blank.pdf");
        let bytes = sheaf_doc::memory::blank_pages(612.0, 792.0, pages).unwrap();
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    /// `doc`'s first pane laid out, as a frame would.
    #[cfg(target_os = "linux")]
    fn lay_out(doc: &mut Doc) {
        let pane = &doc.panes[0];
        let spec = LayoutSpec {
            zoom: pane.zoom,
            mode: pane.mode,
            rotation: pane.rotation,
            row: 0,
            view_w: 800.0,
            view_h: 600.0,
            layout: doc.layout,
        };
        doc.panes[0].layout = Some(Layout::build(spec, &doc.model));
    }

    /// The document `view` shows.
    #[cfg(target_os = "linux")]
    fn shown(view: &mut DocView) -> &mut Doc {
        match &mut view.state {
            State::Ready(doc) => doc,
            _ => panic!("ready"),
        }
    }

    /// A 1×1 image, standing in for a render.
    #[cfg(target_os = "linux")]
    fn pixel() -> Arc<RenderImage> {
        let pixel = image::Frame::new(image::RgbaImage::new(1, 1));
        Arc::new(RenderImage::new(smallvec::smallvec![pixel]))
    }

    /// A save changes nothing the document shows, so it drops nothing
    /// rendered: through the view's own end of a save (`finish_save`), the
    /// page's render key stays as it was, and every tile and preview kept
    /// under it is still there. Run in a headless app for the view's focus
    /// handle; on Linux, where it needs no main thread. The file is in a
    /// `sheaf-` temporary directory, which the recent files leave out.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_save_keeps_every_image() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("every-fact.pdf");
        std::fs::write(&path, sheaf_doc::forms::fixture::every_fact().bytes()).unwrap();
        let (mut model, _) = as_the_view_opens(&path, false).unwrap();
        let page = model.page(0).unwrap();
        let turn = sheaf_doc::tx::fixture::rotate(&[page], Turn::Quarter);
        model.commit_ops(vec![turn], "Rotate").unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let mut doc = Doc::new(model, cx.focus_handle(), Default::default());
                let key = |doc: &Doc| {
                    let view = doc.views.view(doc.model.state(), page, &doc.inputs);
                    view.unwrap().key
                };
                let was = key(&doc);
                doc.previews.insert((was, Rotation::R90), pixel());
                let job = Job::Page {
                    width: 100,
                    rotation: Rotation::R90,
                };
                doc.tiles.insert((was, job), pixel());
                let (saving, written) = written(&mut doc, &path);
                let view = cx.new(|cx| {
                    let mut view = DocView::unopened(path.clone(), None, false, Instant::now(), cx);
                    view.state = State::Ready(Box::new(doc));
                    view
                });
                view.update(cx, |view, cx| {
                    assert!(view.finish_save(saving, written, cx));
                    let State::Ready(doc) = &view.state else {
                        panic!("ready");
                    };
                    assert!(!doc.model.is_dirty() && !doc.saving);
                    assert_eq!(key(doc), was);
                    assert!(doc.previews.contains_key(&(was, Rotation::R90)));
                    assert!(doc.tiles.contains_key(&(was, job)));
                    assert!(doc.retired.is_empty());
                });
                ran.store(true, Ordering::Relaxed);
                // Once the event loop runs: a stop before it starts is lost.
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// A save with an edit made while it was written leaves the sidecar
    /// of that edit, against the new file, by the time the view has ended
    /// the save (M10.5 D39): the sidecar from before is stale against the
    /// new file, and a crash before a new one would lose the edit.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_save_with_an_edit_since_leaves_its_sidecar_as_it_ends() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("every-fact.pdf");
        std::fs::write(&path, sheaf_doc::forms::fixture::every_fact().bytes()).unwrap();
        let (mut model, _) = as_the_view_opens(&path, false).unwrap();
        let page = model.page(0).unwrap();
        let turn = move || vec![sheaf_doc::tx::fixture::rotate(&[page], Turn::Quarter)];
        model.commit_ops(turn(), "Rotate").unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let mut doc = Doc::new(model, cx.focus_handle(), Default::default());
                let (saving, written) = written(&mut doc, &path);
                // While it was written.
                doc.model.commit_ops(turn(), "Rotate").unwrap();
                let view = cx.new(|cx| {
                    let mut view = DocView::unopened(path.clone(), None, false, Instant::now(), cx);
                    view.state = State::Ready(Box::new(doc));
                    view
                });
                view.update(cx, |view, cx| {
                    assert!(view.finish_save(saving, written, cx));
                    let State::Ready(doc) = &view.state else {
                        panic!("ready");
                    };
                    assert!(doc.model.is_dirty());
                });
                assert!(sheaf_doc::recovery::find(&path).is_some());
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// A mark's edit lays nothing out (bug 26): through the view's one
    /// door ([`DocView::commit`]), an added mark leaves the layout count
    /// and the pane's layout as they were, while the change count moves,
    /// so the Comments panel's view is made again, with the mark. A turned
    /// page lays the pages out anew. A page whose content changed (words
    /// recognized on it), whose boxes didn't, lays nothing out: a layout
    /// holds no page's content, and the page's render key changes, so it's
    /// drawn again.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_mark_edit_lays_nothing_out() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(2);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let mut doc = Doc::new(model, cx.focus_handle(), Default::default());
                lay_out(&mut doc);
                let view = cx.new(|cx| {
                    let mut view = DocView::unopened(path.clone(), None, false, Instant::now(), cx);
                    view.state = State::Ready(Box::new(doc));
                    view
                });
                view.update(cx, |view, cx| {
                    let first = view.comment_view(cx);
                    let doc = shown(view);
                    let (layout, changes) = (doc.layout, doc.changes);
                    let page = doc.page_id(0).unwrap();
                    let rect = Box2 {
                        x: 72.0,
                        y: 72.0,
                        w: 100.0,
                        h: 50.0,
                    };
                    let square = view.new_annot(AnnotKind::Square { rect }, [200, 0, 0]);
                    assert!(view.add_annot(page, square, "Square", cx).is_some());
                    let doc = shown(view);
                    assert_eq!((doc.layout, doc.changes), (layout, changes + 1));
                    let pane = doc.panes[0].layout.as_ref();
                    assert!(pane.is_some_and(|l| l.layout == layout));
                    let second = view.comment_view(cx);
                    assert!(!Arc::ptr_eq(&first, &second));
                    assert!(first.types.is_empty() && !second.types.is_empty());
                    // A turned page goes elsewhere.
                    let rotate = Op::RotatePages {
                        pages: PageSel::ids([page]),
                        by: Turn::Quarter,
                    };
                    let tx = view.lower(vec![rotate], "Rotate", cx).unwrap();
                    view.commit(tx, CommitOpts::labelled("Rotate"), cx).unwrap();
                    let doc = shown(view);
                    assert_eq!((doc.layout, doc.changes), (layout + 1, changes + 2));
                    assert!(doc.panes[0].layout.is_none());
                    // The second page, its words recognized.
                    lay_out(doc);
                    let page = doc.page_id(1).unwrap();
                    let key = |doc: &Doc| {
                        let view = doc.views.view(doc.model.state(), page, &doc.inputs);
                        view.unwrap().key
                    };
                    let was = key(doc);
                    let words = recognized(doc, page);
                    let tx = view.lower(words, "Recognize", cx).unwrap();
                    let c = view.commit(tx, CommitOpts::labelled("Recognize"), cx);
                    let change = c.unwrap().change;
                    assert!(change.geometry.is_empty() && change.content.contains(&page));
                    let doc = shown(view);
                    assert_eq!(doc.layout, layout + 1);
                    assert!(
                        doc.panes[0]
                            .layout
                            .as_ref()
                            .is_some_and(|l| l.layout == layout + 1)
                    );
                    assert_ne!(key(doc), was);
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// What a change removes leaves the selection (M10.5 §14.1): a mark
    /// whose add is undone is no longer selected, nor a page deleted, its
    /// characters or the text selected on it; a mark added on another page
    /// leaves the marks selected as they were, and so does the delete. The
    /// characters read go by the pages' render keys, so a page deleted has
    /// none, and the page beside it, its mark drawn by GPUI, keeps its own.
    #[test]
    #[cfg(target_os = "linux")]
    fn what_a_change_removes_leaves_the_selection() {
        use crate::text_select::TextSel;
        use sheaf_doc::{TextOpts, TextService};
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(3);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let doc = Doc::new(model, cx.focus_handle(), Default::default());
                let view = cx.new(|cx| {
                    let mut view = DocView::unopened(path.clone(), None, false, Instant::now(), cx);
                    view.state = State::Ready(Box::new(doc));
                    view
                });
                view.update(cx, |view, cx| {
                    let pages: Vec<PageId> =
                        (0..3).map(|i| shown(view).page_id(i).unwrap()).collect();
                    let rect = Box2 {
                        x: 72.0,
                        y: 72.0,
                        w: 100.0,
                        h: 50.0,
                    };
                    let square = view.new_annot(AnnotKind::Square { rect }, [200, 0, 0]);
                    let first = view
                        .add_annot(pages[0], square.clone(), "Square", cx)
                        .unwrap();
                    let second = view
                        .add_annot(pages[0], square.clone(), "Square", cx)
                        .unwrap();
                    let doc = shown(view);
                    doc.selected_marks = vec![(pages[0], first), (pages[0], second)];
                    doc.selection = HashSet::from([pages[1], pages[2]]);
                    let snap = doc.model.snapshot();
                    let opts = TextOpts {
                        inputs: doc.inputs.clone(),
                    };
                    let chars: Vec<_> = (pages[1..].iter())
                        .map(|&p| TextService::chars(&doc.text, &snap, p, &opts).unwrap())
                        .collect();
                    doc.text_sel = Some(TextSel {
                        page: pages[1],
                        chars: chars[0].clone(),
                        anchor: 0,
                        head: 0,
                    });
                    view.undo(cx);
                    assert_eq!(shown(view).selected_marks, [(pages[0], first)]);
                    let mut commit = |ops: Vec<Op>, label: &str| {
                        let tx = view.lower(ops, label, cx).unwrap();
                        view.commit(tx, CommitOpts::labelled(label), cx).unwrap();
                    };
                    commit(
                        vec![sheaf_doc::tx::fixture::add(pages[2], &square)],
                        "Square",
                    );
                    commit(vec![sheaf_doc::tx::fixture::delete(&[pages[1]])], "Delete");
                    let doc = shown(view);
                    assert_eq!(doc.selected_marks, [(pages[0], first)]);
                    assert_eq!(doc.selection, HashSet::from([pages[2]]));
                    assert!(doc.text_sel.is_none());
                    assert!(view.cached_chars(pages[1]).is_none());
                    assert!(view.cached_chars(pages[2]).is_some());
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Each change the document goes through reaches what follows it once
    /// (M10.5 §14.1): a commit, a commit and an amend in a plugin call's
    /// group, an undo, a redo, a jump, the end of a save and an "Undo all
    /// of …" each give one `Changed`, with its cause, and the file's facts
    /// coming in give `Opened` alone. Each step runs in an update of its
    /// own, after which what it emitted has been heard, and plugin calls
    /// read the document as it is then (§14.3).
    #[test]
    #[cfg(target_os = "linux")]
    fn each_transition_is_one_change() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::atomic::{AtomicBool, Ordering};
        /// `f` on the view, and what listeners heard of it.
        fn step(
            cx: &mut gpui::AsyncApp,
            view: &Entity<DocView>,
            heard: &RefCell<Vec<String>>,
            f: impl FnOnce(&mut DocView, &mut Context<DocView>),
        ) -> Vec<String> {
            cx.update(|cx| view.update(cx, f));
            #[cfg(feature = "plugins")]
            cx.update(|cx| {
                view.update(cx, |v, _| {
                    let doc = shown(v);
                    let read = doc.plugin_cell.load().snap.version;
                    assert_eq!(read, doc.model.current_version());
                })
            });
            heard.take()
        }
        let (_dir, path) = blank_file(2);
        // The model as the view makes it, before the file's facts are in.
        let store = Store::new();
        let (primary, stamp) = store.read_file(&path, None).unwrap();
        let (src, blob) = (primary.clone(), primary.id());
        let came = Came::Read {
            path: path.as_path().into(),
            stamp,
        };
        let name = sheaf_doc::store::file_name(&path);
        let model = opening_model(store, primary, name, Some(came), Identity::default());
        let facts = sheaf_doc::open::finish(&src, &sheaf_ops::NoProgress);
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let doc = Doc::new(model, cx.focus_handle(), Default::default());
                let view = cx.new(|cx| {
                    let mut view = DocView::unopened(path.clone(), None, false, Instant::now(), cx);
                    view.state = State::Ready(Box::new(doc));
                    view
                });
                let heard = Rc::new(RefCell::new(Vec::new()));
                cx.subscribe(&view, {
                    let heard = heard.clone();
                    move |_, ev: &DocEvent, _| {
                        let what = match ev {
                            DocEvent::Opened => "Opened".to_string(),
                            DocEvent::Changed { change } => format!("{:?}", change.cause),
                            _ => return,
                        };
                        heard.borrow_mut().push(what);
                    }
                })
                .detach();
                cx.spawn(async move |cx| {
                    let ready = |v: &mut DocView, cx: &mut Context<DocView>| {
                        v.ready(blob, facts, Instant::now(), false, cx)
                    };
                    assert_eq!(step(cx, &view, &heard, ready), ["Opened"]);
                    let version = |cx: &mut gpui::AsyncApp| {
                        cx.update(|cx| view.update(cx, |v, _| shown(v).model.current_version()))
                    };
                    let opened = version(cx);
                    let turn = || Op::RotatePages {
                        pages: PageSel::All,
                        by: Turn::Quarter,
                    };
                    let rotate = |v: &mut DocView, cx: &mut Context<DocView>| {
                        let tx = v.lower(vec![turn()], "Rotate", cx).unwrap();
                        v.commit(tx, CommitOpts::labelled("Rotate"), cx).unwrap();
                    };
                    assert_eq!(step(cx, &view, &heard, rotate), ["Commit"]);
                    // A plugin call's batches: the first makes a version,
                    // the next joins it.
                    let plugin = sheaf_doc::GroupKind::PluginCall { plugin: "p".into() };
                    let begin = |v: &mut DocView, _: &mut Context<DocView>| {
                        shown(v).model.begin_group(plugin)
                    };
                    let g = cx.update(|cx| view.update(cx, begin));
                    let batch = |v: &mut DocView, cx: &mut Context<DocView>| {
                        let by = Actor::Plugin {
                            id: "p".into(),
                            version: "1".into(),
                        };
                        let tx = shown(v).model.tx(by);
                        let tx = v.lower_in(tx, vec![turn()], "Plugin", cx).unwrap();
                        let opts = CommitOpts {
                            group: Some(g),
                            ..CommitOpts::labelled("Plugin")
                        };
                        v.commit(tx, opts, cx).unwrap();
                    };
                    assert_eq!(step(cx, &view, &heard, batch), ["Commit"]);
                    assert_eq!(step(cx, &view, &heard, batch), ["Amend"]);
                    let call = version(cx);
                    let undo = |v: &mut DocView, cx: &mut Context<DocView>| v.undo(cx);
                    assert_eq!(step(cx, &view, &heard, undo), ["Undo"]);
                    let redo = |v: &mut DocView, cx: &mut Context<DocView>| v.redo(cx);
                    assert_eq!(step(cx, &view, &heard, redo), ["Redo"]);
                    for to in [opened, call] {
                        let jump = |v: &mut DocView, cx: &mut Context<DocView>| v.jump_to(to, cx);
                        assert_eq!(step(cx, &view, &heard, jump), ["Jump"]);
                    }
                    let save = |v: &mut DocView, cx: &mut Context<DocView>| {
                        let (saving, written) = written(shown(v), &path);
                        assert!(v.finish_save(saving, written, cx));
                    };
                    assert_eq!(step(cx, &view, &heard, save), ["Saved"]);
                    let revert = |v: &mut DocView, cx: &mut Context<DocView>| {
                        v.undo_group(g, "Plugin".into(), cx);
                        let notice = shown(v).notice.as_deref();
                        assert_eq!(notice, Some("Undid all of Plugin"));
                    };
                    assert_eq!(step(cx, &view, &heard, revert), ["Revert"]);
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// `files` combined into `out` as the Combine dialog combines them,
    /// then shown as the workspace shows what it made (`adopt`), in a
    /// headless app: the view shows the document Combine made, nothing
    /// read again (its store and its version), at its one version, the
    /// saved file's, with nothing to undo, every field named apart, what
    /// Combine said as its notice, and its scripts as Combine left them;
    /// autosave and recovery follow its file, the saved one, and
    /// listeners hear it opened. What Combine said.
    #[cfg(target_os = "linux")]
    fn shown_as_combined(files: &[std::path::PathBuf], out: &std::path::Path) -> Option<String> {
        use sheaf_ops::combine::{CombineItem, CombineParams};
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let items: Vec<CombineItem> = files.iter().map(CombineItem::new).collect();
        let answers = sheaf_doc::export::PRINT_ANSWERS;
        let (session, report) = sheaf_ops::combine::combine(
            &items,
            &CombineParams::default(),
            out,
            &answers,
            &sheaf_ops::NoProgress,
        )
        .unwrap();
        let (store, version) = (session.doc.store().clone(), session.doc.current_version());
        let scripts = session.doc.state().form().scripts.clone();
        let st = session.doc.state();
        let names: Vec<String> = (st.terminals())
            .map(|(id, _, _)| st.full_name(id).unwrap())
            .collect();
        let apart: HashSet<&String> = names.iter().collect();
        assert_eq!(apart.len(), names.len(), "{names:?}");
        let notice = report.notice.clone();
        let out = out.to_path_buf();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let (ran, notice) = (ran.clone(), notice.clone());
            move |cx| {
                let view = cx.new(|cx| {
                    let who = Identity::default();
                    let (scale, at) = (Default::default(), Instant::now());
                    DocView::adopting(
                        out.clone(),
                        session,
                        notice.clone(),
                        who,
                        scale,
                        false,
                        at,
                        cx,
                    )
                });
                let heard = Rc::new(RefCell::new(Vec::new()));
                cx.subscribe(&view, {
                    let heard = heard.clone();
                    move |_, ev: &DocEvent, _| heard.borrow_mut().push(ev.clone())
                })
                .detach();
                cx.spawn(async move |cx| {
                    cx.update(|cx| view.update(cx, |v, cx| v.adopted(cx)));
                    assert_eq!(heard.take(), [DocEvent::Opened]);
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            assert_eq!(v.path, out);
                            let doc = shown(v);
                            let model = &doc.model;
                            assert!(Arc::ptr_eq(model.store(), &store), "the same document");
                            assert_eq!(model.current_version(), version);
                            assert!(model.is_ready() && doc.reading.is_none());
                            assert_eq!(model.version_count(), 1);
                            assert!(!model.can_undo() && !model.is_dirty());
                            let file = model.file().map(|(file, _)| file.to_path_buf());
                            assert_eq!(file, Some(out.clone()));
                            assert_eq!(doc.notice, notice);
                            assert_eq!(model.state().form().scripts, scripts);
                        })
                    });
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
        report.notice
    }

    /// Combine hands the document it made to a window as it is (M10.5
    /// §14.1): two forms whose fields have one name and each a document
    /// script, the second's fields put under a parent of their own and
    /// the scripts turned off, which the window says; and the 1040 with
    /// Schedules 1 and B from Greg's corpus (skipped without it), the
    /// schedules renamed for their files and their scripts on.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_combined_document_opens_in_its_window_as_made() {
        use lopdf::{Object, dictionary};
        use sheaf_doc::forms::fixture::{Form, row};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let form = |name: &str| {
            let mut f = Form::new(1);
            f.text("name", 0, row(0));
            let script = f.pdf.add_object(dictionary! {
                "S" => "JavaScript", "JS" => Object::string_literal("var v;"),
            });
            let names = vec![Object::string_literal("Init"), script.into()];
            let catalog = f.catalog;
            f.dict(catalog).set(
                "Names",
                dictionary! { "JavaScript" => dictionary! { "Names" => names } },
            );
            let path = dir.path().join(format!("{name}.pdf"));
            std::fs::write(&path, f.bytes()).unwrap();
            path
        };
        let files = [form("a"), form("b")];
        let said = shown_as_combined(&files, &dir.path().join("combined.pdf"));
        let want = "Fields from b.pdf are under “b”, since their names were taken; its scripts \
                    still use the old names. Scripts are off: fields from more than one form \
                    have them";
        assert_eq!(said.as_deref(), Some(want));

        let Some(corpus) = sheaf_doc::forms::corpus::Corpus::or_skip() else {
            return;
        };
        let files = ["f1040", "f1040s1", "f1040sb"].map(|name| {
            let path = format!("forms/irs/{name}.pdf");
            corpus.path(&corpus.entry(&path).unwrap().expect("in the manifest"))
        });
        let said = shown_as_combined(&files, &dir.path().join("1040.pdf"));
        let renamed = |file: &str| {
            format!("Fields from {file}.pdf are under “{file}”, since their names were taken")
        };
        let want = [renamed("f1040s1"), renamed("f1040sb")].join(". ");
        assert_eq!(said, Some(want));
    }

    /// A view showing `model`, the document of the file at `path`.
    #[cfg(target_os = "linux")]
    fn view_of(model: Document, path: &std::path::Path, cx: &mut App) -> Entity<DocView> {
        let doc = Doc::new(model, cx.focus_handle(), Default::default());
        cx.new(|cx| {
            let mut view = DocView::unopened(path.into(), None, false, Instant::now(), cx);
            view.state = State::Ready(Box::new(doc));
            view
        })
    }

    /// An insert of every page of `file` before `before` into `doc`'s
    /// document, planned as its planner plans it, on the version it's at.
    #[cfg(target_os = "linux")]
    fn insert_planned(
        doc: &Doc,
        file: &std::path::Path,
        before: Option<PageId>,
    ) -> crate::page_insert::PlannedInsert {
        let snap = doc.model.snapshot();
        let lower = doc.model.lower_cx(Actor::Job(sheaf_doc::JobKind::Insert));
        let store = doc.model.store();
        let p = &sheaf_ops::NoProgress;
        crate::page_insert::plan_insert(&snap, store, lower, file, before, true, p).unwrap()
    }

    /// `doc`'s pages, in order.
    #[cfg(target_os = "linux")]
    fn order(doc: &Doc) -> Vec<PageId> {
        doc.model.state().order().iter().copied().collect()
    }

    /// The op that gives page `page` of `doc` a word recognized on it: a
    /// change to what the page draws, its boxes as they were.
    #[cfg(target_os = "linux")]
    fn recognized(doc: &Doc, page: PageId) -> Vec<Op> {
        let word = sheaf_doc::OcrWord {
            text: "Total".into(),
            rect: Box2 {
                x: 72.0,
                y: 600.0,
                w: 50.0,
                h: 14.0,
            },
            confidence: 0.9,
        };
        let geom = doc.model.geom(page);
        let layer = sheaf_doc::text_layer::from_engine(vec![word], &geom, "test");
        sheaf_doc::tx::fixture::ocr([(page, Arc::new(layer))])
    }

    /// An insert planned before a page that's deleted before it lands
    /// says why, and changes nothing (M10.5 §8.5): lowered again on the
    /// current version, its anchor names no page, so the version and the
    /// pages stay as they were, and nothing keeps the file it read: the
    /// store lets it go.
    #[test]
    #[cfg(target_os = "linux")]
    fn an_insert_whose_anchor_went_says_so_and_changes_nothing() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(3);
        let (_from, file) = blank_file(2);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    let doc = shown(view);
                    let anchor = doc.page_id(1).unwrap();
                    let plan = insert_planned(doc, &file, Some(anchor));
                    let src = plan.src().id();
                    let delete = Op::DeletePages {
                        pages: PageSel::ids([anchor]),
                    };
                    let tx = view.lower(vec![delete], "Delete page", cx).unwrap();
                    view.commit(tx, CommitOpts::labelled("Delete page"), cx)
                        .unwrap();
                    let doc = shown(view);
                    let was = (doc.model.current_version(), order(doc));
                    assert!(view.land(plan, "Insert blank.pdf", cx).is_none());
                    let doc = shown(view);
                    assert_eq!((doc.model.current_version(), order(doc)), was);
                    assert!(doc.model.store().get(src).is_none());
                    let notice = doc.notice.as_deref().unwrap_or_default();
                    assert!(notice.starts_with("Insert blank.pdf failed: "), "{notice}");
                    assert!(notice.ends_with("isn't there"), "{notice}");
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// An insert planned while the user moves a page elsewhere lands too:
    /// lowered again on the moved pages, before the page it was planned
    /// before, and what it made keeps the ids its plan drew (M10.5 §8.5).
    /// The document keeps the file, whose pages draw from its own engine,
    /// which the render worker opens when one is drawn.
    #[test]
    #[cfg(target_os = "linux")]
    fn an_insert_during_a_move_lands_both() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(3);
        let (_from, file) = blank_file(2);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    let doc = shown(view);
                    let [a, b, c] = std::array::from_fn(|i| doc.page_id(i).unwrap());
                    let plan = insert_planned(doc, &file, Some(b));
                    let (src, planned) = (plan.src().id(), plan.created());
                    let to_end = Op::MovePages {
                        pages: PageSel::ids([a]),
                        before: None,
                    };
                    let tx = view.lower(vec![to_end], "Move page", cx).unwrap();
                    view.commit(tx, CommitOpts::labelled("Move page"), cx)
                        .unwrap();
                    let commit = view.land(plan, "Insert blank.pdf", cx).unwrap();
                    assert_eq!(commit.created, planned);
                    let made: Vec<PageId> = commit.created.pages().collect();
                    let doc = shown(view);
                    assert_eq!(order(doc), [made[0], made[1], b, c, a]);
                    assert!(doc.model.store().get(src).is_some());
                    let view = doc.views.view(doc.model.state(), made[0], &doc.inputs);
                    assert_eq!(view.unwrap().engine, EngineKey::Base(src));
                    assert_eq!(doc.model.undo_label(), Some("Insert blank.pdf"));
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// An image's page, planned while the user moves a page, lands too,
    /// lowered again on the moved pages: made in memory again, the same
    /// bytes and so the same blob, it keeps the ids its plan drew (M10.5
    /// §3.9, §8.5). Once it's landed, nothing holds the image.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_made_page_lands_lowered_again() {
        use sheaf_doc::store::{BlobKind, MemoryKind, Origin};
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(3);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    let doc = shown(view);
                    let [a, b, c] = std::array::from_fn(|i| doc.page_id(i).unwrap());
                    let red = image::RgbImage::from_pixel(40, 20, image::Rgb([200, 30, 30]));
                    let png = sheaf_image::encode::png(&red, 72).unwrap();
                    let kind = BlobKind::image(&png).unwrap();
                    let image = doc.model.store().intern(png.into(), kind).unwrap();
                    let from = sheaf_edit::PageSource::Images {
                        images: vec![sheaf_edit::BlobSel::Id(image.id())],
                        fit: sheaf_edit::Fit::LETTER,
                    };
                    let (snap, lower) = (doc.model.snapshot(), doc.model.lower_cx(Actor::User));
                    let (p, held) = (&sheaf_ops::NoProgress, image.id());
                    let plan =
                        crate::page_insert::plan_made(&snap, lower, from, vec![image], Some(b), p);
                    let plan = plan.unwrap();
                    let made_as = match plan.src().blob().kind() {
                        BlobKind::Pdf { origin, .. } => origin.clone(),
                        _ => panic!("a PDF"),
                    };
                    assert_eq!(made_as, Origin::Memory(MemoryKind::Images));
                    let (src, planned) = (plan.src().id(), plan.created());
                    let to_end = Op::MovePages {
                        pages: PageSel::ids([a]),
                        before: None,
                    };
                    let tx = view.lower(vec![to_end], "Move page", cx).unwrap();
                    view.commit(tx, CommitOpts::labelled("Move page"), cx)
                        .unwrap();
                    let commit = view.land(plan, "Insert image", cx).unwrap();
                    assert_eq!(commit.created, planned);
                    let made: Vec<PageId> = commit.created.pages().collect();
                    let doc = shown(view);
                    assert_eq!(order(doc), [made[0], b, c, a]);
                    let def = doc.model.state().def(made[0]).unwrap();
                    assert_eq!(def.base.src.id(), src);
                    assert!(
                        doc.model.store().get(held).is_none(),
                        "the image isn't held"
                    );
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Page labels from their dialog land on the version it opened on
    /// (M10.5 §8.5): after a page was turned meanwhile they land, the turn
    /// kept; after the labels changed meanwhile it's a conflict, which the
    /// notice says, and nothing changes.
    #[test]
    #[cfg(target_os = "linux")]
    fn labels_land_on_the_version_their_dialog_opened_on() {
        use sheaf_doc::labels::Style;
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(3);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    let opened = view.model_snapshot().unwrap();
                    let last = shown(view).page_id(2).unwrap();
                    let turn = Op::RotatePages {
                        pages: PageSel::ids([last]),
                        by: Turn::Quarter,
                    };
                    let tx = view.lower(vec![turn], "Rotate", cx).unwrap();
                    view.commit(tx, CommitOpts::labelled("Rotate"), cx).unwrap();
                    let turned = shown(view).model.state().geom(last).cloned();
                    let roman = (0, 1, Style::LowerRoman, String::new(), 1);
                    view.set_page_labels(opened.clone(), Some(roman), cx);
                    let doc = shown(view);
                    assert_eq!(doc.model.undo_label(), Some("Page labels"));
                    let labels = doc.model.state().page_labels().unwrap();
                    assert_eq!(labels, ["i", "ii", "3"]);
                    assert_eq!(doc.model.state().geom(last).cloned(), turned);
                    let was = doc.model.current_version();
                    let lettered = (0, 2, Style::UpperLetters, String::new(), 1);
                    view.set_page_labels(opened, Some(lettered), cx);
                    let doc = shown(view);
                    assert_eq!(doc.model.current_version(), was);
                    let notice = doc.notice.as_deref().unwrap_or_default();
                    let conflict = "Page labels failed: “Page labels” changed the same data";
                    assert_eq!(notice, conflict);
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Blank review deletes the pages it shows checked on the version its
    /// search measured (M10.5 §8.5): one moved since is deleted all the
    /// same, the delete lowered again over the move; one changed since
    /// (words recognized on it, in the page's slot the search's read names),
    /// not the page the search measured, is a conflict, which the notice
    /// says, and nothing changes.
    #[test]
    #[cfg(target_os = "linux")]
    fn blank_review_deletes_what_its_search_measured() {
        use sheaf_ops::blank::{Candidate, Reason};
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(4);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    let doc = shown(view);
                    let [a, b, c, d] = std::array::from_fn(|i| doc.page_id(i).unwrap());
                    let planning = doc.model.snapshot();
                    let review = |checked: &[PageId]| crate::blank_review::Review {
                        planning: planning.clone(),
                        candidates: (checked.iter().enumerate())
                            .map(|(index, &id)| Candidate {
                                index,
                                id,
                                ink: 0.0,
                                reason: Reason::Blank,
                            })
                            .collect(),
                        checked: checked.iter().copied().collect(),
                        scroll: ScrollHandle::new(),
                    };
                    let to_end = Op::MovePages {
                        pages: PageSel::ids([a]),
                        before: None,
                    };
                    let tx = view.lower(vec![to_end], "Move page", cx).unwrap();
                    view.commit(tx, CommitOpts::labelled("Move page"), cx)
                        .unwrap();
                    view.blank_review = Some(review(&[a, c]));
                    view.delete_reviewed(cx);
                    let doc = shown(view);
                    assert_eq!(order(doc), [b, d]);
                    assert_eq!(doc.model.undo_label(), Some("Delete 2 blank pages"));
                    let words = recognized(doc, b);
                    let tx = view.lower(words, "Recognize", cx).unwrap();
                    view.commit(tx, CommitOpts::labelled("Recognize"), cx)
                        .unwrap();
                    let was = shown(view).model.current_version();
                    view.blank_review = Some(review(&[b]));
                    view.delete_reviewed(cx);
                    let doc = shown(view);
                    assert_eq!((doc.model.current_version(), order(doc)), (was, vec![b, d]));
                    let notice = doc.notice.as_deref().unwrap_or_default();
                    let conflict = "Delete 1 blank page failed: “Recognize” changed the same data";
                    assert_eq!(notice, conflict);
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Planners run side by side, each landing as it ends (jobs.rs), and
    /// Escape stops the newest, which then lands nothing: one whose work
    /// stops, and one whose work doesn't look and ends as if done.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_stopped_planner_lands_nothing() {
        use std::cell::Cell;
        use std::rc::Rc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(3);
        let (_from, file) = blank_file(2);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                let landed = Rc::new(Cell::new(false));
                view.update(cx, |v, cx| {
                    v.insert_file(file.clone(), None, "first".into(), false, cx);
                    v.insert_file(file, None, "second".into(), false, cx);
                    assert!(v.cancel_planner(cx));
                    let lands = {
                        let landed = landed.clone();
                        move |_: &mut DocView, (), _: &mut Context<DocView>| {
                            landed.set(true);
                            None
                        }
                    };
                    v.run_planner("third", |_| anyhow::Ok(()), |_| None, lands, cx);
                    assert!(v.cancel_planner(cx));
                    let stopped: Vec<bool> = (v.planners.iter())
                        .map(|p| p.tracker.snapshot().cancelled)
                        .collect();
                    assert_eq!(stopped, [false, true, true]);
                });
                cx.spawn(async move |cx| {
                    let running = |cx: &mut gpui::AsyncApp| {
                        cx.update(|cx| !view.read(cx).planners.is_empty())
                    };
                    for _ in 0..1000 {
                        if !running(cx) {
                            break;
                        }
                        let tick = std::time::Duration::from_millis(10);
                        cx.background_executor().timer(tick).await;
                    }
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            assert!(v.planners.is_empty());
                            let doc = shown(v);
                            assert_eq!(doc.model.page_count(), 5);
                            assert_eq!(doc.model.undo_label(), Some("Insert first"));
                        })
                    });
                    assert!(!landed.get());
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// The page each page of `doc` shows: a page shown from a copy made of
    /// it would show another.
    #[cfg(target_os = "linux")]
    fn bases(doc: &Doc) -> Vec<sheaf_doc::PageBase> {
        let st = doc.model.state();
        (st.order().iter())
            .map(|&p| st.def(p).unwrap().base.clone())
            .collect()
    }

    /// A watermark set from its dialog is one op committed on the version
    /// the dialog opened on (M10.5 slice 16): one undo step called
    /// "Watermark", or "Background", the notice its line, with text the
    /// built-in fonts can't draw said; the pages draw it with no copy made
    /// of them, and an undo takes it off. Set again as it is, it says so.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_watermark_is_one_undo_step() {
        use sheaf_doc::tx::fixture::{set_watermark, watermark};
        use sheaf_edit::Z;
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(2);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    let was = bases(shown(view));
                    let opened = view.model_snapshot().unwrap();
                    let set = set_watermark(watermark("DRAFT"), Z::Over, None);
                    view.set_watermark(opened, set, None, cx);
                    let doc = shown(view);
                    assert_eq!(doc.model.undo_label(), Some("Watermark"));
                    let said = doc.notice.as_deref();
                    assert_eq!(said, Some("Add a watermark to all pages"));
                    assert!(doc.model.state().watermark(Z::Over).is_some());
                    assert_eq!(bases(doc), was, "no copy");
                    let opened = view.model_snapshot().unwrap();
                    let page = opened.state.page_at(1).unwrap();
                    let set = set_watermark(watermark("機密"), Z::Under, Some(&[page]));
                    view.set_watermark(opened, set, None, cx);
                    let doc = shown(view);
                    assert_eq!(doc.model.undo_label(), Some("Background"));
                    let said = doc.notice.as_deref().unwrap_or_default();
                    let unencodable = "Add a background to page 2. Some characters can't be \
                        drawn in the built-in fonts and show as “?” in the file; the text \
                        itself is kept";
                    assert_eq!(said, unencodable);
                    view.undo(cx);
                    let doc = shown(view);
                    assert!(doc.model.state().watermark(Z::Under).is_none());
                    assert_eq!(doc.model.undo_label(), Some("Watermark"));
                    let now = doc.model.current_version();
                    let opened = view.model_snapshot().unwrap();
                    let set = set_watermark(watermark("DRAFT"), Z::Over, None);
                    view.set_watermark(opened, set, None, cx);
                    let doc = shown(view);
                    assert_eq!(doc.model.current_version(), now);
                    let said = doc.notice.as_deref();
                    assert_eq!(said, Some("No change to the watermark"));
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Remove Watermark is a planner (M10.5 §6.5, D61): it lands once its
    /// lowering has read the pages' content off the UI thread, as one undo
    /// step on the version its dialog opened on, taking the document's
    /// watermark off and leaving the author's own, which its notice
    /// counts; with nothing of sheaf's or others' to take off, it says so.
    #[test]
    #[cfg(target_os = "linux")]
    fn remove_watermark_is_planned_and_counts_the_author_s() {
        use lopdf::{Document as Pdf, Object, Stream, dictionary};
        use sheaf_doc::tx::fixture::{set_watermark, watermark};
        use sheaf_edit::Z;
        use std::sync::atomic::{AtomicBool, Ordering};
        // Two Letter pages, the first with a watermark of the author's,
        // as Word writes one.
        let mut pdf = Pdf::with_version("1.7");
        let root = pdf.new_object_id();
        let drawn: [&[u8]; 2] = [
            b"/Artifact <</Type /Pagination /Subtype /Watermark>> BDC \
              0 0 1 rg 100 100 50 50 re f EMC",
            b"0 g 300 300 10 10 re f",
        ];
        let kids: Vec<Object> = (drawn.into_iter())
            .map(|content| {
                let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
                let page =
                    dictionary! { "Type" => "Page", "Parent" => root, "Contents" => content };
                pdf.add_object(page).into()
            })
            .collect();
        let pages = dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => 2,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        };
        pdf.objects.insert(root, Object::Dictionary(pages));
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        pdf.trailer.set("Root", catalog);
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("authored.pdf");
        pdf.save(&path).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |v, cx| {
                    let opened = v.model_snapshot().unwrap();
                    let set = set_watermark(watermark("DRAFT"), Z::Over, None);
                    v.set_watermark(opened, set, None, cx);
                    let opened = v.model_snapshot().unwrap();
                    v.remove_watermark(opened, Z::Over, false, cx);
                    assert_eq!(v.planners.len(), 1, "a planner");
                    let st = shown(v).model.state();
                    assert!(st.watermark(Z::Over).is_some(), "not landed yet");
                });
                cx.spawn(async move |cx| {
                    planned(&view, cx).await;
                    cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            let doc = shown(v);
                            let said = "Remove the watermark from pages 1, 2 \
                                (1 the author's left in place)";
                            assert_eq!(doc.notice.as_deref(), Some(said));
                            assert_eq!(doc.model.undo_label(), Some("Remove watermark"));
                            assert!(doc.model.state().watermark(Z::Over).is_none());
                            let opened = v.model_snapshot().unwrap();
                            v.remove_watermark(opened, Z::Under, false, cx);
                        })
                    });
                    planned(&view, cx).await;
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            let doc = shown(v);
                            let said = doc.notice.as_deref();
                            assert_eq!(said, Some("No background to remove"));
                            assert_eq!(doc.model.undo_label(), Some("Remove watermark"));
                        })
                    });
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Document > Remove Recognized Text…, answered: the text comes off
    /// the pages chosen, as a planner, in one undo step called "Remove
    /// recognized text", the notice its line; an undo puts it back. Every
    /// page then, and again, when there's none left, it says so.
    #[test]
    #[cfg(target_os = "linux")]
    fn removing_recognized_text_is_one_undo_step() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(3);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let texts = |v: &mut DocView| -> Vec<bool> {
            let doc = shown(v);
            let st = doc.model.state();
            (st.order().iter())
                .map(|&p| st.ocr_text(p).is_some())
                .collect()
        };
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |v, cx| {
                    let doc = shown(v);
                    let pages = order(doc);
                    let ops = (pages.iter()).flat_map(|&p| recognized(doc, p)).collect();
                    let tx = v.lower(ops, "OCR", cx).unwrap();
                    v.commit(tx, CommitOpts::labelled("OCR"), cx).unwrap();
                    assert_eq!(texts(v), [true; 3]);
                    let opened = v.model_snapshot().unwrap();
                    v.remove_recognized_text(opened, vec![pages[1]], cx);
                    assert_eq!(v.planners.len(), 1, "a planner");
                    assert_eq!(texts(v), [true; 3], "not landed yet");
                });
                cx.spawn(async move |cx| {
                    planned(&view, cx).await;
                    cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            assert_eq!(texts(v), [true, false, true]);
                            let doc = shown(v);
                            let said = doc.notice.as_deref();
                            assert_eq!(said, Some("Remove recognized text from page 2"));
                            assert_eq!(doc.model.undo_label(), Some("Remove recognized text"));
                            v.undo(cx);
                            assert_eq!(texts(v), [true; 3], "one undo puts it back");
                            let opened = v.model_snapshot().unwrap();
                            let all = order(shown(v));
                            v.remove_recognized_text(opened, all, cx);
                        })
                    });
                    planned(&view, cx).await;
                    cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            assert_eq!(texts(v), [false; 3]);
                            let said = shown(v).notice.as_deref();
                            assert_eq!(said, Some("Remove recognized text from pages 1–3"));
                            let opened = v.model_snapshot().unwrap();
                            let all = order(shown(v));
                            v.remove_recognized_text(opened, all, cx);
                        })
                    });
                    planned(&view, cx).await;
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            let doc = shown(v);
                            let said = doc.notice.as_deref();
                            assert_eq!(said, Some("No recognized text to remove"));
                            assert_eq!(doc.model.undo_label(), Some("Remove recognized text"));
                        })
                    });
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Removing recognized text from a page whose only layer is an older
    /// sheaf's that lost its `q` changes nothing, and the notice says the
    /// layer stays.
    #[test]
    #[cfg(target_os = "linux")]
    fn removing_recognized_text_says_what_stays() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let mut pdf = sheaf_doc::text_layer::fixture::legacy_page();
        let id = pdf.get_pages()[&1];
        let page = pdf.get_dictionary_mut(id).unwrap();
        let contents = page.get(b"Contents").unwrap().as_array().unwrap()[1..].to_vec();
        page.set("Contents", contents);
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("legacy.pdf");
        pdf.save(&path).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                let version = view.update(cx, |v, cx| {
                    let opened = v.model_snapshot().unwrap();
                    let version = opened.version;
                    let all = order(shown(v));
                    v.remove_recognized_text(opened, all, cx);
                    version
                });
                cx.spawn(async move |cx| {
                    planned(&view, cx).await;
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            let doc = shown(v);
                            assert_eq!(doc.model.current_version(), version);
                            let said = "No recognized text to remove. Recognized text sheaf \
                                        wrote earlier stays on page 1: taking it out would \
                                        change what else the page draws";
                            assert_eq!(doc.notice.as_deref(), Some(said));
                        })
                    });
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// A header and footer set from its dialog is one op committed on the
    /// version the dialog opened on (M10.5 slice 17): one undo step called
    /// "Header & footer", the notice its line, with text the built-in fonts
    /// can't draw said; the pages draw it with no copy made of them, and an
    /// undo puts back the one before. Set again as it is, it says so.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_header_is_one_undo_step() {
        use sheaf_doc::tx::fixture::{header, set_header};
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(2);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    let was = bases(shown(view));
                    let opened = view.model_snapshot().unwrap();
                    let set = set_header(header("Page <<1>> of <<N>>"), None);
                    view.set_header_footer(opened, set, cx);
                    let doc = shown(view);
                    assert_eq!(doc.model.undo_label(), Some("Header & footer"));
                    let said = doc.notice.as_deref();
                    assert_eq!(said, Some("Set the header and footer"));
                    let first = doc.model.state().header_footer().cloned();
                    assert!(first.is_some());
                    assert_eq!(bases(doc), was, "no copy");
                    let opened = view.model_snapshot().unwrap();
                    let page = opened.state.page_at(1).unwrap();
                    let set = set_header(header("機密"), Some(&[page]));
                    view.set_header_footer(opened, set, cx);
                    let doc = shown(view);
                    assert_eq!(doc.model.undo_label(), Some("Header & footer"));
                    let said = doc.notice.as_deref().unwrap_or_default();
                    let unencodable = "Set the header and footer. Some characters can't be \
                        drawn in the built-in fonts and show as “?” in the file; the text \
                        itself is kept";
                    assert_eq!(said, unencodable);
                    view.undo(cx);
                    let doc = shown(view);
                    assert_eq!(doc.model.state().header_footer().cloned(), first);
                    let now = doc.model.current_version();
                    let opened = view.model_snapshot().unwrap();
                    let set = set_header(header("Page <<1>> of <<N>>"), None);
                    view.set_header_footer(opened, set, cx);
                    let doc = shown(view);
                    assert_eq!(doc.model.current_version(), now);
                    let said = doc.notice.as_deref();
                    assert_eq!(said, Some("No change to the header and footer"));
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Remove Header & Footer is a planner (M10.5 §6.5, D61), as Remove
    /// Watermark is: it lands once its lowering has read the pages' content
    /// off the UI thread, as one undo step on the version its dialog opened
    /// on, taking the document's header and footer off and leaving the
    /// author's running head, which its notice counts; asked to, it takes
    /// that too; and with nothing of sheaf's or others' left to take off,
    /// it says so.
    #[test]
    #[cfg(target_os = "linux")]
    fn remove_header_footer_is_planned_and_counts_the_author_s() {
        use lopdf::{Document as Pdf, Object, Stream, dictionary};
        use sheaf_doc::tx::fixture::{header, set_header};
        use std::sync::atomic::{AtomicBool, Ordering};
        // Two Letter pages, the first with a running head of the author's,
        // in a `q` of its own.
        let mut pdf = Pdf::with_version("1.7");
        let root = pdf.new_object_id();
        let font = pdf.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let drawn: [&[u8]; 2] = [
            b"/Artifact <</Type /Pagination /Subtype /Header>> BDC \
              q BT /F1 9 Tf 72 760 Td (Chapter One) Tj ET Q EMC",
            b"0 g 300 300 10 10 re f",
        ];
        let kids: Vec<Object> = (drawn.into_iter())
            .map(|content| {
                let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
                let page =
                    dictionary! { "Type" => "Page", "Parent" => root, "Contents" => content };
                pdf.add_object(page).into()
            })
            .collect();
        let pages = dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => 2,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        };
        pdf.objects.insert(root, Object::Dictionary(pages));
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        pdf.trailer.set("Root", catalog);
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("authored.pdf");
        pdf.save(&path).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |v, cx| {
                    let opened = v.model_snapshot().unwrap();
                    let set = set_header(header("Page <<1>>"), None);
                    v.set_header_footer(opened, set, cx);
                    let opened = v.model_snapshot().unwrap();
                    v.remove_header_footer(opened, false, cx);
                    assert_eq!(v.planners.len(), 1, "a planner");
                    let st = shown(v).model.state();
                    assert!(st.header_footer().is_some(), "not landed yet");
                });
                cx.spawn(async move |cx| {
                    planned(&view, cx).await;
                    cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            let doc = shown(v);
                            let said = "Remove the header and footer from pages 1, 2 \
                                (1 the author's left in place)";
                            assert_eq!(doc.notice.as_deref(), Some(said));
                            let label = Some("Remove header & footer");
                            assert_eq!(doc.model.undo_label(), label);
                            assert!(doc.model.state().header_footer().is_none());
                            let opened = v.model_snapshot().unwrap();
                            v.remove_header_footer(opened, true, cx);
                        })
                    });
                    planned(&view, cx).await;
                    cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            let doc = shown(v);
                            let said = "Remove the header and footer from pages 1, 2 \
                                (1 the author's)";
                            assert_eq!(doc.notice.as_deref(), Some(said));
                            let opened = v.model_snapshot().unwrap();
                            v.remove_header_footer(opened, true, cx);
                        })
                    });
                    planned(&view, cx).await;
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            let doc = shown(v);
                            let said = doc.notice.as_deref();
                            assert_eq!(said, Some("No header or footer to remove"));
                            let label = Some("Remove header & footer");
                            assert_eq!(doc.model.undo_label(), label);
                        })
                    });
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// `doc`'s first pane planned as a frame 800 by 600 px plans it, at a
    /// device px a px, and its images let go as the frame lets them go.
    #[cfg(target_os = "linux")]
    fn frame(doc: &mut Doc) -> PaneFrame {
        let frame = plan_pane(doc, 0, (800.0, 600.0), true, 1.0);
        evict(doc, std::slice::from_ref(&frame));
        frame
    }

    /// Every image `frame` wants, landed: a pixel under each one's key.
    #[cfg(target_os = "linux")]
    fn landed(doc: &mut Doc, frame: &PaneFrame) {
        for w in &frame.wants {
            match w.job {
                Job::Page { rotation, .. } => doc.previews.insert((w.view.key, rotation), pixel()),
                Job::Tile { .. } => doc.tiles.insert((w.view.key, w.job), pixel()),
            };
        }
    }

    /// Whether `want`'s image is had already.
    #[cfg(target_os = "linux")]
    fn had(doc: &Doc, want: &Want) -> bool {
        match want.job {
            Job::Page { rotation, .. } => doc.previews.contains_key(&(want.view.key, rotation)),
            Job::Tile { .. } => doc.tiles.contains_key(&(want.view.key, want.job)),
        }
    }

    /// The preview `frame` wants of page `index`.
    #[cfg(target_os = "linux")]
    fn preview(frame: &PaneFrame, index: usize) -> Want {
        let pl = (frame.plans.iter()).find(|pl| pl.index == index);
        let id = pl.expect("planned").pref.id;
        (frame.wants.iter())
            .find(|w| w.id == id && matches!(w.job, Job::Page { .. }))
            .copied()
            .expect("a preview")
    }

    /// `want`, rendered by the pool.
    #[cfg(target_os = "linux")]
    fn rendered(doc: &Doc, want: Want) -> Rendered {
        let rx = doc.request(want).expect("the document's page");
        futures::executor::block_on(rx).unwrap().unwrap()
    }

    /// An unsaved header shows under view rotation (M10.5 §10.4): no
    /// overlay of GPUI's draws it, but the page's view is its group's
    /// overlay, which does, and the pool's render of the pane's preview,
    /// turned with the pane, has its ink at the right, where the page's top
    /// went, and is otherwise the page drawn without it, from its base.
    #[test]
    #[cfg(target_os = "linux")]
    fn an_unsaved_header_shows_under_view_rotation() {
        use sheaf_doc::tx::fixture::{header, set_header};
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(2);
        let (mut model, _) = as_the_view_opens(&path, false).unwrap();
        let page = model.page(0).unwrap();
        let header = set_header(header("DRAFT DRAFT DRAFT"), Some(&[page]));
        model.commit_ops(vec![header], "Header").unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let mut doc = Doc::new(model, cx.focus_handle(), Default::default());
                doc.panes[0].rotation = Rotation::R90;
                let drawn = frame(&mut doc);
                assert!(matches!(drawn.plans[0].view.engine, EngineKey::Group(..)));
                assert_eq!(drawn.plans[0].rotation, Rotation::R90);
                let with = rendered(&doc, preview(&drawn, 0)).raster;
                doc.inputs.generators = false;
                let bare = frame(&mut doc);
                assert!(matches!(bare.plans[0].view.engine, EngineKey::Base(_)));
                let without = rendered(&doc, preview(&bare, 0)).raster;
                assert_eq!((with.width, with.height), (without.width, without.height));
                let columns: Vec<u32> = (with.data.chunks(4).zip(without.data.chunks(4)))
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .map(|(i, _)| i as u32 % with.width)
                    .collect();
                assert!(!columns.is_empty(), "the header shows");
                assert!(columns.iter().all(|&x| x > with.width * 3 / 4), "turned");
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// A square another tool drew, by `author`, with its appearance.
    #[cfg(target_os = "linux")]
    fn their_square(pdf: &mut lopdf::Document, author: &str) -> lopdf::ObjectId {
        use lopdf::{Object, Stream, dictionary};
        let ap = pdf.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 80.into(), 80.into()],
            },
            b"0 0.6 0 rg 0 0 80 80 re f".to_vec(),
        ));
        pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Square", "T" => Object::string_literal(author),
            "NM" => Object::string_literal(format!("{author}'s")),
            "Rect" => vec![200.into(), 300.into(), 280.into(), 380.into()],
            "C" => vec![0.into(), 0.6.into(), 0.into()], "AP" => dictionary! { "N" => ap },
        })
    }

    /// Hiding a comment draws its page again, and no other (M10.5
    /// §10.2): as the Comments panel's filter hides it, its page's screen
    /// key changes and no other page's does, so their images all stay
    /// cached and the frame wants none of theirs anew; its page keeps its
    /// old image until its new one lands.
    #[test]
    #[cfg(target_os = "linux")]
    fn hiding_a_comment_rerenders_only_its_page() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let mut f = sheaf_doc::forms::fixture::Form::new(3);
        for (page, author) in ["Alice", "Bob", "Bob"].into_iter().enumerate() {
            let square = their_square(&mut f.pdf, author);
            f.set_annots(page, &[square]);
        }
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("comments.pdf");
        std::fs::write(&path, f.bytes()).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    view.comment_view(cx);
                    let doc = shown(view);
                    let first = doc.page_id(0).unwrap();
                    let before = frame(doc);
                    landed(doc, &before);
                    let before = frame(doc);
                    assert_eq!(before.keep.len(), 3);
                    doc.comment_filter.authors.insert("Alice".into());
                    view.comment_view(cx);
                    let doc = shown(view);
                    assert_eq!(doc.inputs.hidden.len(), 1);
                    let after = frame(doc);
                    for (&(page, was, _), &(_, now, _)) in before.keep.iter().zip(&after.keep) {
                        assert_eq!(was == now, page != first, "{page:?}");
                    }
                    let (theirs, ours): (Vec<&Want>, Vec<&Want>) =
                        after.wants.iter().partition(|w| w.id != first);
                    assert!(!theirs.is_empty() && theirs.iter().all(|w| had(doc, w)));
                    assert!(ours.iter().all(|w| !had(doc, w)));
                    let pl = &after.plans[0];
                    assert_eq!(pl.shows, before.plans[0].view.key);
                    assert!(doc.previews.contains_key(&(pl.shows, pl.rotation)));
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Hiding a comment leaves print preview and what OCR reads as they
    /// were (M10.5 §10.4): with View > Hide Comments, the comment's page
    /// draws anew on screen, but print preview, passing print's own inputs,
    /// finds and draws it as before, the comment with it, and OCR's input,
    /// the page's content alone, is as before, without it.
    #[test]
    #[cfg(target_os = "linux")]
    fn hiding_a_comment_leaves_print_preview_and_the_ocr_input_alone() {
        use sheaf_ocr::PageSource;
        use sheaf_ops::views::ViewPages;
        use std::sync::atomic::{AtomicBool, Ordering};
        let mut f = sheaf_doc::forms::fixture::Form::new(1);
        let square = their_square(&mut f.pdf, "Alice");
        f.set_annots(0, &[square]);
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("comment.pdf");
        std::fs::write(&path, f.bytes()).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    // The screen's key, print preview's and its picture (as
                    // its view says its engine draws it), and OCR's picture
                    // of the page.
                    let drawn = |view: &mut DocView| {
                        let prefs = crate::print_dialog::PrintPrefs::default();
                        let ctx = view.print_context(prefs).unwrap();
                        let page = ctx.from.snap.state.page_at(0).unwrap();
                        let doc = shown(view);
                        let screen = doc.views.view(doc.model.state(), page, &doc.inputs);
                        let (preview, job) = ctx.preview(page, 400, true).unwrap();
                        let rx = ctx.render_preview(page, 400, true);
                        let printed = futures::executor::block_on(rx).unwrap().unwrap();
                        let request = printed.request;
                        assert_eq!((request.page.0, request.job), (preview.index, job));
                        let pages = ViewPages::new(ctx.views.clone(), ctx.from.snap.clone());
                        let read = pages.render(&pages.page(page).unwrap(), 400).unwrap();
                        let key = preview.key;
                        (screen.unwrap().key, key, printed.raster.data, read.data)
                    };
                    let (screen, key, printed, read) = drawn(view);
                    view.toggle_comments_shown(cx);
                    assert_eq!(shown(view).inputs.hidden.len(), 1);
                    let hid = drawn(view);
                    assert_ne!(hid.0, screen, "the screen draws the page anew");
                    assert_eq!(hid.1, key);
                    assert!(hid.2 == printed && hid.3 == read);
                    let green = |px: &[u8]| px[1] > 120 && px[0] < 60 && px[2] < 60;
                    assert!(printed.chunks(4).any(green), "print shows the comment");
                    assert!(!read.chunks(4).any(green), "OCR never reads it");
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// An edit keeps its page's old image on screen until the new one
    /// lands (M10.5 §10.2), and then no image of the old key is left: kept
    /// while the page is drawn by it, let go once it isn't.
    #[test]
    #[cfg(target_os = "linux")]
    fn an_edit_keeps_the_old_image_until_the_new_one_lands() {
        use sheaf_doc::tx::fixture::{header, set_header};
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, path) = blank_file(2);
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    let doc = shown(view);
                    let page = doc.page_id(0).unwrap();
                    let first = frame(doc);
                    landed(doc, &first);
                    let old = frame(doc).plans[0].shows;
                    assert_eq!(old, first.plans[0].view.key);
                    let header = set_header(header("Draft"), Some(&[page]));
                    let tx = view.lower(vec![header], "Header", cx).unwrap();
                    view.commit(tx, CommitOpts::labelled("Header"), cx).unwrap();
                    let doc = shown(view);
                    let edited = frame(doc);
                    let pl = &edited.plans[0];
                    assert_ne!(pl.view.key, old);
                    assert_eq!(pl.shows, old, "the old image, meanwhile");
                    assert!(doc.previews.contains_key(&(old, pl.rotation)));
                    let want = preview(&edited, 0);
                    let new = rendered(doc, want);
                    view.on_rendered((want.view.key, want.job), Ok(Ok(new)), cx);
                    let doc = shown(view);
                    let after = frame(doc);
                    assert_eq!(after.plans[0].shows, want.view.key);
                    assert!(!doc.previews.keys().any(|(k, _)| *k == old));
                    assert!(!doc.tiles.keys().any(|(k, _)| *k == old));
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// A Letter page drawing a blue box in layer "Notes", which its file
    /// shows.
    #[cfg(target_os = "linux")]
    fn layered() -> Vec<u8> {
        use lopdf::{Document as Pdf, Object, Stream, dictionary};
        let mut pdf = Pdf::with_version("1.7");
        let ocg = pdf.add_object(dictionary! {
            "Type" => "OCG", "Name" => Object::string_literal("Notes"),
        });
        let drawn = b"/OC /L0 BDC 0 0 1 rg 100 300 400 300 re f EMC".to_vec();
        let content = pdf.add_object(Stream::new(dictionary! {}, drawn));
        let root = pdf.new_object_id();
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => root, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Properties" => dictionary! { "L0" => ocg } },
        });
        let kids = dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 };
        pdf.objects.insert(root, Object::Dictionary(kids));
        let props = dictionary! {
            "OCGs" => vec![ocg.into()], "D" => dictionary! { "Order" => vec![ocg.into()] },
        };
        let catalog = pdf.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => root, "OCProperties" => props,
        });
        pdf.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();
        bytes
    }

    /// Set in the process a test runs alone in, to the file it opens.
    #[cfg(target_os = "linux")]
    const ALONE: &str = "SHEAF_APP_ALONE";

    /// Runs test `name` again in a process of its own, [`ALONE`] set to
    /// `path` and each of `env` set too, which must pass.
    #[cfg(target_os = "linux")]
    fn alone(name: &str, path: &std::path::Path, env: &[(&str, &std::path::Path)]) {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([name, "--exact", "--nocapture", "--test-threads=1"])
            .env(ALONE, path)
            .envs(env.iter().copied())
            .output()
            .unwrap();
        let said = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        let (stdout, stderr) = (said(&out.stdout), said(&out.stderr));
        assert!(out.status.success(), "{}\n{stdout}{stderr}", out.status);
        assert!(stdout.contains("1 passed"), "{stdout}");
    }

    /// Layers write no file (M10.5 §10.6): in a process of its own, its
    /// temporary directory empty, the Layers panel turns a layer off and
    /// on again, and the page renders without it, from a patch of its
    /// source's catalog, and with it, and the directory stays empty.
    #[test]
    #[cfg(target_os = "linux")]
    fn using_layers_writes_no_temp_file() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let Some(path) = std::env::var_os(ALONE).map(PathBuf::from) else {
            let name = "docview::tests::using_layers_writes_no_temp_file";
            let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
            let (path, tmp) = (dir.path().join("layers.pdf"), dir.path().join("tmp"));
            std::fs::write(&path, layered()).unwrap();
            std::fs::create_dir(&tmp).unwrap();
            alone(name, &path, &[("TMPDIR", &tmp)]);
            let left: Vec<_> = std::fs::read_dir(&tmp).unwrap().collect();
            assert!(left.is_empty(), "{left:?}");
            return;
        };
        let (model, facts) = as_the_view_opens(&path, false).unwrap();
        let layer = facts.layers.iter().find_map(|l| l.id).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    shown(view).set_layers(facts.layers);
                    let drawn = |view: &mut DocView| {
                        let doc = shown(view);
                        let want = preview(&frame(doc), 0);
                        (want.view.engine, rendered(doc, want).raster.data)
                    };
                    let (engine, all) = drawn(view);
                    assert!(matches!(engine, EngineKey::Base(_)));
                    view.toggle_layer(layer, cx);
                    let (engine, off) = drawn(view);
                    assert!(matches!(engine, EngineKey::SourcePatch(..)));
                    assert!(off != all, "the layer is off");
                    view.toggle_layer(layer, cx);
                    let (_, on) = drawn(view);
                    assert!(on == all, "and on again");
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Printing, print preview and image export write no file (M10.5
    /// §10.6). The test runs in a process of its own whose temporary
    /// directory it can't write to, so a file made there, kept or not,
    /// fails it. There a page with a mark is drawn as print preview draws
    /// it (under print's inputs), printed with its markup ([`send_print`])
    /// through a fake `lp`, which keeps what it reads on its stdin beside
    /// the file, and exported to PNG and to TIFF beside the file. The
    /// directory stays empty, and `lp` got the print file, both its pages.
    #[test]
    #[cfg(target_os = "linux")]
    fn printing_previewing_and_exporting_write_no_temp_file() {
        use sheaf_ops::export::{ExportJob, Format, TiffMode};
        use sheaf_render::Renderer;
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::{AtomicBool, Ordering};
        let Some(path) = std::env::var_os(ALONE).map(PathBuf::from) else {
            let name = "docview::tests::printing_previewing_and_exporting_write_no_temp_file";
            let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
            let at = |name| dir.path().join(name);
            let (path, tmp, bin, job) = (at("print.pdf"), at("tmp"), at("bin"), at("job.pdf"));
            std::fs::write(&path, said(2)).unwrap();
            std::fs::create_dir(&tmp).unwrap();
            std::fs::create_dir(&bin).unwrap();
            let lp = bin.join("lp");
            let reply = "echo 'request id is fake-1 (1 file(s))'";
            let script = format!("#!/bin/sh\ncat > '{}'\n{reply}\n", job.display());
            std::fs::write(&lp, script).unwrap();
            let mode = std::fs::Permissions::from_mode;
            std::fs::set_permissions(&lp, mode(0o755)).unwrap();
            std::fs::set_permissions(&tmp, mode(0o555)).unwrap();
            alone(name, &path, &[("TMPDIR", &tmp), ("SHEAF_CUPS_BIN", &bin)]);
            let left: Vec<_> = std::fs::read_dir(&tmp).unwrap().collect();
            assert!(left.is_empty(), "{left:?}");
            let printed = std::fs::read(&job).unwrap();
            assert!(printed.starts_with(b"%PDF"), "lp got the print file");
            let engine = sheaf_render::pdfium::PdfiumEngine::open_bytes(printed.into(), None);
            assert_eq!(engine.unwrap().page_count(), 2);
            return;
        };
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let out = path.with_file_name("pages");
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    let page = shown(view).page_id(0).unwrap();
                    let rect = Box2 {
                        x: 300.0,
                        y: 600.0,
                        w: 80.0,
                        h: 50.0,
                    };
                    let square = view.new_annot(AnnotKind::Square { rect }, [200, 0, 0]);
                    assert!(view.add_annot(page, square, "Square", cx).is_some());
                    let prefs = crate::print_dialog::PrintPrefs::default();
                    let ctx = view.print_context(prefs.clone()).unwrap();
                    let rx = ctx.render_preview(page, 400, true);
                    let preview = futures::executor::block_on(rx).unwrap().unwrap();
                    assert_eq!(preview.raster.width, 400);
                    let pages: Vec<PageId> = ctx.from.snap.state.order().iter().copied().collect();
                    let job = sheaf_print::JobSettings {
                        printer: "Office".into(),
                        title: ctx.title.clone(),
                        copies: 1,
                        collate: true,
                        paper: sheaf_print::Paper::letter(),
                        sizing: sheaf_print::Sizing::ShrinkOversized,
                        orientation: sheaf_print::Orientation::Auto,
                        duplex: sheaf_print::Duplex::Off,
                    };
                    let req = crate::print_dialog::PrintRequest {
                        job,
                        pages: pages.clone(),
                        markup: true,
                        save_pdf: false,
                        prefs,
                    };
                    let State::Ready(doc) = &view.state else {
                        panic!("the view is ready");
                    };
                    let from = doc.model.export_from(Actor::User);
                    assert_eq!(send_print(from.clone(), &req).unwrap(), "request fake-1");
                    for format in [Format::Png, Format::Tiff(TiffMode::Color)] {
                        let job = ExportJob {
                            pages: pages.clone(),
                            markup: true,
                            dpi: 36,
                            format,
                            dir: out.clone(),
                            stem: "print".into(),
                        };
                        let progress = &sheaf_ops::NoProgress;
                        let files = sheaf_ops::export::run(&from, &job, progress).unwrap();
                        assert_eq!(files.len(), if format.single_file() { 1 } else { 2 });
                        assert!(files.iter().all(|f| f.starts_with(&out) && f.is_file()));
                    }
                });
                ran.store(true, Ordering::Relaxed);
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// The exports are jobs, one at a time (M10.5 §12, §14.1): an Extract
    /// is the running job, an export asked for meanwhile is refused and the
    /// status line says why, and once the Extract is written the job is
    /// done and the notice says so. The document's own file is refused as
    /// its target (§12.1) and nothing is written.
    #[test]
    #[cfg(target_os = "linux")]
    fn exports_run_as_jobs_one_at_a_time() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("a.pdf");
        std::fs::write(&path, said(2)).unwrap();
        let before = std::fs::read(&path).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let (out, flat) = (dir.path().join("out.pdf"), dir.path().join("flat.pdf"));
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let (ran, path) = (ran.clone(), path.clone());
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |v, cx| {
                    let ids = vec![shown(v).page_id(0).unwrap()];
                    v.extract_to(ids, out.clone(), cx);
                    assert_eq!(v.job.as_ref().map(|j| j.name.as_str()), Some("Extract"));
                    v.flattened_copy_to(flat.clone(), cx);
                    let said = "Extract is still running — Esc stops it";
                    assert_eq!(shown(v).notice.as_deref(), Some(said));
                });
                cx.spawn(async move |cx| {
                    let ended = async |cx: &mut gpui::AsyncApp| {
                        for _ in 0..1000 {
                            if cx.update(|cx| view.read(cx).job.is_none()) {
                                return;
                            }
                            let tick = std::time::Duration::from_millis(10);
                            cx.background_executor().timer(tick).await;
                        }
                        panic!("the job ran on");
                    };
                    ended(cx).await;
                    cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            let said = format!("Extracted 1 page to {}", out.display());
                            assert_eq!(shown(v).notice.as_deref(), Some(&*said));
                            assert!(out.is_file() && !flat.exists());
                            v.export_clean_to(path.clone(), cx);
                        })
                    });
                    ended(cx).await;
                    cx.update(|cx| {
                        let notice = view.update(cx, |v, _| shown(v).notice.clone());
                        let said = "Export failed: an export can't replace the open file; \
                                    choose another name";
                        assert_eq!(notice.as_deref(), Some(said));
                        ran.store(true, Ordering::Relaxed);
                        cx.quit();
                    });
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    /// A page whose annotations can't be drawn into it (its content can't
    /// be read) has no print preview with markup, and it's tried once: the
    /// preview keeps that, so a later frame doesn't lower it again.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_print_preview_that_cant_be_made_is_tried_once() {
        use lopdf::{Document as Pdf, Object, Stream, dictionary};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("unread.pdf");
        let mut pdf = Pdf::load_mem(&said(1)).unwrap();
        let page = pdf.get_pages()[&1];
        let bad = pdf.add_object(Stream::new(dictionary! {}, b"0 0 9 9 re f EMC".to_vec()));
        let dict = pdf.get_dictionary_mut(page).unwrap();
        dict.set("Contents", Object::Reference(bad));
        pdf.save(&path).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        gpui_platform::headless().run({
            let seen = seen.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    let page = shown(view).page_id(0).unwrap();
                    let rect = Box2 {
                        x: 300.0,
                        y: 600.0,
                        w: 80.0,
                        h: 50.0,
                    };
                    let square = view.new_annot(AnnotKind::Square { rect }, [200, 0, 0]);
                    let added = view.add_annot(page, square, "Square", cx).is_some();
                    let prefs = crate::print_dialog::PrintPrefs::default();
                    let ctx = view.print_context(prefs).unwrap();
                    let mut seen = seen.lock().unwrap();
                    seen.push(added);
                    for _ in 0..2 {
                        seen.push(ctx.preview(page, 400, true).is_none());
                        seen.push(ctx.printed.borrow().get(&page) == Some(&None));
                    }
                    seen.push(ctx.preview(page, 400, false).is_some());
                });
                cx.spawn(async |cx| cx.update(|cx| cx.quit())).detach();
            }
        });
        assert_eq!(*seen.lock().unwrap(), [true; 6]);
    }

    /// The threads of this process.
    #[cfg(target_os = "linux")]
    fn tasks() -> usize {
        std::fs::read_dir("/proc/self/task").unwrap().count()
    }

    /// Waits, a tick at a time, until `view` runs no planner.
    #[cfg(target_os = "linux")]
    async fn planned(view: &Entity<DocView>, cx: &mut gpui::AsyncApp) {
        for _ in 0..1000 {
            if cx.update(|cx| view.read(cx).planners.is_empty()) {
                return;
            }
            let tick = std::time::Duration::from_millis(10);
            cx.background_executor().timer(tick).await;
        }
        panic!("the planners ran on");
    }

    /// The process keeps as many threads as it had once its first pages
    /// rendered, across 20 inserts and 20 crops of each kind (M10.5
    /// §10.3): in a process of its own, each file inserted (another each
    /// time) lands through its planner, each crop of the first page is one
    /// op committed on the UI thread, each Trim White of the last page
    /// measures it through the pool on its planner, its pages render from
    /// engines the one render worker opens, and no thread starts. A crop
    /// makes no copy, so none is read by lopdf, whose first read started
    /// rayon's pool until slice 15.
    #[test]
    #[cfg(target_os = "linux")]
    fn the_thread_count_is_constant_across_20_inserts_and_crops() {
        use sheaf_edit::Margins;
        use std::sync::atomic::{AtomicBool, Ordering};
        let Some(path) = std::env::var_os(ALONE).map(PathBuf::from) else {
            let name = "docview::tests::the_thread_count_is_constant_across_20_inserts_and_crops";
            let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
            let path = dir.path().join("said.pdf");
            std::fs::write(&path, said(2)).unwrap();
            alone(name, &path, &[]);
            return;
        };
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let files: Vec<PathBuf> = (0..=20)
            .map(|k| {
                let file = dir.path().join(format!("{k}.pdf"));
                let bytes = sheaf_doc::memory::blank_pages(600.0 + k as f32, 792.0, 1);
                std::fs::write(&file, bytes.unwrap()).unwrap();
                file
            })
            .collect();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                cx.spawn(async move |cx| {
                    let drawn = |cx: &mut gpui::AsyncApp| {
                        cx.update(|cx| {
                            view.update(cx, |v, _| {
                                let doc = shown(v);
                                for want in frame(doc).wants {
                                    rendered(doc, want);
                                }
                            })
                        })
                    };
                    drawn(cx);
                    let mut threads = vec![tasks()];
                    for (k, file) in files.into_iter().enumerate() {
                        cx.update(|cx| {
                            view.update(cx, |v, cx| {
                                let first = shown(v).page_id(0);
                                v.insert_file(file, first, format!("{k}.pdf"), false, cx);
                            })
                        });
                        planned(&view, cx).await;
                        drawn(cx);
                        cx.update(|cx| {
                            view.update(cx, |v, cx| {
                                let opened = v.model_snapshot().unwrap();
                                let first = opened.state.page_at(0).unwrap();
                                let margin = 4.0 + k as f32;
                                let margins = Margins {
                                    top: margin,
                                    bottom: margin,
                                    left: margin,
                                    right: margin,
                                };
                                v.crop_pages(opened, vec![first], margins, cx);
                                let opened = v.model_snapshot().unwrap();
                                let last = opened.state.page_at(opened.state.page_count() - 1);
                                v.trim_white(opened, vec![last.unwrap()], cx);
                            })
                        });
                        planned(&view, cx).await;
                        drawn(cx);
                        threads.push(tasks());
                    }
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            let doc = shown(v);
                            assert_eq!(doc.model.page_count(), 23);
                            assert_eq!(doc.model.undo_label(), Some("Crop 1 page"));
                            let last = doc.model.geom(doc.model.page(22).unwrap());
                            assert_ne!(last.crop, last.media, "Trim White cropped the last");
                        })
                    });
                    assert!(threads.iter().all(|&n| n == threads[0]), "{threads:?}");
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Two Trim Whites on overlapping pages, started on one version, both
    /// land (M10.5 §8.5): whichever lands second conflicts on a page the
    /// first cropped, is measured again on the version the first made, and
    /// lands, cropping only the page the first didn't. Every page then
    /// holds the crop Trim White finds for it on the version both started
    /// on, the document has the two versions alone, and the notice is the
    /// second's, no conflict left in it.
    #[test]
    #[cfg(target_os = "linux")]
    fn two_overlapping_trims_both_land() {
        use sheaf_doc::geom::Rect;
        use sheaf_ops::crop;
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("said.pdf");
        std::fs::write(&path, said(4)).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ids: Vec<PageId> = (0..4).map(|i| model.page(i).unwrap()).collect();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                let (opened, views, lower) = view.update(cx, |v, cx| {
                    let opened = v.model_snapshot().unwrap();
                    v.trim_white(opened.clone(), ids[..3].to_vec(), cx);
                    v.trim_white(opened.clone(), ids[1..].to_vec(), cx);
                    let doc = shown(v);
                    let lower = doc.model.lower_cx(Actor::Job(sheaf_doc::JobKind::Trim));
                    (opened, Views::new(doc.views.pool().clone()), lower)
                });
                cx.spawn(async move |cx| {
                    planned(&view, cx).await;
                    let want = cx.background_executor().spawn({
                        let ids = ids.clone();
                        async move {
                            let (pad, urgency) = (crop::PAD, crop::Urgency::Background);
                            let p = &sheaf_ops::NoProgress;
                            crop::trim_white(&views, &opened, lower, &ids, pad, urgency, p)
                        }
                    });
                    let want = want.await.unwrap();
                    assert_eq!(want.crops.len(), 4, "{want:?}");
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            let doc = shown(v);
                            assert_eq!(doc.notice.as_deref(), Some("Trimmed 1 page"));
                            for &(id, to) in &want.crops {
                                let g = doc.model.geom(id);
                                let crop = Rect::new(to.x, to.y, to.right(), to.bottom());
                                assert_eq!(g.crop, crop.intersect(g.media), "{id:?}");
                            }
                            let model = &mut doc.model;
                            for label in ["Crop 1 page", "Crop 3 pages"] {
                                assert_eq!(model.undo_label(), Some(label));
                                model.undo().unwrap();
                            }
                            assert!(!model.can_undo());
                        })
                    });
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// The form bar learns the file's form from its base engine, which
    /// the render worker opens when the view asks it, beside the first
    /// page: an AcroForm, and none until it's said.
    #[test]
    #[cfg(target_os = "linux")]
    fn the_form_type_comes_from_the_primary_s_engine() {
        use sheaf_render::FormType;
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("every-kind.pdf");
        std::fs::write(&path, sheaf_doc::forms::fixture::every_kind().bytes()).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |v, cx| {
                    assert_eq!(shown(v).forms.form_type, FormType::None);
                    v.ask_form_type(cx);
                });
                cx.spawn(async move |cx| {
                    let said = |cx: &mut gpui::AsyncApp| {
                        cx.update(|cx| view.update(cx, |v, _| shown(v).forms.form_type))
                    };
                    for _ in 0..1000 {
                        if said(cx) != FormType::None {
                            break;
                        }
                        let tick = std::time::Duration::from_millis(10);
                        cx.background_executor().timer(tick).await;
                    }
                    assert_eq!(said(cx), FormType::AcroForm);
                    cx.update(|cx| view.update(cx, |v, _| assert!(shown(v).notice.is_none())));
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// A file PDFium refuses but sheaf opens (as the corpus's 4422502,
    /// M10.5 slice 13) draws its pages (Greg, 2026-10-20): the first frame
    /// views them on the base engine, as nothing has tried it yet; the
    /// render asked of it finds it refused, tells the source, and draws
    /// the page from its group instead; the next frame views the page on
    /// its group under the same key, so the image that landed is the one
    /// shown and nothing is asked again. The form type's ask then finds the
    /// engine refused too, and the notice says the pages are sheaf's copy;
    /// and Find finds a page's text, and the Select tool reads it, from its
    /// group (slice 14).
    #[test]
    #[cfg(target_os = "linux")]
    fn a_file_pdfium_refuses_draws_its_pages() {
        use sheaf_doc::serialize::fixture::revision_cut_out;
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("refused.pdf");
        std::fs::write(&path, revision_cut_out(said(2))).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let first = model.page(0).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                view.update(cx, |view, cx| {
                    let doc = shown(view);
                    let want = preview(&frame(doc), 0);
                    assert!(matches!(want.view.engine, EngineKey::Base(_)));
                    let image = rendered(doc, want);
                    assert!(doc.model.primary().base_refused());
                    let inked = image.raster.data.chunks(4).any(|px| px[..3] != [255; 3]);
                    assert!(inked, "the page is drawn");
                    view.on_rendered((want.view.key, want.job), Ok(Ok(image)), cx);
                    let doc = shown(view);
                    let next = frame(doc);
                    let pl = &next.plans[0];
                    assert!(matches!(pl.view.engine, EngineKey::Group(..)));
                    assert_eq!((pl.view.key, pl.shows), (want.view.key, want.view.key));
                    assert!(had(doc, &preview(&next, 0)));
                    view.ask_form_type(cx);
                });
                cx.spawn(async move |cx| {
                    let notice = |cx: &mut gpui::AsyncApp| {
                        cx.update(|cx| view.update(cx, |v, _| shown(v).notice.clone()))
                    };
                    for _ in 0..1000 {
                        if notice(cx).is_some() {
                            break;
                        }
                        let tick = std::time::Duration::from_millis(10);
                        cx.background_executor().timer(tick).await;
                    }
                    let notice = notice(cx).expect("a notice");
                    assert!(notice.contains("drawn from sheaf's copy"), "{notice}");
                    assert!(!notice.contains("don't work"), "{notice}");
                    let hits = found(&view, "page 1", cx).await;
                    let read =
                        cx.update(|cx| view.update(cx, |v, cx| select_text(v, &[first], cx)));
                    let text = selected(&read, 1, cx).await.remove(&first).flatten();
                    assert_eq!(text.as_deref(), Some("page 0"));
                    cx.update(|cx| {
                        let State::Ready(doc) = &view.read(cx).state else {
                            panic!("ready");
                        };
                        assert!(hit_in_order(doc, &hits, &order(doc)[1..]), "{hits:?}");
                    });
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// A file of `n` Letter pages, each saying "page i".
    #[cfg(target_os = "linux")]
    fn said(n: usize) -> Vec<u8> {
        use lopdf::{Document as Pdf, Object, Stream, dictionary};
        let mut pdf = Pdf::with_version("1.7");
        let root = pdf.new_object_id();
        let font = pdf.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let kids: Vec<Object> = (0..n)
            .map(|i| {
                let content = format!("BT /F1 28 Tf 72 400 Td (page {i}) Tj ET").into_bytes();
                let content = pdf.add_object(Stream::new(dictionary! {}, content));
                let page =
                    dictionary! { "Type" => "Page", "Parent" => root, "Contents" => content };
                pdf.add_object(page).into()
            })
            .collect();
        let pages = dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => n as i64,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        };
        pdf.objects.insert(root, Object::Dictionary(pages));
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        pdf.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();
        bytes
    }

    /// Find for `query` in `view`, the bar opened if it isn't, once it's
    /// done: its hits.
    #[cfg(target_os = "linux")]
    async fn found(view: &Entity<DocView>, query: &str, cx: &mut gpui::AsyncApp) -> Vec<Hit> {
        cx.update(|cx| {
            view.update(cx, |v, cx| {
                if v.search.is_none() {
                    v.new_search(cx);
                }
                v.start_search(query.into(), cx)
            })
        });
        for _ in 0..1000 {
            if cx.update(|cx| (view.read(cx).search.as_ref()).is_some_and(|s| s.done)) {
                break;
            }
            let tick = std::time::Duration::from_millis(10);
            cx.background_executor().timer(tick).await;
        }
        cx.update(|cx| {
            let search = view.read(cx).search.as_ref().unwrap();
            assert!(search.done, "{query:?} is searched");
            search.hits.clone()
        })
    }

    /// What the Select tool reads of each of `pages` in `v`, as each is
    /// read: its text, or None when it can't be read.
    #[cfg(target_os = "linux")]
    fn select_text(v: &mut DocView, pages: &[PageId], cx: &mut Context<DocView>) -> Selected {
        let read = Selected::default();
        for &page in pages {
            let read = read.clone();
            v.with_chars(page, cx, move |_, chars, _| {
                let text = chars.map(|c| c.text(0, c.len()));
                read.borrow_mut().insert(page, text);
            });
        }
        read
    }

    /// Pages' text as the Select tool reads it, by page.
    #[cfg(target_os = "linux")]
    type Selected = std::rc::Rc<std::cell::RefCell<HashMap<PageId, Option<String>>>>;

    /// What [`select_text`] read, once `n` pages are.
    #[cfg(target_os = "linux")]
    async fn selected(
        read: &Selected,
        n: usize,
        cx: &mut gpui::AsyncApp,
    ) -> HashMap<PageId, Option<String>> {
        for _ in 0..1000 {
            if read.borrow().len() == n {
                break;
            }
            let tick = std::time::Duration::from_millis(10);
            cx.background_executor().timer(tick).await;
        }
        let read = read.take();
        assert_eq!(read.len(), n, "the pages are read: {read:?}");
        read
    }

    /// Whether `hits` are on `pages`, in turn, each at its page's position
    /// in `doc` and on its visible box, in PageSpace.
    #[cfg(target_os = "linux")]
    fn hit_in_order(doc: &Doc, hits: &[Hit], pages: &[PageId]) -> bool {
        let st = doc.model.state();
        let on = |h: &Hit| {
            let v = st.geom(h.page).unwrap().visible();
            (h.rects.iter())
                .all(|r| r.x >= v.x0 && r.y >= v.y0 && r.right() <= v.x1 && r.bottom() <= v.y1)
        };
        let found: Vec<PageId> = hits.iter().map(|h| h.page).collect();
        found == pages && (hits.iter()).all(|h| st.page_at(h.order) == Some(h.page) && on(h))
    }

    /// Find and Patterns search every page as it draws, through the render
    /// worker's walk over the document's order (M10.5 §10.5): a page of an
    /// inserted file, a duplicate, a crop and a page with recognized text
    /// and a header not yet saved are found as the file's own pages are,
    /// each hit on its page, by id, at its position, on its visible box in
    /// PageSpace. Find "page 3" finds the page alone, and "PAGE" every page;
    /// a pattern finds every page, and the recognized words; the header's
    /// text finds the pages it's on; and a new search replaces the one
    /// before it. The Select tool reads each page as it shows too: the
    /// inserted page, the duplicate and the crop, and the header and the
    /// recognized words not yet saved.
    #[test]
    #[cfg(target_os = "linux")]
    fn find_and_patterns_search_through_the_pool() {
        use sheaf_doc::tx::fixture as fx;
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("said.pdf");
        std::fs::write(&path, said(6)).unwrap();
        let (mut model, _) = as_the_view_opens(&path, false).unwrap();
        let p: Vec<PageId> = (0..6).map(|i| model.page(i).unwrap()).collect();
        // Page 7 of another file before the third page, the fifth page
        // duplicated, the second cropped, and the last recognized and
        // headed, as is the first.
        let kind = sheaf_doc::store::BlobKind::file(std::path::Path::new("eight.pdf"), None);
        let eight = model.store().open(said(8).into(), kind).unwrap();
        let ops = vec![
            fx::insert(&eight, 7..8, "eight.pdf", Some(p[2])),
            fx::duplicate(&[p[4]]),
        ];
        model.commit_ops(ops, "Insert").unwrap();
        let word = |text: &str, x| sheaf_doc::OcrWord {
            text: text.into(),
            rect: Box2 {
                x,
                y: 600.0,
                w: 50.0,
                h: 14.0,
            },
            confidence: 0.9,
        };
        let seen = vec![word("Total", 300.0), word("42", 360.0)];
        let layer = sheaf_doc::text_layer::from_engine(seen, &model.geom(p[5]), "test");
        let mut ops = fx::ocr([(p[5], Arc::new(layer))]);
        ops.push(fx::set_header(fx::header("Draft"), Some(&[p[0], p[5]])));
        model.commit_ops(ops, "OCR").unwrap();
        let margins = sheaf_edit::Margins {
            top: 40.0,
            bottom: 50.0,
            left: 20.0,
            right: 30.0,
        };
        let crop = fx::crop(&[p[1]], margins);
        model.commit_ops(vec![crop], "Crop").unwrap();
        let st = model.state();
        let [inserted, copy] = [2, 6].map(|i| st.page_at(i).unwrap());
        assert_eq!(st.def(inserted).unwrap().base.src.id(), eight.id());
        assert!(st.def(copy).unwrap().copy > 0);
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                let selectable = [p[3], inserted, copy, p[1], p[5]];
                let read = view.update(cx, |v, cx| {
                    v.new_search(cx);
                    v.start_search("page 2".into(), cx);
                    v.start_pattern_search(r"page \d|Total \d+".into(), cx);
                    select_text(v, &selectable, cx)
                });
                cx.spawn(async move |cx| {
                    let hits = found(&view, "page 3", cx).await;
                    let patterned = |cx: &mut gpui::AsyncApp| {
                        cx.update(|cx| view.read(cx).pattern.as_ref().is_some_and(|p| p.done))
                    };
                    for _ in 0..1000 {
                        if patterned(cx) {
                            break;
                        }
                        let tick = std::time::Duration::from_millis(10);
                        cx.background_executor().timer(tick).await;
                    }
                    let read = selected(&read, selectable.len(), cx).await;
                    let paged = found(&view, "PAGE", cx).await;
                    let headed = found(&view, "draft", cx).await;
                    let recognized = found(&view, "total 42", cx).await;
                    cx.update(|cx| {
                        let v = view.read(cx);
                        let State::Ready(doc) = &v.state else {
                            panic!("ready");
                        };
                        let order = order(doc);
                        assert!(hit_in_order(doc, &hits, &[p[3]]), "{hits:?}");
                        assert_eq!(hits[0].text, "page 3");
                        assert!(hit_in_order(doc, &paged, &order), "{paged:?}");
                        let pattern = v.pattern.as_ref().unwrap();
                        assert!(pattern.done && pattern.scanned == order.len());
                        let every: Vec<PageId> = order.iter().copied().chain([p[5]]).collect();
                        assert!(
                            hit_in_order(doc, &pattern.hits, &every),
                            "{:?}",
                            pattern.hits
                        );
                        let texts: Vec<&str> = pattern.hits.iter().map(|h| &h.text[..]).collect();
                        let want = [0, 1, 7, 2, 3, 4, 4, 5].map(|i| format!("page {i}"));
                        assert_eq!(texts[..8], want);
                        assert_eq!(texts[8], "Total 42");
                        // Each page's line is at 72, 400 in PageSpace, y up,
                        // whatever its crop.
                        for h in &pattern.hits[..8] {
                            let r = h.rects[0];
                            let at = (r.x - 72.0).abs() < 1.0 && r.y < 400.0 && r.bottom() > 400.0;
                            assert!(at, "{h:?}");
                        }
                        assert!(hit_in_order(doc, &headed, &[p[0], p[5]]), "{headed:?}");
                        assert!(hit_in_order(doc, &recognized, &[p[5]]), "{recognized:?}");
                        let find = v.search.as_ref().unwrap();
                        assert_eq!(find.scanned, order.len());
                    });
                    let text = |page| read[&page].as_deref().expect("the page's characters");
                    assert_eq!(text(p[3]), "page 3");
                    assert_eq!(text(inserted), "page 7");
                    assert_eq!(text(copy), "page 4");
                    assert_eq!(text(p[1]), "page 1");
                    // In the order the page draws them: its own text, the
                    // recognized words over it, then the header.
                    assert_eq!(text(p[5]), "page 5\nTotal 42\nDraft");
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// The calculator's Pick reads the page's text as the screen shows it
    /// (M10.5 §10.5): where a click finds no number, words recognized there
    /// and not yet saved, which the page's realization draws, give one,
    /// read again as the page changed, and it goes onto the tape.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_pick_reads_a_number_recognized_but_not_saved() {
        use sheaf_edit::tape::Row;
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("said.pdf");
        std::fs::write(&path, said(1)).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let page = model.page(0).unwrap();
        let word = |text: &str, x| sheaf_doc::OcrWord {
            text: text.into(),
            rect: Box2 {
                x,
                y: 600.0,
                w: 50.0,
                h: 14.0,
            },
            confidence: 0.9,
        };
        let seen = vec![word("Total", 300.0), word("42", 360.0)];
        let geom = model.geom(page);
        let layer = sheaf_doc::text_layer::from_engine(seen, &geom, "test");
        let ops = sheaf_doc::tx::fixture::ocr([(page, Arc::new(layer))]);
        // On "42", as the page shows.
        let on = geom.from_view().apply(Pt { x: 380.0, y: 607.0 });
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                let pick = move |v: &mut DocView, cx: &mut Context<DocView>| {
                    v.ensure_calc(cx);
                    v.calc.as_mut().unwrap().picking = true;
                    assert!(v.pick_number_at(page, on, 1.0, cx));
                };
                view.update(cx, pick);
                cx.spawn(async move |cx| {
                    let told = |cx: &mut gpui::AsyncApp| {
                        cx.update(|cx| view.update(cx, |v, _| shown(v).notice.take()))
                    };
                    let entries = |cx: &mut gpui::AsyncApp| {
                        cx.update(|cx| {
                            let calc = view.read(cx).calc.as_ref().unwrap();
                            (calc.tape.lines.iter())
                                .filter_map(|l| match l.row {
                                    Row::Entry { value, .. } => Some(value),
                                    _ => None,
                                })
                                .collect::<Vec<f64>>()
                        })
                    };
                    let tick = std::time::Duration::from_millis(10);
                    let mut notice = None;
                    for _ in 0..1000 {
                        notice = told(cx);
                        if notice.is_some() {
                            break;
                        }
                        cx.background_executor().timer(tick).await;
                    }
                    assert_eq!(notice.as_deref(), Some("No number there"));
                    cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            let tx = v.lower(ops, "OCR", cx).unwrap();
                            v.commit(tx, CommitOpts::labelled("OCR"), cx).unwrap();
                            // The commit's own notice.
                            shown(v).notice = None;
                            pick(v, cx);
                        })
                    });
                    for _ in 0..1000 {
                        notice = told(cx);
                        if !entries(cx).is_empty() || notice.is_some() {
                            break;
                        }
                        cx.background_executor().timer(tick).await;
                    }
                    assert_eq!(entries(cx), [42.0], "{notice:?}");
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// A page that changes while its characters are read is read again,
    /// as it is now: what the Select tool gets is never what the page
    /// showed before (a header added meanwhile is in it).
    #[test]
    #[cfg(target_os = "linux")]
    fn a_page_changed_while_read_is_read_again() {
        use sheaf_doc::tx::fixture as fx;
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("said.pdf");
        std::fs::write(&path, said(1)).unwrap();
        let (model, _) = as_the_view_opens(&path, false).unwrap();
        let page = model.page(0).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                let read = view.update(cx, |v, cx| {
                    let read = select_text(v, &[page], cx);
                    let ops = vec![fx::set_header(fx::header("Draft"), None)];
                    let tx = v.lower(ops, "Header", cx).unwrap();
                    v.commit(tx, CommitOpts::labelled("Header"), cx).unwrap();
                    read
                });
                cx.spawn(async move |cx| {
                    let read = selected(&read, 1, cx).await;
                    assert_eq!(read[&page].as_deref(), Some("page 0\nDraft"));
                    // And they're kept for the page as it is.
                    let kept = cx.update(|cx| view.read(cx).cached_chars(page));
                    assert_eq!(kept.map(|c| c.text(0, c.len())), read[&page]);
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// A selection keeps the characters it was made in: with "Page <<1>>
    /// of <<N>>" on every page, a page deleted after the selected one
    /// changes how that page draws, and so its render key, and the
    /// selection still copies, with no read of the page.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_selection_outlives_a_change_to_how_its_page_draws() {
        use crate::text_select::TextSel;
        use sheaf_doc::tx::fixture as fx;
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("said.pdf");
        std::fs::write(&path, said(3)).unwrap();
        let (mut model, _) = as_the_view_opens(&path, false).unwrap();
        let p: Vec<PageId> = (0..3).map(|i| model.page(i).unwrap()).collect();
        let numbered = fx::set_header(fx::header("Page <<1>> of <<N>>"), None);
        model.commit_ops(vec![numbered], "Header").unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = view_of(model, &path, cx);
                let read = view.update(cx, |v, cx| select_text(v, &[p[1]], cx));
                cx.spawn(async move |cx| {
                    let read = selected(&read, 1, cx).await;
                    assert!(read[&p[1]].as_deref().unwrap().starts_with("page 1\n"));
                    cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            let chars = v.cached_chars(p[1]).unwrap();
                            shown(v).text_sel = Some(TextSel {
                                page: p[1],
                                chars,
                                anchor: 0,
                                head: 6,
                            });
                            let tx = v.lower(vec![fx::delete(&[p[2]])], "Delete", cx);
                            v.commit(tx.unwrap(), CommitOpts::labelled("Delete"), cx)
                                .unwrap();
                            assert!(v.cached_chars(p[1]).is_none(), "its key changed");
                            let text = v.selected_text().map(|(_, text, _)| text);
                            assert_eq!(text.as_deref(), Some("page 1"));
                            assert!(v.copy_text(cx));
                            let told = shown(v).notice.as_deref();
                            assert_eq!(told, Some("Copied 5 characters"));
                        })
                    });
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    #[test]
    fn the_status_bar_counts_the_pages_read() {
        use sheaf_ops::Progress;
        let reading = sheaf_ops::Tracker::new();
        assert_eq!(opening_line(None), "Opening…");
        assert_eq!(opening_line(Some(&reading)), "Opening…");
        reading.set_total(Some(40_000), Default::default());
        reading.set(12_000);
        assert_eq!(
            opening_line(Some(&reading)),
            "Opening: page 12000 of 40000…"
        );
        // The pages are read: the rest of the facts.
        reading.set(40_000);
        assert_eq!(opening_line(Some(&reading)), "Opening…");
    }
}
