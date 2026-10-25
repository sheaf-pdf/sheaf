//! A Type 1 program read ahead of read-fonts (M11 slice 3): its tokens as
//! read-fonts' `Type1Font` reads them (`RawDicts`, its `Parser`), counted,
//! and its charstrings by glyph name, which read-fonts keeps to itself.
//!
//! read-fonts 0.41 reads a program by recursion, and its work grows faster
//! than the program does. The scan here reads the program as it would,
//! bounding the depth of what it reads ([`PROC_DEPTH`], [`INTS_AHEAD`]) and
//! counting its work ([`WORK`]), so only a program within all three is
//! handed to it ([`program`](super::program)). The charstrings found let
//! sheaf evaluate them through read-fonts' interpreter within fuel of its
//! own.

use std::collections::HashMap;

/// How deep a Type 1 program's procedures may nest. Adobe's own
/// `/OtherSubrs` nest a few levels.
pub(super) const PROC_DEPTH: usize = 16;

/// How many integers in a row a Type 1 program may have: read-fonts reads
/// ahead of each to the token after it, which, for another integer, reads
/// ahead in turn. Real programs have a dozen or so (`/BlueValues`).
pub(super) const INTS_AHEAD: usize = 256;

/// What reading a Type 1 program may cost, in bytes read as tokens, at
/// most: this many times its size.
const WORK: usize = 64;

/// A Type 1 program's charstrings by glyph name, each decrypted and past
/// its `lenIV` bytes, as read-fonts reads them (`read_charstrings`): None
/// for a name the program gives twice, whose glyph sheaf can't tell from
/// another's by name. A program without `.notdef` has the one read-fonts
/// makes up.
pub(super) type Charstrings = HashMap<Vec<u8>, Option<Vec<u8>>>;

/// `data`, a Type 1 program, scanned as read-fonts reads it: its
/// charstrings, where its procedures nest within [`PROC_DEPTH`] and reading
/// it costs within [`WORK`], and its integers run within [`INTS_AHEAD`];
/// None where not, or where read-fonts wouldn't read it either: no header,
/// no `eexec`, no `/CharStrings`.
pub(super) fn scan(data: &[u8]) -> Option<Charstrings> {
    let fuel = WORK.saturating_mul(data.len().max(1 << 14));
    let mut parser = Parser::new(data, fuel);
    let (base, private) = dicts(data, &mut parser)?;
    let mut parser = Parser::new(base, parser.fuel);
    while parser.next(0).ok()?.is_some() {}
    let mut parser = Parser::new(&private, parser.fuel);
    let mut len_iv = 4;
    let mut found = None;
    loop {
        match parser.next(0).ok()? {
            None => break,
            Some(Token::Name(b"lenIV")) => match parser.next(0).ok()? {
                Some(Token::Int(n)) => len_iv = n,
                _ => return None,
            },
            Some(Token::Name(b"CharStrings")) if found.is_none() => {
                found = Some(parser.charstrings(len_iv)?);
            }
            Some(_) => {}
        }
    }
    found
}

/// The program's base dictionary and its private one decrypted, as
/// read-fonts finds them (`RawDicts::new`): a PFB's segments, or the clear
/// text and what follows `eexec`, hex or binary.
fn dicts<'a>(data: &'a [u8], parser: &mut Parser<'a>) -> Option<(&'a [u8], Vec<u8>)> {
    let header = |d: &[u8]| d.starts_with(b"%!PS-AdobeFont") || d.starts_with(b"%!FontType");
    if let Some((TEXT, size)) = pfb_tag(data, 0) {
        let data = data.get(6..)?;
        header(data).then_some(())?;
        let (base, rest) = data.split_at_checked(size as usize)?;
        let mut binary = Vec::new();
        let mut at = 0;
        // The binary segments that follow, each whole.
        while let Some((BINARY, len)) = pfb_tag(rest, at) {
            let Some(segment) = rest.get(at + 6..at + 6 + len as usize) else {
                break;
            };
            binary.extend_from_slice(segment);
            at += 6 + segment.len();
        }
        return Some((base, decrypt(&binary, EEXEC_SEED, 4)));
    }
    header(data).then_some(())?;
    let start = parser.eexec()?;
    let (base, rest) = data.split_at_checked(start)?;
    let private = match rest.len() > 3 && rest[..4].iter().all(u8::is_ascii_hexdigit) {
        true => decrypt(&hex(rest), EEXEC_SEED, 4),
        false => decrypt(rest, EEXEC_SEED, 4),
    };
    Some((base, private))
}

