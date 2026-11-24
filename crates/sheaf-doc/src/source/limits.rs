//! The caps every reader of a source runs under: one value for all of
//! them, given to the PDF layer's crates in their own terms at one place
//! each ([`ContentLimits::family`], [`Limits::fonts`], [`Limits::gfx`]).

/// The caps on reading a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The PDF reader's, on reading its objects: what an object stream or a
    /// cross-reference stream may decompress to, the sections in the
    /// `/Prev` chain, object numbers, and the depth of a resolution path
    /// or a chain of references.
    pub reader: crate::pdf::ReadLimits,
    pub page_tree: PageTreeLimits,
    pub forms: FormLimits,
    pub content: ContentLimits,
}

/// The caps on the page table's walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTreeLimits {
    /// Levels of nodes under the root, and nodes on a `/Parent` chain:
    /// PDFium's `kMaxPageLevel`.
    pub depth: usize,
    /// Pages the tree lists, nodes the walk visits, and a quarter of the
    /// kids it looks at: PDFium opens fewer than its `kPageMaxNum`,
    /// 0xFFFFF.
    pub pages: usize,
}

/// The caps on reading a form (`forms::read`): past one, the form is
/// damaged. The form check and the XFA reader read within the same ones,
/// and so do the checks of signatures' digests, past whose caps a
/// signature is one that can't be checked (`forms::sigs::Integrity`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormLimits {
    /// How deep the field tree may go.
    pub depth: usize,
    /// Field tree nodes read.
    pub nodes: usize,
    /// Entries in `/Fields` and every `/Kids` array the tree reaches. A
    /// field or widget listed twice is read once, but each list is looked
    /// through, so fields that share one `/Kids` array each cost its
    /// length.
    pub entries: usize,
    /// Widgets on pages.
    pub widgets: usize,
    /// The longest full name, in bytes, and all of them together. Real
    /// ones are under 2 KiB, and a parent's name is in each of its kids'.
    pub name: usize,
    pub names: usize,
    /// Items in an `/Opt` or a list box's value.
    pub items: usize,
    /// The longest value, script or other text, and all of them together.
    pub text: usize,
    pub texts: usize,
    /// The longest `/DA` read. Real ones are a few dozen bytes.
    pub da: usize,
    /// Name tree entries, kids and scripts, read for document scripts.
    pub scripts: usize,
    /// The most of an XFA copy read, decompressed.
    pub xfa: usize,
    /// Signatures whose digests a source checks, each a hash of nearly
    /// the whole file: a real file has a few.
    pub signatures: usize,
    /// The longest `/Contents` read as a signature's CMS. Real ones are
    /// tens of KiB, more with revocation data in them.
    pub cms: usize,
}

/// The caps on reading a page's content (`content`): past one, the content
/// can't be read, and an op that needs it fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentLimits {
    /// What a page's streams decompress to, together; where content is
    /// interpreted, with the forms and fonts it draws, and the work of
    /// drawing their programs' glyphs, in bytes of charstring and glyph
    /// data.
    pub decompressed: usize,
    /// Operands before an operator. PDFium keeps the last 16; a real
    /// operator takes at most 33 (`scn` with DeviceN's 32 components and
    /// a pattern), but a stream may hold runs of numbers no operator ends.
    pub operands: usize,
    /// How deep marked content nests: where content is interpreted, every
    /// range open across a form and what draws it. It's handed to
    /// pdf-content-streams for arrays and dictionaries too, which stop its
    /// tokens past 64 levels whatever this says (PDFium reads what's
    /// deeper as no object), so only a value under 64 lowers that.
    pub depth: usize,
    /// An inline image's data, in bytes.
    pub inline_image: usize,
    /// How deep Form XObjects are drawn within each other: PDFium's
    /// `kMaxFormLevel`. A use deeper than this is skipped and reported,
    /// as PDFium skips it, and the rest of the page is drawn; so is a use
    /// of a form it's drawn within, at once, where PDFium draws it to
    /// this depth.
    pub form_depth: usize,
    /// Graphics states `q` saved and `Q` hasn't restored, where content is
    /// interpreted, within each content stream (a form's apart from what
    /// draws it); elsewhere their balance is only counted. PDFium has no
    /// cap, so this one only bounds the memory they take, far above any
    /// real page's.
    pub states: usize,
    /// Operators interpreted on a page, and the codes its strings show,
    /// counted across every use of the forms it draws, so a form drawn
    /// many times costs each time.
    pub operators: usize,
    pub glyphs: usize,
    /// Bytes of content interpreted on a page, counted as `operators` are,
    /// so a form of whitespace or comments, which has few operators, still
    /// costs its length each time it's drawn.
    pub interpreted: usize,
    /// Path segments: of all those a page's content paints, counted as
    /// `operators` are, and of the clip and the path being built together,
    /// which bounds the memory a path and the clip take.
    pub segments: usize,
}

