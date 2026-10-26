//! Which objects share an object stream: grouped by the pages that use
//! them, in the parts Annex F lays a linearized file out in, so the
//! linearization crate (between milestones, item 3) can reuse it. A pure
//! function of each object's number and its use, with no lopdf or
//! materializer types in it.

/// The most objects a container holds: qpdf's default.
pub const MEMBERS: usize = 100;

/// Which pages use an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// Only page `i`, from 0.
    Page(u32),
    /// More than one page.
    Shared,
    /// No page: the catalog, the page tree, the form, outlines, names, and
    /// anything no page reaches.
    None,
}

/// The containers `objects` go in, each listed once by number: their
/// members by number, in order. Objects are ordered in parts, as Annex F
/// lays out a linearized file: those only page 1 uses; those a single
/// later page uses, by page; those pages share; then the rest. Numbers
/// ascend within each part, and within each page's. Containers fill in
/// that order, up to [`MEMBERS`] each, and the first part's last container
/// ends with it, so page 1's objects never share one with a later page's.
pub fn group(objects: impl IntoIterator<Item = (u32, Use)>) -> Vec<Vec<u32>> {
    let mut ordered: Vec<((u8, u32), u32)> = (objects.into_iter())
        .map(|(n, used)| (part(used), n))
        .collect();
    ordered.sort_unstable();
    let first = ordered.partition_point(|&((part, _), _)| part == 0);
    let (first, rest) = ordered.split_at(first);
    (first.chunks(MEMBERS).chain(rest.chunks(MEMBERS)))
        .map(|members| members.iter().map(|&(_, n)| n).collect())
        .collect()
}

/// Where an object `used` so goes: its part, and its page in the second.
fn part(used: Use) -> (u8, u32) {
    match used {
        Use::Page(0) => (0, 0),
        Use::Page(i) => (1, i),
        Use::Shared => (2, 0),
        Use::None => (3, 0),
    }
}
