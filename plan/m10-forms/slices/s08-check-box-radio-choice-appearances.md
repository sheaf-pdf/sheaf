# M10 slice 8: Check box, radio and choice appearances

Status: open · Part of [M10: Forms](../README.md)

**Check box, radio and choice appearances** (proposal 10).
- **On the edit API:** pure generators beside slice 6's
  (`appearance::button`, `appearance::choice`). A group's `/AS`
  semantics (radio groups, NoToggleToOff, RadiosInUnison, check boxes
  acting as radios) are lowering rules of `SetFieldValues`: setting a
  group's value puts each widget's `/AS`. Slice 9 adds the op; this
  slice's rules are what its lowering calls, tested on their own.
- On-states from `/AP /N` (`/1` ×394, `/Yes`, `/On`, `/spouse`,
  `/Choice1`…), set through `/V` and `/AS`. Missing appearances are
  made from `/MK /CA` in ZapfDingbats (`4` ×505, `8` ×254, `6`, `l`),
  centered by its widths, with `/D` appearances. An off state that
  draws nothing stays so: 757 button widgets (746 check-box widgets
  and 11 radio widgets) have no `/Off` in `/N`, which is Adobe's
  usual way.
- Radio groups with NoToggleToOff and RadiosInUnison, and the 37 check
  box groups whose widgets have distinct on-states and act as radios
  (GA, NY, IL, OH, PA, VA).
- `/Opt` export values on check boxes and radios (8 fields in CA 540
  and NJ-1040), used in `/V` and in data export.
- Combo boxes, editable or not, and list boxes with single or
  multiple selection (`/TI`, `/I`, the selection highlight).
- Push buttons keep their appearances; a missing one is drawn from
  `/MK /CA`.
- **Checks:** pixel checks for each kind and state; a radio-like check
  box group turns its other widgets off; a CA 540 check box with
  `/Opt` writes its export value.