impl ContentLimits {
    /// These caps as pdf-content-streams takes them: its defaults, with
    /// `operands`, `depth` and `inline_image` as here, and its cap on a
    /// page's joined streams as `decompressed`. Every call into
    /// pdf-content-streams that takes limits takes them from here: a
    /// splice and a page's join from the document's, and the names a
    /// resources dictionary binds from sheaf's defaults
    /// ([`naming_limits`](crate::content::sink::naming_limits)).
    pub fn family(&self) -> crate::pdf::content::Limits {
        let mut limits = crate::pdf::content::Limits::DEFAULT;
        limits.operands = self.operands;
        limits.depth = self.depth;
        limits.inline_image = self.inline_image;
        limits.content = self.decompressed;
        limits
    }
}

impl Limits {
    /// These caps as pdf-fonts takes them: its defaults, with content's
    /// as [`ContentLimits::family`] gives them, which a font's CMaps and
    /// a Type 3 font's glyphs are read under, and with the CMaps one
    /// `/UseCMap` chain may run to as content's `depth`, the cap sheaf
    /// has always cut that chain by. Every font read through pdf-fonts
    /// takes its limits from here.
    pub fn fonts(&self) -> crate::pdf::fonts::Limits {
        let mut limits = crate::pdf::fonts::Limits::DEFAULT;
        limits.content = self.content.family();
        limits.cmaps = self.content.depth;
        limits
    }

    /// These caps as pdf-gfx interprets and writes content again under
    /// them: its defaults, with content's as [`ContentLimits::family`]
    /// gives them and fonts' as [`Limits::fonts`] does; how deep forms are
    /// drawn, the states saved, the operators, glyphs, path segments and
    /// bytes interpreted, and what's decompressed, as content's caps have
    /// them; and marked content's nesting as content's `depth`, the cap
    /// sheaf has always held it to. pdf-gfx counts some of them its own
    /// way: the states one content stream saves, not every stream open;
    /// every marked range open, a form's with its page's; a path's
    /// segments with the clip's. And content's `depth` holds less than it
    /// says: pdf-content-streams fails arrays and dictionaries nested
    /// past the 64 levels PDFium builds, whatever the cap (512). Every
    /// interpretation through pdf-gfx takes its limits from here.
    pub fn gfx(&self) -> crate::pdf::gfx::Limits {
        let c = &self.content;
        let mut limits = crate::pdf::gfx::Limits::DEFAULT;
        limits.content = c.family();
        limits.fonts = self.fonts();
        limits.form_depth = c.form_depth;
        limits.states = c.states;
        limits.operators = c.operators;
        limits.glyphs = c.glyphs;
        limits.segments = c.segments;
        limits.marked = c.depth;
        limits.interpreted = c.interpreted;
        limits.decompressed = c.decompressed;
        limits
    }

