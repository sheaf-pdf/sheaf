# M11 slice 6: Apply: images, line art, annotations

Status: open · Part of [M11: Security & redaction](../README.md)

**Apply: images, line art, annotations** (proposal 7).
- Pixel replacement and re-encoding, cloning shared images, masks,
  inline images.
- Path clipping and the crate for it.
- Shadings, by the stated limit.
- Annotations and widgets under marks; `/Thumb`.
- Check: pixels under marks are the fill color in the image data
  itself, not just in a render. Outside the marks, renders match
  within a tolerance.
