//! The resource names content uses: what a page's `/Resources` must keep
//! for it, by category, for pruning what a removal leaves unused; and what
//! content keeps once ranges of it are taken out, its balance and names
//! with those only the ranges used, in one pass ([`kept`]).

use std::collections::BTreeSet;

use super::balance::Balance;
use super::lex::{self, LexError, ResCx, Tok};
use super::page::{Concat, SpanAcross};
use crate::source::Limits;

/// A resource category: a key of `/Resources`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResKind {
    ColorSpace,
    ExtGState,
    Font,
    Pattern,
    Properties,
    Shading,
    XObject,
}

impl ResKind {
    pub const ALL: [ResKind; 7] = [
        ResKind::ColorSpace,
        ResKind::ExtGState,
        ResKind::Font,
        ResKind::Pattern,
        ResKind::Properties,
        ResKind::Shading,
        ResKind::XObject,
    ];

    /// Its key in `/Resources`.
    pub fn key(self) -> &'static [u8] {
        match self {
            ResKind::ColorSpace => b"ColorSpace",
            ResKind::ExtGState => b"ExtGState",
            ResKind::Font => b"Font",
            ResKind::Pattern => b"Pattern",
            ResKind::Properties => b"Properties",
            ResKind::Shading => b"Shading",
            ResKind::XObject => b"XObject",
        }
    }
}

/// Resource names, each in its category, decoded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameSet {
    names: BTreeSet<(ResKind, Vec<u8>)>,
}

impl NameSet {
    pub fn contains(&self, kind: ResKind, name: &[u8]) -> bool {
        self.names.contains(&(kind, name.to_vec()))
    }

    /// The names of `kind`, in order.
    pub fn of(&self, kind: ResKind) -> impl Iterator<Item = &[u8]> {
        self.names
            .range((kind, Vec::new())..)
            .take_while(move |(k, _)| *k == kind)
            .map(|(_, name)| name.as_slice())
    }

    /// Every name, each in its category, in order.
    pub fn iter(&self) -> impl Iterator<Item = (ResKind, &[u8])> {
        self.names
            .iter()
            .map(|(kind, name)| (*kind, name.as_slice()))
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn remove(&mut self, kind: ResKind, name: &[u8]) {
        self.names.remove(&(kind, name.to_vec()));
    }

    fn insert(&mut self, kind: ResKind, name: Option<impl Into<Vec<u8>>>) {
        if let Some(name) = name {
            self.names.insert((kind, name.into()));
        }
    }
}

/// The resource names `content` uses, lexed with `res`, but for those
/// used only by steps inside `except` (ranges about to be taken out):
/// - `Tf`'s font, `Do`'s XObject, `gs`'s ExtGState and `sh`'s shading;
/// - `cs` and `CS`'s color space, and an inline image's, but for the
///   device spaces and `/Pattern`, which aren't resources;
/// - `scn` and `SCN`'s pattern, when the last operand is a name;
/// - `BDC` and `DP`'s property list, when it's named.
pub fn names_used(
    content: &Concat,
    res: &ResCx,
    lim: &Limits,
    except: &[SpanAcross],
) -> Result<NameSet, LexError> {
    Ok(kept(content, res, lim, except)?.names)
}

/// What's left of `content` once `drop` is taken out, read in one pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Kept {
    /// How its `q`s and `Q`s balance.
    pub balance: Balance,
    /// The resource names it uses ([`names_used`]).
    pub names: NameSet,
    /// The names only what's taken out uses, which the resources of what's
    /// kept can go without.
    pub dropped: NameSet,
}

/// What `content`, lexed with `res`, keeps once the steps inside `drop`
/// (ranges about to be taken out, whole streams among them) are taken out.
pub fn kept(
    content: &Concat,
    res: &ResCx,
    lim: &Limits,
    drop: &[SpanAcross],
) -> Result<Kept, LexError> {
    let drop: Vec<_> = drop.iter().filter_map(|&s| content.offsets(s)).collect();
    let s = content.bytes();
    let mut kept = Kept::default();
    let mut inside = NameSet::default();
    lex::steps(s, res, lim, |step| {
        let range = step.range();
        let dropped = drop
            .iter()
            .any(|d| d.start <= range.start && range.end <= d.end);
        let names = match dropped {
            true => &mut inside,
            false => &mut kept.names,
        };
        let name = |i| step.name(i);
        match step.tok {
            Tok::Op(b"Tf") => names.insert(ResKind::Font, name(1)),
            Tok::Op(b"Do") => names.insert(ResKind::XObject, name(0)),
            Tok::Op(b"gs") => names.insert(ResKind::ExtGState, name(0)),
            Tok::Op(b"sh") => names.insert(ResKind::Shading, name(0)),
            Tok::Op(b"cs" | b"CS") => {
                names.insert(ResKind::ColorSpace, name(0).filter(|n| !device(n)));
            }
            Tok::Op(b"scn" | b"SCN") => names.insert(ResKind::Pattern, name(0)),
            Tok::Op(b"BDC" | b"DP") => names.insert(ResKind::Properties, name(0)),
            Tok::InlineImage(_) => {
                let keys: &[&[u8]] = &[b"CS", b"ColorSpace"];
                let cs = lex::image_entry(s, step.at.clone(), lim.content.depth, keys);
                let cs = cs
                    .and_then(lex::name)
                    .filter(|n| !device(n) && !abbreviated(n));
                names.insert(ResKind::ColorSpace, cs);
            }
            _ => {}
        }
        if !dropped && let Tok::Op(op) = step.tok {
            kept.balance.take(op);
        }
        Ok::<_, LexError>(())
    })?;
    kept.dropped.names = (inside.names)
        .into_iter()
        .filter(|n| !kept.names.names.contains(n))
        .collect();
    Ok(kept)
}

/// Whether a color space name is a device space or `/Pattern`: no
/// resource.
fn device(name: &[u8]) -> bool {
    matches!(
        name,
        b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK" | b"Pattern"
    )
}

/// Whether a color space name is an inline image's abbreviation.
fn abbreviated(name: &[u8]) -> bool {
    matches!(name, b"G" | b"RGB" | b"CMYK" | b"I")
}
