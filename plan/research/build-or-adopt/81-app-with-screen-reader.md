# Build or adopt 81: The app with a screen reader (AccessKit through gpui-pre)

Area: M17: Accessibility (and M7's text with layout) · Part of [build or adopt](README.md)

- **Need:** roles, labels and states on every control; announcements; a
  tree of its own for the drawn page view; a way to test it all.
- **Where:** `ROADMAP.md:546-574`.
- **Today's plan:** "GPUI (gpui-pre 0.3.9) reports its UI through
  AccessKit … An element with an id and a role (`.role(Role::Button)`) is
  reported" (`:549-552`). "Open: how to test it. GPUI has an `a11y` debug
  module, and AccessKit trees can be read back; neither is explored yet"
  (`:572-574`).
- **What gpui-pre 0.3.9 has** (its registry source,
  `src/_accessibility.rs` and `src/window.rs`):
  - `.role()` and `on_a11y_action(AccessibleAction::…)`; `.on_click()`
    registers `Click` by itself.
  - `Element::a11y_synthetic_children` with an `A11ySubtreeBuilder`: "a
    custom Element may want to appear as if it is really made of multiple
    nodes". The hook for the page view's text, marks and fields; it runs
    after prepaint, so it knows what's visible.
  - `Window::debug_a11y_tree_json()` (`window.rs:7011`): "the last
    frame's accessibility information".
  - `is_a11y_active()` is true only while assistive technology is
    connected; `Application::new_inaccessible` turns a11y off.
  - Whether a headless `#[gpui::test]` can build a tree without an AT
    connected is unverified: no a11y code was found in its test platform.
- **Candidates:** [accesskit](https://crates.io/crates/accesskit) 0.25.1
  (2026-09-25; MIT OR Apache-2.0, its adapters too). sheaf locks 0.24.1
  through gpui-pre, with accesskit_windows 0.34.0, accesskit_macos 0.26.3
  and accesskit_unix 0.22.1 (AT-SPI). `accesskit_consumer` 0.38 (in the
  lock) can walk a tree in tests.
- **Fit:** everything goes through GPUI, so there's no direct dependency
  to choose. Flatpak needs access to the a11y bus (unverified; check with
  the packaging work).
- **Recommendation:** **adopt** (through gpui-pre). Spike first:
  `debug_a11y_tree_json` in a headless test, and
  `a11y_synthetic_children` on the page view. Medium-high.
