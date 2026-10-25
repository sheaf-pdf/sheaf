# M5 proposal 1: One intake pipeline for files and devices

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**One intake pipeline for files and devices.** `sheaf_ops::intake`
takes pages from a stream and lands them in a document a batch at a
time. Today's `sheaf_ops::scan` becomes the same pipeline fed from
image files.
- **The stream:** `trait Pages { fn next(&mut self, p) ->
  Result<Option<RawPage>> }`. Files give a decoded image a page (a
  TIFF's pages one by one); a scanner gives what it scans as it scans
  it, through the worker's client (proposal 4).
  - As built (slice 1): `Pages` also has `count()`, how many pages
    the stream has when it knows (image files count a TIFF's from
    its headers, `io::count_frames`), for progress, which counts
    pages; and intake's page is `intake::RawPage { image: RgbImage,
    name }`, the name for messages ("fax.tif, page 2: blank, left
    out"). Proposal 9's `RawPage` (bytes, dpi, sheet, side) is
    sheaf-scanner's, slice 6's, which maps a device's pages into
    intake's and may add what it needs then. A TIFF is read an image
    at a time from the file (`sheaf_image::io::frames`): its first
    page is `io::load`'s image, as `scan` read every file before, and
    its later ones are decoded as images-to-pdf decodes them;
    reduced-resolution images (thumbnails) aren't pages.
  - As built (slice 1): whatever stops an intake but its landing (a
    cancel, a page that can't be read, cleaned or recognized), the
    pages it had ready land first, so the pages so far stay (one undo
    step), and its error is returned. A cancel cuts short the page
    it comes during (that page's cleanup or OCR), which is left out.
- **A page at a time:** clean (proposal 3), then blank (leave out,
  with `sheaf_image::blank` as today), then separators (proposal 15,
  only when asked: a separator is never OCRed), then OCR (one engine,
  built once, as today).
- **Batches:** every 8 pages, or 2 seconds, whichever comes first,
  the batch becomes one `MemoryKind::Images` source (`made_pages`)
  and one commit: an `InsertPages`, plus `SetOcrText` for its pages.
  - As built (slice 1): the 2 seconds are since the last landing (the
    intake's start, for the first), and `Pages::next` is a blocking
    pull, so the clock is read as each page is done
    (`Batching::due(pages, since_last)`): a slow device's page lands
    right after it's cleaned, and a fast one waits for the page after
    it at most. `Batching::DEFAULT` is the app's and the devices'
    (slices 2, 6 and 7). The command line batches by count only
    (`scan::BATCHING`, 8 pages and no clock): a Rewrite numbers what
    an insert brought by its source, each batch's own, so batches cut
    by a clock would write the same pages in another object order;
    by count, the same images make the same file (recognized text's
    block ids aside, which are drawn at random).
  The commits of one intake are one group (a new `GroupKind::Scan`,
  made by `Actor::Job(JobKind::Scan)`), so they're one undo step, as an
  OCR run's flushes are; undoing it mid-scan stops the scan, as it
  stops OCR.
- **Where it lands** is a trait too: a `Session` (the command line),
  or a document's planner in the app (the open document for Insert,
  a new one for Create). The app's batches commit as page inserts do
  (`plan_made`, `Base::At`), so the user can keep working while pages
  arrive.
  - As built (slice 2): every landing makes a batch's source in its
    document's store (`intake::batch_source`, `made_pages` before) and
    puts its insert at the end and its pages' text into a transaction
    it gives (`intake::apply_batch`), so a Session's landing and the
    app's share them; `plan_made` makes the insert alone.
  - As built (slice 2): the app's landing (`scan_run.rs`) plans each
    batch on the intake's own thread, on the version the view hands
    it with the scan job's lowering, and the view commits it with
    `Base::At` and `Stale::Relower` in the scan's group. Planned in a
    background task of its own, as a planner's edit is, a batch would
    need a free background thread while the intake's waits for the
    answer: with as many scans running as there are threads, none
    would be. An insert at the end reads no order (a sheaf-doc test
    pins it), so no edit of the user's conflicts with a batch, and
    none is planned again.
- **Memory stays flat.** A page is encoded as soon as it's cleaned
  (JPEG for color and gray, CCITT G4 for black and white) and its
  pixels dropped. Today every page is held to the end, and a black and
  white page is held as 8-bit pixels (`Pixels::Bilevel`, 8.4 MB for
  Letter at 300 dpi): 500 pages hold 4.2 GB (found while planning).
  - As built (slice 1): a black and white page is held as its G4
    bytes from the moment it's made, by every reader
    (`Pixels::G4`, `PageImage::from_image`). The gate
    (`scan_of_500_black_and_white_pages` in gate_7k) asserts under
    400 MB added under both profiles: Photo, the command's default
    and the path the 4.2 GB was measured on, and Scanner.
- Alternatives: keep `scan` as it is and write a second path for
  devices (two pipelines to keep alike); land everything at the end
  (nothing to see until the feeder is empty, and nothing recoverable
  if sheaf dies at page 400).