const TEXT: u16 = 0x8001;
const BINARY: u16 = 0x8002;

/// A PFB segment's tag and size, at `at`.
fn pfb_tag(data: &[u8], at: usize) -> Option<(u16, u32)> {
    let header = data.get(at..at.checked_add(6)?)?;
    let tag = u16::from_be_bytes([header[0], header[1]]);
    let size = u32::from_le_bytes([header[2], header[3], header[4], header[5]]);
    matches!(tag, TEXT | BINARY).then_some((tag, size))
}

const EEXEC_SEED: u32 = 55665;
const CHARSTRING_SEED: u32 = 4330;

/// `data` decrypted from `seed`, its first `skip` bytes dropped.
fn decrypt(data: &[u8], mut seed: u32, skip: usize) -> Vec<u8> {
    let plain = data.iter().map(|&b| {
        let b = u32::from(b);
        let plain = b ^ (seed >> 8);
        seed = b.wrapping_add(seed).wrapping_mul(52845).wrapping_add(22719) & 0xFFFF;
        plain as u8
    });
    plain.skip(skip).collect()
}

/// Hex digits as bytes, whitespace passed over, ending at anything else;
/// a digit left over is a byte's high half.
fn hex(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut high = None;
    for &c in data {
        if is_whitespace(c) {
            continue;
        }
        let Some(digit) = (c as char).to_digit(16) else {
            break;
        };
        match high.take() {
            None => high = Some(digit as u8),
            Some(h) => out.push(h << 4 | digit as u8),
        }
    }
    out.extend(high.map(|h| h << 4));
    out
}