    pub const DEFAULT: Limits = Limits {
        reader: crate::pdf::ReadLimits::DEFAULT,
        page_tree: PageTreeLimits {
            depth: 1024,
            pages: 0xFFFFF - 1,
        },
        forms: FormLimits {
            depth: 32,
            nodes: 100_000,
            entries: 200_000,
            widgets: 100_000,
            name: 64 << 10,
            names: 64 << 20,
            items: 10_000,
            text: 1 << 20,
            texts: 64 << 20,
            da: 1024,
            scripts: 100_000,
            xfa: 64 << 20,
            signatures: 256,
            cms: 4 << 20,
        },
        content: ContentLimits {
            decompressed: 256 << 20,
            operands: 1 << 16,
            depth: 512,
            inline_image: 64 << 20,
            form_depth: 40,
            states: 1 << 16,
            operators: 1 << 24,
            glyphs: 1 << 24,
            interpreted: 1 << 30,
            segments: 1 << 23,
        },
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An interpretation's caps are the family's defaults with each of
    /// sheaf's content caps in its place, every field of sheaf's taken
    /// once: what each one of sheaf's is changed to, `gfx()` gives, and no
    /// other field of the family's moves with it.
    #[test]
    fn an_interpretation_s_limits_are_the_family_s_with_sheaf_s_caps() {
        let family = crate::pdf::gfx::Limits::DEFAULT;
        let lim = Limits::DEFAULT;
        let gfx = lim.gfx();
        assert_eq!(gfx.content, lim.content.family());
        assert_eq!(gfx.fonts, lim.fonts());
        let ours = |g: &crate::pdf::gfx::Limits| {
            [
                g.form_depth,
                g.states,
                g.operators,
                g.glyphs,
                g.segments,
                g.marked,
                g.interpreted,
                g.decompressed,
            ]
        };
        let c = &lim.content;
        assert_eq!(
            ours(&gfx),
            [
                c.form_depth,
                c.states,
                c.operators,
                c.glyphs,
                c.segments,
                c.depth,
                c.interpreted,
                c.decompressed,
            ]
        );
        // Sheaf's numbers aren't the family's, so each above is sheaf's.
        let unlike = (ours(&family).into_iter().zip(ours(&gfx))).filter(|(f, g)| f != g);
        assert_eq!(unlike.count(), 7, "all but form_depth, 40 in both");
        assert_eq!(family.form_depth, c.form_depth);

        // Each of sheaf's caps moves its own field and no other.
        type Set = fn(&mut ContentLimits, usize);
        let sets: [(&str, Set); 11] = [
            ("decompressed", |c, n| c.decompressed = n),
            ("operands", |c, n| c.operands = n),
            ("depth", |c, n| c.depth = n),
            ("inline_image", |c, n| c.inline_image = n),
            ("form_depth", |c, n| c.form_depth = n),
            ("states", |c, n| c.states = n),
            ("operators", |c, n| c.operators = n),
            ("glyphs", |c, n| c.glyphs = n),
            ("interpreted", |c, n| c.interpreted = n),
            ("segments", |c, n| c.segments = n),
            ("none", |_, _| {}),
        ];
        for (name, set) in sets {
            let mut moved = Limits::DEFAULT;
            set(&mut moved.content, 7);
            let g = moved.gfx();
            assert_eq!(g.content, moved.content.family(), "{name}");
            assert_eq!(g.fonts, moved.fonts(), "{name}");
            let c = &moved.content;
            let want = [
                c.form_depth,
                c.states,
                c.operators,
                c.glyphs,
                c.segments,
                c.depth,
                c.interpreted,
                c.decompressed,
            ];
            assert_eq!(ours(&g), want, "{name}");
        }
        // What sheaf has no cap of is the family's.
        let mut reader = Limits::DEFAULT;
        reader.reader.max_depth = 3;
        reader.forms.depth = 3;
        reader.page_tree.depth = 3;
        assert_eq!(reader.gfx(), gfx);
    }
}
