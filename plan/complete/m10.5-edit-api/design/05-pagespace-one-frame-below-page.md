# M10.5 §5: PageSpace: one frame below the page

Part of [M10.5: the design](README.md).


**PageSpace** is the page's default user space: points, y up, before
`/Rotate`, independent of every box. It is the space of `/MediaBox`,
`/CropBox`, `/Rect`, `/QuadPoints` and `/XYZ`, and the space widgets
already use. Everything anchored to a page is stored in it:
- marks, others' annotation rects and widgets;
- OCR words (with `upright` for reading order);
- bookmark `DestView` and link rects;
- generator placement, which is computed;
- text-service glyph boxes, so the 16× device trick goes;
- region events and query results;
- Op JSON.

```rust
impl Geom {
    pub fn visible(&self) -> Rect;                 // crop ∩ media
    pub fn shown(&self) -> PageSize;               // visible, rotated
    pub fn to_view(&self) -> Affine;               // PageSpace → displayed (top-left of visible box, y down, after rotate)
    pub fn from_view(&self) -> Affine;
}
```

- **Displayed space is derived from `Geom::to_view` alone.**
  `save::geom_of`, `file::page_geom` and `annot::PageGeom` collapse into
  `Geom`.
- **Rotation is absolute model data.** The render `Job.rotation` is
  still a delta, `geom.rotate − base /Rotate`, so rotating a page needs
  no realization, and rotation is not in the render key.
- **A crop is a box edit.** Nothing stored moves. `Rebase.shift`,
  `TextLayer::shifted` and rebasing via `AnnotKind::translated` are
  deleted. (`translated` stays for drag and paste.)
- **The edges convert once, and there is more than one edge:**
  - **Pointer:** through `from_view`, composed with the pane's view
    rotation.
  - **Clipboard:** copied marks carry their geometry in the *source
    page's view frame*. Paste converts through the target page's
    `from_view`, which also derives `turn`, so a mark pasted onto a page
    with another `/Rotate` or CropBox origin keeps its visual position.
  - **Moves:** `MoveMarks { by: Delta }` with
    `Delta = Page(Vec2) | View(Vec2)`. A `View` delta is converted per
    mark through its own page's frame. Moving to another page maps
    through the source page's `to_view` and the target's `from_view`.
  - **Plugins:** WIT `page-info` carries `to-view` and `from-view`
    matrices. sheaf-plugin-api provides `to_view`/`from_view` helpers,
    and text and region results can return view rects on request.
    Placing something "at the top right as seen" therefore needs no
    `/Rotate` arithmetic.
  - **OCR:** pixels convert through the `Geom` and render transform
    captured when the page was rendered, never the commit-time `Geom`.
  - **XFDF and FDF:** rects are already user space.
- **A property test (slice 5).** For every `/Rotate` × view rotation ×
  CropBox offset × MediaBox not at the origin, `to_view ∘ from_view` is
  the identity within 1/16 pt.