/// A token, as read-fonts' Type 1 parser reads one.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Token<'a> {
    Int(i64),
    Binary(&'a [u8]),
    Name(&'a [u8]),
    Raw(&'a [u8]),
    /// A procedure, a literal string or a hex string.
    Other,
}

/// Past [`PROC_DEPTH`] or [`INTS_AHEAD`], or out of fuel.
struct Refused;

#[derive(Clone)]
struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
    /// Bytes it may yet read as tokens.
    fuel: usize,
    /// How many integers it's reading ahead of.
    ahead: usize,
}

impl<'a> Parser<'a> {
    fn new(data: &'a [u8], fuel: usize) -> Parser<'a> {
        Parser {
            data,
            pos: 0,
            fuel,
            ahead: 0,
        }
    }

    /// The next token, read as read-fonts reads it, `depth` procedures in:
    /// None at the end, or where read-fonts would stop.
    fn next(&mut self, depth: usize) -> Result<Option<Token<'a>>, Refused> {
        loop {
            if !self.skip_whitespace()? {
                return Ok(None);
            }
            let start = self.pos;
            let Some(c) = self.next_byte()? else {
                return Ok(None);
            };
            match c {
                b'%' => self.skip_line()?,
                b'{' => return self.proc(depth + 1),
                b'(' => return self.string(),
                b'<' if self.peek_byte() == Some(b'<') => self.pos += 1,
                b'<' => return self.hex_string(),
                b'>' => match self.next_byte()? {
                    Some(b'>') => {}
                    _ => return Ok(None),
                },
                b'/' => match self.peek_byte() {
                    Some(c) if is_whitespace(c) || is_special(c) => {
                        self.pos += usize::from(!is_special(c));
                        return Ok(Some(Token::Name(&[])));
                    }
                    Some(_) => {
                        let count = self.skip_until(|c| is_whitespace(c) || is_special(c))?;
                        return Ok(self.data.get(start + 1..start + count).map(Token::Name));
                    }
                    None => {}
                },
                b'[' | b']' => return Ok(Some(Token::Raw(&self.data[start..start + 1]))),
                _ => {
                    let count = self.skip_until(|c| is_whitespace(c) || is_special(c))?;
                    let Some(content) = self.data.get(start..start + count) else {
                        return Ok(None);
                    };
                    let numeric = c.is_ascii_digit() || c == b'-';
                    let int = (numeric && !content.contains(&b'.'))
                        .then(|| int(content))
                        .flatten();
                    let Some(int) = int else {
                        return Ok(Some(Token::Raw(content)));
                    };
                    // An integer followed by `RD` or `-|` is the length of
                    // the binary blob after them and a space.
                    let mut ahead = self.clone();
                    ahead.ahead += 1;
                    if ahead.ahead > INTS_AHEAD {
                        return Err(Refused);
                    }
                    let following = ahead.next(depth)?;
                    self.fuel = ahead.fuel;
                    if matches!(following, Some(Token::Raw(b"RD" | b"-|"))) {
                        self.next(depth)?;
                        self.pos += 1;
                        let end = self.pos.checked_add(int as usize);
                        let Some(blob) = end.and_then(|end| self.data.get(self.pos..end)) else {
                            return Ok(None);
                        };
                        self.burn(blob.len())?;
                        self.pos += blob.len();
                        return Ok(Some(Token::Binary(blob)));
                    }
                    return Ok(Some(Token::Int(int)));
                }
            }
        }
    }

    /// A procedure's tokens, read as read-fonts reads them (`read_proc`):
    /// a byte, and a token where that byte doesn't end it.
    fn proc(&mut self, depth: usize) -> Result<Option<Token<'a>>, Refused> {
        if depth > PROC_DEPTH {
            return Err(Refused);
        }
        loop {
            match self.next_byte()? {
                None => return Ok(None),
                Some(b'}') => return Ok(Some(Token::Other)),
                Some(_) => {}
            }
            if self.next(depth)?.is_none() {
                return Ok(None);
            }
            self.skip_whitespace()?;
        }
    }

    /// A literal string's bytes, its parentheses balanced, a backslash
    /// taking the byte after it.
    fn string(&mut self) -> Result<Option<Token<'a>>, Refused> {
        let mut depth = 1;
        while let Some(c) = self.next_byte()? {
            match c {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos += 1;
                        return Ok(Some(Token::Other));
                    }
                }
                b'\\' if self.next_byte()?.is_none() => return Ok(None),
                _ => {}
            }
        }
        Ok(None)
    }

    /// A hex string's digits and whitespace, to its `>`.
    fn hex_string(&mut self) -> Result<Option<Token<'a>>, Refused> {
        while let Some(c) = self.next_byte()? {
            if !is_whitespace(c) && !c.is_ascii_hexdigit() {
                return Ok((c == b'>').then_some(Token::Other));
            }
        }
        Ok(None)
    }

    /// Where the data after `eexec` starts, as read-fonts finds it
    /// (`find_eexec_data`): past the whitespace after the token, to a
    /// carriage return that no line feed follows before it.
    fn eexec(&mut self) -> Option<usize> {
        while let Some(token) = self.next(0).ok()? {
            if token != Token::Raw(b"eexec") {
                continue;
            }
            let data = self.data;
            let mut start = self.pos;
            let mut linefeed = None;
            while start < data.len() {
                match data[start] {
                    b' ' | b'\t' => {}
                    b'\n' => linefeed = Some(start),
                    b'\r' => {
                        let next = || {
                            data[start..]
                                .iter()
                                .position(|&b| b == b'\n')
                                .map_or(0, |p| p + start)
                        };
                        if *linefeed.get_or_insert_with(next) < start {
                            break;
                        }
                    }
                    _ => break,
                }
                start += 1;
            }
            return (start < data.len()).then_some(start);
        }
        None
    }

    /// `/CharStrings`' entries, as read-fonts reads them
    /// (`read_charstrings`), decrypted with `len_iv`.
    fn charstrings(&mut self, len_iv: i64) -> Option<Charstrings> {
        let Some(Token::Int(count)) = self.next(0).ok()? else {
            return None;
        };
        usize::try_from(count).ok()?;
        let mut found = Charstrings::new();
        let ends = |p: &Parser<'_>| {
            p.peek_byte()
                .is_some_and(|c| is_whitespace(c) || is_special(c))
        };
        while let Some(token) = self.next(0).ok()? {
            match token {
                Token::Raw(b"end") if ends(self) => break,
                Token::Raw(b"def") if ends(self) && !found.is_empty() => break,
                Token::Name(name) => {
                    let Some(Token::Binary(data)) = self.next(0).ok()? else {
                        return None;
                    };
                    let plain = match usize::try_from(len_iv) {
                        Ok(skip) => decrypt(data, CHARSTRING_SEED, skip),
                        Err(_) => data.to_vec(),
                    };
                    found
                        .entry(name.to_vec())
                        .and_modify(|twice| *twice = None)
                        .or_insert(Some(plain));
                }
                _ => {}
            }
        }
        // read-fonts makes up a `.notdef` where there's none: `0 333 hsbw
        // endchar`.
        found
            .entry(b".notdef".to_vec())
            .or_insert(Some(vec![0x8B, 0xF7, 0xE1, 0x0D, 0x0E]));
        Some(found)
    }

    fn burn(&mut self, n: usize) -> Result<(), Refused> {
        self.fuel = self.fuel.checked_sub(n).ok_or(Refused)?;
        Ok(())
    }

    fn next_byte(&mut self) -> Result<Option<u8>, Refused> {
        let Some(&byte) = self.data.get(self.pos) else {
            return Ok(None);
        };
        self.burn(1)?;
        self.pos += 1;
        Ok(Some(byte))
    }

    fn peek_byte(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    /// Whitespace passed over: false at the end.
    fn skip_whitespace(&mut self) -> Result<bool, Refused> {
        while let Some(&c) = self.data.get(self.pos) {
            if !is_whitespace(c) {
                return Ok(true);
            }
            self.burn(1)?;
            self.pos += 1;
        }
        Ok(false)
    }

    fn skip_line(&mut self) -> Result<(), Refused> {
        while let Some(c) = self.next_byte()? {
            if c == b'\n' || c == b'\r' {
                break;
            }
        }
        Ok(())
    }

    /// Bytes passed over until `stop` holds, one more than there are.
    fn skip_until(&mut self, stop: impl Fn(u8) -> bool) -> Result<usize, Refused> {
        let mut count = 0;
        while let Some(byte) = self.peek_byte() {
            if stop(byte) {
                break;
            }
            self.burn(1)?;
            self.pos += 1;
            count += 1;
        }
        Ok(count + 1)
    }
}

/// An integer as read-fonts reads one (`decode_int`): decimal, or a radix
/// of 2 to 36 before `#`.
fn int(bytes: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(bytes).ok()?;
    match s.find('#') {
        Some(at @ (1 | 2)) => {
            let radix = s[..at]
                .parse::<u32>()
                .ok()
                .filter(|r| (2..=36).contains(r))?;
            i64::from_str_radix(&s[at + 1..], radix).ok()
        }
        _ => s.parse().ok(),
    }
}

fn is_whitespace(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\r' | b'\t' | b'\0' | 0x0C)
}

fn is_special(c: u8) -> bool {
    matches!(
        c,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}
