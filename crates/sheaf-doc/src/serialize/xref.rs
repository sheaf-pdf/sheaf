//! Cross-reference sections, as a table or a stream. A full write's
//! section is one subsection, `[0, Size)`, listing every number it doesn't
//! write as free; an append's is runs of the numbers it lists, which for
//! an update are only those it writes: an update frees nothing (M10.5
//! slice 10). Free entries are linked in a list from entry 0. Positions are written
//! from the header: PDFium, pdf.js and qpdf count them from `%PDF-`, past
//! any bytes a writer put before it. An object in an object stream is
//! listed as its container and its place there (7.5.8.3's type 2), which
//! only a stream can say. A full write's stream is deflated under the PNG
//! Up predictor, as qpdf writes its own; an update's is written
//! uncompressed, as it was (an Append is unchanged: B62).

use std::collections::BTreeMap;
use std::io::Write as _;

use lopdf::{Dictionary, Object, Stream, dictionary};

use super::{Error, object};

/// What a section says of an object number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// Written at file position `pos`.
    InUse { pos: usize, generation: u16 },
    /// Free, to be used next at `generation`.
    Free { generation: u16 },
    /// The `index`th object in object stream `container`, at generation 0.
    Packed { container: u32, index: u16 },
}

/// A cross-reference section's entries, by object number.
#[derive(Debug, Clone)]
pub struct Section {
    entries: BTreeMap<u32, Entry>,
    /// The trailer's `/Size`: past every number listed.
    size: u32,
    /// Where the header starts in the file.
    header: usize,
    /// A full write's one subsection, else an append's runs.
    full: bool,
}

/// A row as written: its type (0 free, 1 in use, 2 in an object stream),
/// its offset from the header, the next free number or its container, and
/// its generation or its place in the container.
struct Row {
    kind: u8,
    field: u64,
    generation: u16,
}

impl Section {
    /// A full write's, of at least `size` numbers. It writes its own
    /// header, at the start.
    pub fn full(size: u32) -> Section {
        Section {
            entries: BTreeMap::new(),
            size,
            header: 0,
            full: true,
        }
    }

    /// An append's, after a file whose `/Size` is `size` and whose header
    /// starts at `header`.
    pub fn append(size: u32, header: usize) -> Section {
        Section {
            entries: BTreeMap::new(),
            size,
            header,
            full: false,
        }
    }

    /// Lists object `num` as `entry`, `/Size` growing past it as far as a
    /// u32 goes. Entry 0 is the free list's, the section's own.
    pub fn insert(&mut self, num: u32, entry: Entry) {
        debug_assert_ne!(num, 0, "object 0 heads the free list");
        self.entries.insert(num, entry);
        self.size = self.size.max(num.saturating_add(1));
    }

    /// The trailer's `/Size`: past every number listed.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Whether it lists object `num`.
    pub fn lists(&self, num: u32) -> bool {
        self.entries.contains_key(&num)
    }

    /// The section as a table at file position `pos`, then `trailer` with
    /// its `/Size`, and `startxref`. A table can't list an object stream's
    /// member, so one is refused.
    pub fn table(
        &self,
        out: &mut Vec<u8>,
        mut trailer: Dictionary,
        pos: usize,
    ) -> Result<(), Error> {
        let start = out.len();
        out.extend_from_slice(b"xref\n");
        for (first, rows) in self.subsections() {
            let _ = writeln!(out, "{first} {}", rows.len());
            for (num, row) in (first..).zip(rows) {
                if row.kind == 2 {
                    out.truncate(start);
                    return Err(Error::Packed(num));
                }
                if row.field > 9_999_999_999 {
                    out.truncate(start);
                    return Err(Error::TooFar(row.field));
                }
                let kind = if row.kind == 1 { 'n' } else { 'f' };
                let _ = writeln!(out, "{:010} {:05} {kind} ", row.field, row.generation);
            }
        }
        trailer.set("Size", i64::from(self.size));
        out.extend_from_slice(b"trailer\n");
        object::write(out, &Object::Dictionary(trailer)).inspect_err(|_| out.truncate(start))?;
        let _ = write!(out, "\nstartxref\n{}\n%%EOF\n", pos - self.header);
        Ok(())
    }

