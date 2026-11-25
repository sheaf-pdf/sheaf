//! The fallback font (M10 slice 7): a TrueType program sheaf bundles, Noto
//! Sans Regular, which draws what the font text is in can't: Latin, Greek
//! and Cyrillic past what a standard 14 font's encoding holds.
//!
//! A file embeds a subset of it, the glyphs the file draws in it and no
//! others, written by pdf-fonts ([`embed_write`]): a Type 0 font whose
//! codes are each glyph's number in the whole program, its program cut to
//! the glyphs drawn behind a `/CIDToGIDMap` stream, with each glyph's
//! width in `/W` and its text in `/ToUnicode`. So the codes a string is
//! drawn with depend only on the text, whatever else a file draws in the
//! font.
//!
//! **Bound late.** Content that draws in it names one resource,
//! [`Fallback::res`]: a marker, the same in every output, so a drawing is
//! the same bytes whatever else is drawn beside it, and never written.
//! What a drawing draws in it travels with what it draws with ([`Uses`]),
//! and whatever writes the drawing's resources joins its glyphs where it
//! binds them ([`Sink::fallback`], which [`sink::bind`] calls): the sink
//! gives the marker the object number the subset will be, and writes the
//! subset ([`Fallback::subset`]) once every glyph that binding draws is
//! joined.
//!
//! [`Sink::fallback`]: crate::content::sink::Sink::fallback
//! [`sink::bind`]: crate::content::sink::bind

use std::sync::{Arc, OnceLock};

use crate::content::{Res, ResBody, ResId, ResourceNeeds};
use crate::pdf::fonts::{EmbedOptions, Glyphs, Program, embed_write};
use crate::pdf::forms::FieldFallback;
use crate::pdf::{Object, dictionary};

/// The font sheaf bundles to draw what another font can't
/// ([`Fallback::SANS`]): Noto Sans Regular, unhinted, from the Noto
/// project's release v2.015 (`assets/NotoSans-Regular.ttf`; SIL Open Font
/// License 1.1, `assets/LICENSE-NotoSans.txt`).
///
/// There is one. Text chooses it by name where it draws ([`DrawFont`]'s
/// `encode` and `width`), and a sink joins every glyph drawn in it into
/// one set ([`Sink::fallback`]), as do the values that carry them
/// ([`Uses`], a save's record): a second face would give each of those
/// its face, not only this type a second value.
///
/// [`DrawFont`]: crate::content::DrawFont
/// [`Sink::fallback`]: crate::content::sink::Sink::fallback
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fallback(());

/// The marker's id: a number no resource's objects digest to, as it's
/// no digest, so nothing a file holds is taken for it. Content names the
/// marker by it, so by the same name in every output.
const SANS_MARKER: ResId = ResId(0x6a2e_91c4_57fd_0b38_e1a9_43c6_0d7b_25f2);

impl Fallback {
    /// Noto Sans Regular.
    pub const SANS: Fallback = Fallback(());

    /// Its program's bytes, as the release has it.
    const SANS_PROGRAM: &[u8] = include_bytes!("../../../assets/NotoSans-Regular.ttf");

    /// Its program, opened once.
    pub fn program(self) -> &'static Program {
        static SANS: OnceLock<Program> = OnceLock::new();
        SANS.get_or_init(|| {
            Program::open(Fallback::SANS_PROGRAM, 0)
                .expect("the bundled program is a TrueType font pdf-fonts opens")
        })
    }

    /// The resource content names it by: a marker the same in every
    /// output, never written. A sink gives it the object number its
    /// subset is written at ([`Fallback::is`] tells it).
    pub fn res(self) -> Res {
        static SANS: OnceLock<Res> = OnceLock::new();
        SANS.get_or_init(|| {
            // What it is, said, should a reader ever see it; it binds
            // nothing a reader draws.
            let marker = dictionary! {
                "Type" => Object::name("Font"),
                "Subtype" => Object::name("Type0"),
                "BaseFont" => Object::name("NotoSans-Regular"),
            };
            let objects: Arc<[Object]> = Arc::from([Object::from(marker)]);
            Res::named(SANS_MARKER, ResBody::Objects(objects)).expect("a dictionary is a resource")
        })
        .clone()
    }

    /// It as a text field's appearance draws in it ([`FieldFallback`],
    /// pdf-forms'): its program, named by its marker, so a field's
    /// appearance binds it as any other drawing does.
    pub fn field(self) -> FieldFallback<'static> {
        FieldFallback::new(self.res(), self.program())
    }

    /// Whether `res` is its marker ([`Fallback::res`]).
    pub fn is(self, res: &Res) -> bool {
        res.id() == SANS_MARKER
    }

    /// Its subset of `glyphs`, as a resource: the objects pdf-fonts writes
    /// ([`embed_write`]), the Type 0 font first, named by their digest, so
    /// equal glyphs give an equal resource. A glyph past the program's
    /// (another program's, which is a bug) is left out and logged.
    pub fn subset(self, glyphs: &Glyphs) -> Res {
        let program = self.program();
        let count = program.glyph_count();
        let past = glyphs.iter().filter(|&(glyph, _)| glyph >= count).count();
        let kept;
        let glyphs = match past {
            0 => glyphs,
            _ => {
                log::error!("{past} glyphs past the fallback font's {count} are left out of it");
                let mut within = Glyphs::new();
                for (glyph, text) in glyphs.iter().filter(|&(glyph, _)| glyph < count) {
                    within.insert(glyph, text);
                }
                kept = within;
                &kept
            }
        };
        // Its licence lets it be embedded and cut (`tests`), and pdf-fonts
        // cuts any glyphs of a program it opened.
        let objects = embed_write(program, glyphs, &EmbedOptions::default())
            .expect("the bundled program is cut to any of its glyphs");
        Res::of(objects).expect("a font's objects are one at least, their streams in memory")
    }
}

/// Whether a subset written of `subset` ([`Fallback::subset`]) draws every
/// glyph of `glyphs`, each read back as the text it keeps for it: joined
/// to it, nothing changes. What lets a save bind a subset a file already
/// holds rather than write another.
pub fn draws_all(subset: &Glyphs, glyphs: &Glyphs) -> bool {
    glyphs.len() <= subset.len() && {
        let mut joined = subset.clone();
        joined.merge(glyphs);
        joined == *subset
    }
}

/// What content draws with: the resources it names ([`ResourceNeeds`]),
/// and the glyphs it draws in the fallback font ([`Glyphs`]), which
/// whatever binds the resources joins ([`sink::bind`]). Glyphs are a
/// drawing's value, never a side effect, so a drawing kept and bound
/// again still says what it draws.
///
/// [`sink::bind`]: crate::content::sink::bind
#[derive(Debug, Clone, Default)]
pub struct Uses {
    pub needs: ResourceNeeds,
    pub glyphs: Glyphs,
}

/// Content that draws nothing in the fallback.
impl From<ResourceNeeds> for Uses {
    fn from(needs: ResourceNeeds) -> Uses {
        Uses {
            needs,
            glyphs: Glyphs::new(),
        }
    }
}
