# Build or adopt 101: Needs that need no crate

Area: Needs with no crate, and notes · Part of [build or adopt](README.md)

- **Opening links:** gpui-pre has `App::open_url` (`app.rs:1809`), so
  "Follow links in the page view" (`ROADMAP.md:756-766`) needs no crate.
- **Dragging pages between windows** (`ROADMAP.md:865-877`): GPUI work.
- **A binder index page:** an M13 link op.
- **The calculator items F1–F4:** the tape is f64 with sheaf's own
  rounding (`crates/sheaf-edit/src/tape.rs:273-320`). rust_decimal
  1.43.0 (MIT) exists if exact decimal arithmetic is ever wanted; no
  entry asks for it.
