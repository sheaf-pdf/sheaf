# M5 proposal 9: The device model

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**The device model.**
```rust
pub struct DeviceInfo {
    pub id: DeviceId,             // "sane:fujitsu:fi-7160:12345", "twain64:PaperStream IP fi-7160", "wia:{…}"
    pub name: String,             // as the driver names it
    pub maker: String, pub model: String,
    pub driver: Driver,           // Sane | Twain { bits } | Wia | Fake
}
pub struct Caps {
    pub sources: Vec<Source>,     // Flatbed | Feeder { duplex: bool }
    pub resolutions: Vec<u32>,    // or a range, listed
    pub modes: Vec<Mode>,         // Color | Gray | Bw
    pub max_area: SizeMm,
}
pub struct Settings {
    pub source: Source, pub dpi: u32, pub mode: Mode,
    pub paper: Paper,             // Auto (the driver's own size detection) | Letter | Legal | A4 | SizeMm
}
pub struct RawPage { pub image: PageBytes, pub dpi: (f32, f32), pub sheet: u32, pub side: Side /* Front | Back */ }
pub enum ScanError { FeederEmpty, Jammed, DoubleFeed, CoverOpen, Busy, Gone, Stopped /* watchdog */, Driver(String) }
pub enum Done { FeederEmpty, OnePage /* flatbed */, Cancelled }
```
- Settings are clamped to the device's range (the SANE `test`
  backend's area stops at 200 mm), and what was clamped is said.
- The client is synchronous and runs on a background thread, with
  events on a channel, as the OCR job's are (`OcrEvent`).
- `fake` is a backend scripted in tests: pages, sides, a jam at
  sheet 4, a hang, a crash.
