//! zlib's `inflate`, driven the way Pillow's `ZipDecode` drives it, for the pixel stream of a PNG.
//!
//! Pillow's PNG decoder calls `inflate` once per image row (the output buffer is that row) while it
//! still has input, and a stream that ends, or runs out of input, before the last row is complete
//! is an error or a partial picture depending on exactly how many input bytes zlib has taken by
//! then. Another inflater (`miniz_oxide`, the `png` crate's `fdeflate`) reads ahead of its output
//! and cannot tell. This one takes input one byte at a time as zlib does (`PULLBYTE`), decodes
//! the next symbol before it looks at the output room (`LEN` before `LIT`/`MATCH`), and carries a
//! pending literal or match from one row to the next, so the same bytes are consumed at the same
//! moments. The errors zlib raises (`Z_DATA_ERROR`) are [`Broken`]; they all read the same to
//! Pillow ("broken data stream when reading image file").

/// `Z_DATA_ERROR`: the stream is not valid deflate/zlib data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Broken;

/// Why a call returned, other than an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The output limit is reached with a literal or match still pending.
    Full,
    /// The input of this call ran out.
    Need,
    /// `Z_STREAM_END`: the stream, its adler32 included, is complete.
    End,
    /// `Z_NEED_DICT`: the stream wants a preset dictionary, which Pillow cannot give.
    NeedDict,
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
];
/// The order the code-length code's lengths are stored in.
const CLEN_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// A canonical Huffman code: `count[len]` codes of each length, symbols in code order.
#[derive(Clone)]
struct Huff {
    count: [u16; 16],
    symbol: Vec<u16>,
    max: usize,
}

/// Which table `inflate_table` is building: only the code-length code must be complete.
#[derive(PartialEq, Eq, Clone, Copy)]
enum Kind {
    Codes,
    Other,
}

impl Huff {
    /// `inflate_table`'s acceptance: no over-subscribed set, no incomplete set (except one code of
    /// length 1 in a literal/length or distance table), and a table with no symbols at all is fine.
    fn build(lens: &[u8], kind: Kind) -> Result<Huff, Broken> {
        let mut count = [0u16; 16];
        for &l in lens {
            count[usize::from(l)] += 1;
        }
        let max = (1..16).rev().find(|&l| count[l] != 0).unwrap_or(0);
        count[0] = 0;
        if max == 0 {
            return Ok(Huff {
                count,
                symbol: Vec::new(),
                max: 0,
            });
        }
        let mut left = 1i32;
        for &c in &count[1..] {
            left = (left << 1) - i32::from(c);
            if left < 0 {
                return Err(Broken);
            }
        }
        if left > 0 && (kind == Kind::Codes || max != 1) {
            return Err(Broken);
        }
        let mut offs = [0u16; 16];
        for l in 1..15 {
            offs[l + 1] = offs[l] + count[l];
        }
        let mut symbol = vec![0u16; lens.len()];
        for (sym, &l) in lens.iter().enumerate() {
            if l != 0 {
                symbol[usize::from(offs[usize::from(l)])] = sym as u16;
                offs[usize::from(l)] += 1;
            }
        }
        Ok(Huff { count, symbol, max })
    }

    fn fixed_lit() -> Huff {
        let mut lens = [8u8; 288];
        lens[144..256].fill(9);
        lens[256..280].fill(7);
        Huff::build(&lens, Kind::Other).expect("the fixed literal/length code is complete")
    }

    fn fixed_dist() -> Huff {
        Huff::build(&[5u8; 32], Kind::Other).expect("the fixed distance code is complete")
    }
}

#[derive(Clone, Copy)]
enum St {
    Head,
    /// After the zlib header or a finished block.
    Block,
    Stored,
    Dynamic,
    Copy(usize),
    Len,
    Lit(u8),
    LenExt(usize),
    Dist(usize),
    DistExt(usize, usize),
    Match(usize, usize),
    Check,
    Done,
}

pub(crate) struct Inflate<'a> {
    data: &'a [u8],
    pos: usize,
    limit: usize,
    hold: u64,
    bits: u32,
    st: St,
    last: bool,
    lit: Huff,
    dist: Huff,
    clen: Huff,
    pub(crate) out: Vec<u8>,
}

/// A symbol resolved by [`Inflate::peek`].
enum Peek {
    Sym(u16, u32),
    NeedInput,
}