    /// The section as a stream, object `num` at file position `pos`, which
    /// lists itself, then `startxref`. `trailer`'s entries are its
    /// dictionary's. `/W` is as narrow as the widest field needs. A full
    /// write's rows are deflated under the PNG Up predictor, each less the
    /// row above it, which leaves mostly zeros where offsets climb slowly;
    /// an append's are written as they are.
    pub fn stream(
        mut self,
        out: &mut Vec<u8>,
        num: u32,
        mut trailer: Dictionary,
        pos: usize,
    ) -> Result<(), Error> {
        self.insert(num, Entry::InUse { pos, generation: 0 });
        let subsections = self.subsections();
        let widest = subsections
            .iter()
            .flat_map(|(_, rows)| rows.iter().map(|r| r.field))
            .max()
            .unwrap_or(0);
        let width = (u64::BITS - widest.leading_zeros()).div_ceil(8).max(1) as usize;
        let columns = 1 + width + 2;
        let mut index = Vec::with_capacity(subsections.len() * 2);
        let mut data = Vec::new();
        for (first, rows) in &subsections {
            index.push(Object::Integer(i64::from(*first)));
            index.push(Object::Integer(rows.len() as i64));
            for row in rows {
                data.push(row.kind);
                data.extend_from_slice(&row.field.to_be_bytes()[8 - width..]);
                data.extend_from_slice(&row.generation.to_be_bytes());
            }
        }
        trailer.set("Type", Object::Name(b"XRef".to_vec()));
        trailer.set("Size", i64::from(self.size));
        trailer.set(
            "W",
            vec![
                Object::Integer(1),
                Object::Integer(width as i64),
                Object::Integer(2),
            ],
        );
        trailer.set("Index", index);
        if self.full {
            data = super::deflated(&up(&data, columns));
            trailer.set("Filter", Object::Name(b"FlateDecode".to_vec()));
            trailer.set(
                "DecodeParms",
                dictionary! { "Predictor" => 12, "Columns" => columns as i64 },
            );
        }
        object::indirect(out, (num, 0), &Object::Stream(Stream::new(trailer, data)))?;
        let _ = write!(out, "startxref\n{}\n%%EOF\n", pos - self.header);
        Ok(())
    }

    /// The rows, in subsections of contiguous numbers, each with its first
    /// number. Entry 0 heads the free list in a full write, and in an
    /// append that frees anything; each free entry gives the next, the
    /// last 0.
    fn subsections(&self) -> Vec<(u32, Vec<Row>)> {
        let head = Entry::Free {
            generation: u16::MAX,
        };
        let listed: Vec<(u32, Entry)> = if self.full {
            let unused = Entry::Free { generation: 0 };
            std::iter::once((0, head))
                .chain((1..self.size).map(|n| (n, *self.entries.get(&n).unwrap_or(&unused))))
                .collect()
        } else {
            let frees = self
                .entries
                .values()
                .any(|e| matches!(e, Entry::Free { .. }));
            frees
                .then_some((0, head))
                .into_iter()
                .chain(self.entries.iter().map(|(&n, &e)| (n, e)))
                .collect()
        };
        let mut rows = Vec::with_capacity(listed.len());
        let mut next = 0;
        for &(n, entry) in listed.iter().rev() {
            rows.push(match entry {
                Entry::InUse { pos, generation } => Row {
                    kind: 1,
                    field: (pos - self.header) as u64,
                    generation,
                },
                Entry::Free { generation } => {
                    let row = Row {
                        kind: 0,
                        field: u64::from(next),
                        generation,
                    };
                    next = n;
                    row
                }
                Entry::Packed { container, index } => Row {
                    kind: 2,
                    field: u64::from(container),
                    generation: index,
                },
            });
        }
        let mut out: Vec<(u32, Vec<Row>)> = Vec::new();
        for (&(n, _), row) in listed.iter().zip(rows.into_iter().rev()) {
            match out.last_mut() {
                Some((first, run)) if *first + run.len() as u32 == n => run.push(row),
                _ => out.push((n, vec![row])),
            }
        }
        out
    }
}

/// `rows`, each `columns` bytes, under the PNG Up predictor: each row
/// tagged 2 and less the row above it, byte by byte, the first less zeros.
fn up(rows: &[u8], columns: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(rows.len() + rows.len() / columns);
    let zeros = vec![0; columns];
    let mut above = zeros.as_slice();
    for row in rows.chunks(columns) {
        out.push(2);
        out.extend(row.iter().zip(above).map(|(b, a)| b.wrapping_sub(*a)));
        above = row;
    }
    out
}
