# Subtotal

Ships with sheaf. **Shift+drag** a rectangle over numbers on a page, with any
tool, and the status bar shows their sum and count, updating as you drag,
like a spreadsheet's status-bar sum. Escape or a plain click clears the
region.

Every number in the region counts, including unsaved OCR text: `$1,234.56`,
`1,234`, `(1,234.00)` and `1,234-` as negatives, and a lone dash as zero.
Percentages and words with letters (`W-2`, `1099-INT`) don't count.
Arithmetic is exact to four decimal places, and the total shows as many
decimals as the most precise number.

It reads each page's words once and filters them as the region moves, so
dragging stays cheap. It reads a page's words again only after a change
that can change them: to the page's text (its content, or text recognized
on it), or to a header, footer or watermark, or the page order or labels a
header's page numbers read. A change to marks alone keeps them.

The parsing and arithmetic have unit tests that run on the host: `cargo test`.
