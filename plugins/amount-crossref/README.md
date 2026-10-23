# Amount Cross-Reference

Ships with sheaf. Finds dollar amounts that appear on more than one page,
the way a K-1 total shows up again on the 1040 line it flows to, and lists
each candidate pair in the **Amount Matches** panel. **Link** places a
numbered connector pair just right of both amounts as their pages show,
turned or not (one undo step); **Dismiss** drops the pair. **Tools →
Cross-Reference → Link All Matches** links every pair still listed.

Tools → Cross-Reference → Find Matching Amounts… asks for the smallest
amount worth matching, which pages to scan, and whether to skip round
numbers (which match by coincidence more often).

It reads page text through `text.pattern-search`, so OCR text that hasn't
been saved yet counts too. The pairs it lists are the document's as it was
open then: after it's opened again they aren't linked, and Find Matching
Amounts finds them again.

The matching logic has unit tests that run on the host: `cargo test`.