impl<'a> Inflate<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Inflate {
            data,
            pos: 0,
            limit: 0,
            hold: 0,
            bits: 0,
            st: St::Head,
            last: false,
            lit: Huff::fixed_lit(),
            dist: Huff::fixed_dist(),
            clen: Huff::fixed_dist(),
            out: Vec::new(),
        }
    }

    /// Input bytes are available up to `end` (the end of the block Pillow just read).
    pub(crate) fn set_limit(&mut self, end: usize) {
        self.limit = end.min(self.data.len());
    }

    /// Whether any input is left in the current block (`avail_in > 0`).
    pub(crate) fn has_input(&self) -> bool {
        self.pos < self.limit
    }

    fn pull(&mut self) -> bool {
        if self.pos >= self.limit {
            return false;
        }
        self.hold |= u64::from(self.data[self.pos]) << self.bits;
        self.pos += 1;
        self.bits += 8;
        true
    }

    /// `NEEDBITS(n)`.
    fn need(&mut self, n: u32) -> bool {
        while self.bits < n {
            if !self.pull() {
                return false;
            }
        }
        true
    }

    fn take(&mut self, n: u32) -> u32 {
        let v = (self.hold & ((1u64 << n) - 1)) as u32;
        self.hold >>= n;
        self.bits -= n;
        v
    }

    /// `BYTEBITS`: drop the bits left in the current byte.
    fn byte_align(&mut self) {
        let drop = self.bits & 7;
        self.hold >>= drop;
        self.bits -= drop;
    }

    /// Decode the next symbol of `huff` without taking it, pulling only as many bytes as the code
    /// needs. `Err` is a bit pattern no code has.
    fn peek(&mut self, which: Which) -> Result<Peek, Broken> {
        loop {
            let (huff, kind) = match which {
                Which::Lit => (&self.lit, Kind::Other),
                Which::Dist => (&self.dist, Kind::Other),
                Which::Clen => (&self.clen, Kind::Codes),
            };
            if huff.max == 0 {
                // `inflate_table` marks a table with no symbols with one-bit entries; only the
                // code-length code is ever read through one without erroring, and it reads
                // symbol 0.
                if kind != Kind::Codes {
                    return Err(Broken);
                }
                if self.bits >= 1 {
                    return Ok(Peek::Sym(0, 1));
                }
            } else {
                let max = huff.max as u32;
                let (mut code, mut first, mut index) = (0u32, 0u32, 0u32);
                for len in 1..=self.bits.min(max) {
                    code |= ((self.hold >> (len - 1)) & 1) as u32;
                    let count = u32::from(huff.count[len as usize]);
                    if code < first + count {
                        let sym = huff.symbol[(index + code - first) as usize];
                        return Ok(Peek::Sym(sym, len));
                    }
                    index += count;
                    first = (first + count) << 1;
                    code <<= 1;
                }
                if self.bits >= max {
                    return Err(Broken);
                }
            }
            if !self.pull() {
                return Ok(Peek::NeedInput);
            }
        }
    }

    /// The tables of a dynamic block (`TABLE`, `LENLENS`, `CODELENS`), `None` if the input ends
    /// first.
    fn dynamic_tables(&mut self) -> Result<Option<(Huff, Huff)>, Broken> {
        if !self.need(14) {
            return Ok(None);
        }
        let nlen = self.take(5) as usize + 257;
        let ndist = self.take(5) as usize + 1;
        let ncode = self.take(4) as usize + 4;
        if nlen > 286 || ndist > 30 {
            return Err(Broken);
        }
        let mut clens = [0u8; 19];
        for &slot in &CLEN_ORDER[..ncode] {
            if !self.need(3) {
                return Ok(None);
            }
            clens[slot] = self.take(3) as u8;
        }
        self.clen = Huff::build(&clens, Kind::Codes)?;
        let total = nlen + ndist;
        let mut lens = [0u8; 320];
        let mut have = 0usize;
        while have < total {
            let (sym, n) = match self.peek(Which::Clen)? {
                Peek::NeedInput => return Ok(None),
                Peek::Sym(sym, n) => (sym, n),
            };
            if sym < 16 {
                self.take(n);
                lens[have] = sym as u8;
                have += 1;
                continue;
            }
            let (extra, base) = match sym {
                16 => (2, 3),
                17 => (3, 3),
                _ => (7, 11),
            };
            if !self.need(n + extra) {
                return Ok(None);
            }
            self.take(n);
            let copy = base + self.take(extra) as usize;
            let fill = if sym == 16 {
                if have == 0 {
                    return Err(Broken);
                }
                lens[have - 1]
            } else {
                0
            };
            if have + copy > total {
                return Err(Broken);
            }
            lens[have..have + copy].fill(fill);
            have += copy;
        }
        if lens[256] == 0 {
            return Err(Broken);
        }
        let lit = Huff::build(&lens[..nlen], Kind::Other)?;
        let dist = Huff::build(&lens[nlen..total], Kind::Other)?;
        Ok(Some((lit, dist)))
    }

    /// Run until the output reaches `out_limit` bytes, the input runs out, the stream ends or it is
    /// found broken.
    pub(crate) fn run(&mut self, out_limit: usize) -> Result<Stop, Broken> {
        loop {
            match self.st {
                St::Head => {
                    if !self.need(16) {
                        return Ok(Stop::Need);
                    }
                    let cmf = (self.hold & 0xff) as u32;
                    let flg = ((self.hold >> 8) & 0xff) as u32;
                    if (cmf << 8 | flg) % 31 != 0 || cmf & 15 != 8 || (cmf >> 4) + 8 > 15 {
                        return Err(Broken);
                    }
                    self.take(16);
                    if flg & 0x20 != 0 {
                        if !self.need(32) {
                            return Ok(Stop::Need);
                        }
                        return Ok(Stop::NeedDict);
                    }
                    self.st = St::Block;
                }
                St::Block => {
                    if self.last {
                        self.byte_align();
                        self.st = St::Check;
                        continue;
                    }
                    if !self.need(3) {
                        return Ok(Stop::Need);
                    }
                    self.last = self.take(1) == 1;
                    match self.take(2) {
                        0 => self.st = St::Stored,
                        1 => {
                            self.lit = Huff::fixed_lit();
                            self.dist = Huff::fixed_dist();
                            self.st = St::Len;
                        }
                        2 => self.st = St::Dynamic,
                        _ => return Err(Broken),
                    }
                }
                St::Dynamic => {
                    // The header is read whole or not at all: a block boundary inside it leaves
                    // its bytes for the next call.
                    let snap = (self.pos, self.hold, self.bits);
                    match self.dynamic_tables()? {
                        Some((lit, dist)) => {
                            self.lit = lit;
                            self.dist = dist;
                            self.st = St::Len;
                        }
                        None => {
                            (self.pos, self.hold, self.bits) = snap;
                            return Ok(Stop::Need);
                        }
                    }
                }
                St::Stored => {
                    self.byte_align();
                    if !self.need(32) {
                        return Ok(Stop::Need);
                    }
                    let v = self.take(32);
                    if (v & 0xffff) != ((v >> 16) ^ 0xffff) {
                        return Err(Broken);
                    }
                    self.st = if v & 0xffff == 0 {
                        St::Block
                    } else {
                        St::Copy((v & 0xffff) as usize)
                    };
                }
                St::Copy(n) => {
                    let room = out_limit.saturating_sub(self.out.len());
                    let have = self.limit - self.pos;
                    let copy = n.min(room).min(have);
                    if copy == 0 {
                        return Ok(if room == 0 { Stop::Full } else { Stop::Need });
                    }
                    self.out.extend_from_slice(&self.data[self.pos..self.pos + copy]);
                    self.pos += copy;
                    self.st = if copy == n { St::Block } else { St::Copy(n - copy) };
                }
                St::Len => match self.peek(Which::Lit)? {
                    Peek::NeedInput => return Ok(Stop::Need),
                    Peek::Sym(sym, len) => {
                        self.take(len);
                        self.st = match sym {
                            0..=255 => St::Lit(sym as u8),
                            256 => St::Block,
                            257..=285 => St::LenExt(usize::from(sym) - 257),
                            _ => return Err(Broken),
                        };
                    }
                },
                St::Lit(b) => {
                    if self.out.len() >= out_limit {
                        return Ok(Stop::Full);
                    }
                    self.out.push(b);
                    self.st = St::Len;
                }
                St::LenExt(i) => {
                    let extra = u32::from(LEN_EXTRA[i]);
                    if !self.need(extra) {
                        return Ok(Stop::Need);
                    }
                    let len = usize::from(LEN_BASE[i]) + self.take(extra) as usize;
                    self.st = St::Dist(len);
                }
                St::Dist(len) => match self.peek(Which::Dist)? {
                    Peek::NeedInput => return Ok(Stop::Need),
                    Peek::Sym(sym, n) => {
                        if sym > 29 {
                            return Err(Broken);
                        }
                        self.take(n);
                        self.st = St::DistExt(len, usize::from(sym));
                    }
                },
                St::DistExt(len, i) => {
                    let extra = u32::from(DIST_EXTRA[i]);
                    if !self.need(extra) {
                        return Ok(Stop::Need);
                    }
                    let dist = usize::from(DIST_BASE[i]) + self.take(extra) as usize;
                    self.st = St::Match(len, dist);
                }
                St::Match(len, dist) => {
                    if self.out.len() >= out_limit {
                        return Ok(Stop::Full);
                    }
                    if dist > self.out.len() {
                        return Err(Broken);
                    }
                    let n = len.min(out_limit - self.out.len());
                    let from = self.out.len() - dist;
                    for k in 0..n {
                        let b = self.out[from + k];
                        self.out.push(b);
                    }
                    self.st = if n == len { St::Len } else { St::Match(len - n, dist) };
                }
                St::Check => {
                    if !self.need(32) {
                        return Ok(Stop::Need);
                    }
                    let want = self.take(32).swap_bytes();
                    if want != adler32(&self.out) {
                        return Err(Broken);
                    }
                    self.st = St::Done;
                }
                St::Done => return Ok(Stop::End),
            }
        }
    }
}

enum Which {
    Lit,
    Dist,
    Clen,
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}
