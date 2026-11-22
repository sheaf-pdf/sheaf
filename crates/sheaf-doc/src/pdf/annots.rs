//! Annotations through `pdf-annots` (M18 slice 24): an appearance as the
//! parts of the form XObject that holds it ([`SynthesizedAp`]: its box,
//! its matrix, its content and what that draws with, by what each is),
//! whether the family drew it or sheaf did, on pdf-gfx's
//! [`Canvas`](super::gfx::Canvas) ([`SynthesizedAp::drawn`]). A mark's
//! appearance reaches the writer as one, whoever drew it, and the writer
//! makes its form from those parts (`crate::content::sink::form`).

pub use pdf_annots::SynthesizedAp;
