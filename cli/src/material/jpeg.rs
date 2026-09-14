//! A baseline and progressive JPEG decoder, matching stb_image.
//!
//! The wallpaper's colour comes from quantizing its thumbnail, and the
//! thumbnail is a JPEG. The library this replaces decodes it with stb_image,
//! so this does too — not any JPEG decoder, but that one: the IDCT rounding,
//! the chroma upsampling filter and the YCbCr fixed-point constants all show
//! up in the final pixels, and a pixel out by one can pick a different
//! dominant colour and shift the whole palette.
//!
//! Ported from `stb_image.h` v2.29, the copy vendored in materialyoucolor.
//! stb ships SSE2 kernels for the IDCT, the 2x2 upsampler and the colour
//! conversion; its own comment says they are bit-identical to the scalar ones,
//! and measured against the C on this machine they are, so only the scalar
//! ones are here.

const FAST_BITS: usize = 9;
const BMASK: [u32; 17] = [
    0, 1, 3, 7, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767, 65535,
];
const JBIAS: [i32; 16] = [
    0, -1, -3, -7, -15, -31, -63, -127, -255, -511, -1023, -2047, -4095, -8191, -16383, -32767,
];

/// Where a coefficient at position N of the zigzag stream lands in a
/// row-major 8x8 block. The tail of 63s is stb's: it lets corrupt input run
/// past the end without leaving the array.
const DEZIGZAG: [usize; 64 + 15] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27,
    20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63, 63, 63, 63, 63, 63, 63, 63,
    63, 63, 63, 63, 63, 63, 63, 63,
];

const MARKER_NONE: u8 = 0xff;

pub struct Image {
    pub width: usize,
    pub height: usize,
    /// Three bytes per pixel, row by row.
    pub rgb: Vec<u8>,
}

#[derive(Clone)]
struct Huffman {
    fast: [u8; 1 << FAST_BITS],
    code: [u16; 256],
    values: [u8; 256],
    size: [u8; 257],
    maxcode: [u32; 18],
    delta: [i32; 17],
}

impl Default for Huffman {
    fn default() -> Huffman {
        Huffman {
            fast: [255; 1 << FAST_BITS],
            code: [0; 256],
            values: [0; 256],
            size: [0; 257],
            maxcode: [0; 18],
            delta: [0; 17],
        }
    }
}

impl Huffman {
    fn build(&mut self, counts: &[i32; 16]) -> Result<(), String> {
        let mut k = 0usize;
        for (i, count) in counts.iter().enumerate() {
            for _ in 0..*count {
                if k >= 257 {
                    return Err("bad size list".into());
                }
                self.size[k] = (i + 1) as u8;
                k += 1;
            }
        }
        self.size[k] = 0;

        let mut code: u32 = 0;
        let mut k = 0usize;
        for j in 1..=16usize {
            self.delta[j] = k as i32 - code as i32;
            if self.size[k] == j as u8 {
                while self.size[k] == j as u8 {
                    self.code[k] = code as u16;
                    code += 1;
                    k += 1;
                }
                if code - 1 >= 1 << j {
                    return Err("bad code lengths".into());
                }
            }
            self.maxcode[j] = code << (16 - j);
            code <<= 1;
        }
        self.maxcode[17] = 0xffff_ffff;

        self.fast = [255; 1 << FAST_BITS];
        for i in 0..k {
            let s = self.size[i] as usize;
            if s <= FAST_BITS {
                let c = (self.code[i] as usize) << (FAST_BITS - s);
                for j in 0..(1 << (FAST_BITS - s)) {
                    self.fast[c + j] = i as u8;
                }
            }
        }
        Ok(())
    }

    /// Decodes magnitude and value together for short AC codes, so the common
    /// case never touches the slow path.
    fn build_fast_ac(&self, fast_ac: &mut [i16; 1 << FAST_BITS]) {
        for i in 0..(1 << FAST_BITS) {
            let fast = self.fast[i];
            fast_ac[i] = 0;
            if fast < 255 {
                let rs = self.values[fast as usize] as i32;
                let run = (rs >> 4) & 15;
                let magbits = rs & 15;
                let len = self.size[fast as usize] as i32;

                if magbits != 0 && len + magbits <= FAST_BITS as i32 {
                    let mut k = ((i << len) & ((1 << FAST_BITS) - 1)) >> (FAST_BITS - magbits as usize);
                    let m = 1usize << (magbits - 1);
                    let k = if k < m {
                        k = k.wrapping_add((!0usize << magbits).wrapping_add(1));
                        k as isize
                    } else {
                        k as isize
                    };
                    if (-128..=127).contains(&k) {
                        fast_ac[i] = ((k as i32 * 256) + (run * 16) + (len + magbits)) as i16;
                    }
                }
            }
        }
    }
}

#[derive(Clone, Default)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    hd: usize,
    ha: usize,
    dc_pred: i32,
    /// Pixels this component actually covers, before padding out to whole MCUs.
    x: usize,
    y: usize,
    /// The padded plane the IDCT writes into.
    w2: usize,
    h2: usize,
    data: Vec<u8>,
    coeff: Vec<i16>,
    coeff_w: usize,
    coeff_h: usize,
}

struct Decoder<'a> {
    bytes: &'a [u8],
    pos: usize,

    img_x: usize,
    img_y: usize,
    img_n: usize,

    huff_dc: [Huffman; 4],
    huff_ac: [Huffman; 4],
    fast_ac: [[i16; 1 << FAST_BITS]; 4],
    dequant: [[u16; 64]; 4],

    comp: Vec<Component>,
    h_max: usize,
    v_max: usize,
    mcu_w: usize,
    mcu_h: usize,
    mcu_x: usize,
    mcu_y: usize,

    code_buffer: u32,
    code_bits: i32,
    marker: u8,
    nomore: bool,

    progressive: bool,
    scan_n: usize,
    order: [usize; 4],
    spec_start: i32,
    spec_end: i32,
    succ_high: i32,
    succ_low: i32,
    eob_run: i32,
    restart_interval: i32,
    todo: i32,

    jfif: bool,
    app14_colour_transform: i32,
    rgb_components: usize,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8]) -> Decoder<'a> {
        Decoder {
            bytes,
            pos: 0,
            img_x: 0,
            img_y: 0,
            img_n: 0,
            huff_dc: Default::default(),
            huff_ac: Default::default(),
            fast_ac: [[0; 1 << FAST_BITS]; 4],
            dequant: [[0; 64]; 4],
            comp: Vec::new(),
            h_max: 1,
            v_max: 1,
            mcu_w: 8,
            mcu_h: 8,
            mcu_x: 0,
            mcu_y: 0,
            code_buffer: 0,
            code_bits: 0,
            marker: MARKER_NONE,
            nomore: false,
            progressive: false,
            scan_n: 0,
            order: [0; 4],
            spec_start: 0,
            spec_end: 63,
            succ_high: 0,
            succ_low: 0,
            eob_run: 0,
            restart_interval: 0,
            todo: 0,
            jfif: false,
            app14_colour_transform: -1,
            rgb_components: 0,
        }
    }

    fn at_eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Reads past the end as zero, the way stb's buffer does.
    fn get8(&mut self) -> u8 {
        let byte = self.bytes.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        byte
    }

    fn get16be(&mut self) -> usize {
        let hi = self.get8() as usize;
        (hi << 8) | self.get8() as usize
    }

    fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    // ---- the entropy-coded bitstream ------------------------------------

    fn grow_buffer(&mut self) {
        loop {
            let b = if self.nomore { 0u32 } else { self.get8() as u32 };
            if b == 0xff {
                let mut c = self.get8();
                while c == 0xff {
                    c = self.get8();
                }
                if c != 0 {
                    self.marker = c;
                    self.nomore = true;
                    return;
                }
                // A stuffed zero: the 0xff is a literal data byte, so it goes
                // into the bit buffer unchanged.
            }
            self.code_buffer |= b << (24 - self.code_bits);
            self.code_bits += 8;
            if self.code_bits > 24 {
                return;
            }
        }
    }

    fn huff_decode(&mut self, dc: bool, which: usize) -> i32 {
        if self.code_bits < 16 {
            self.grow_buffer();
        }
        let h = if dc { &self.huff_dc[which] } else { &self.huff_ac[which] };

        let c = ((self.code_buffer >> (32 - FAST_BITS)) & ((1 << FAST_BITS) - 1)) as usize;
        let k = h.fast[c];
        if k < 255 {
            let s = h.size[k as usize] as i32;
            if s > self.code_bits {
                return -1;
            }
            let value = h.values[k as usize] as i32;
            self.code_buffer <<= s;
            self.code_bits -= s;
            return value;
        }

        // Longer than the fast table covers: find the code length by comparing
        // against the pre-shifted maxcode table.
        let temp = self.code_buffer >> 16;
        let mut k = FAST_BITS + 1;
        while k < 18 && temp >= h.maxcode[k] {
            k += 1;
        }
        if k == 17 || k == 18 {
            self.code_bits -= 16;
            return -1;
        }
        if k as i32 > self.code_bits {
            return -1;
        }
        let c = (((self.code_buffer >> (32 - k)) & BMASK[k]) as i32) + h.delta[k];
        if !(0..256).contains(&c) {
            return -1;
        }
        let value = h.values[c as usize] as i32;
        self.code_bits -= k as i32;
        self.code_buffer <<= k;
        value
    }

    /// JPEG's combined receive-and-extend: read n bits, then sign-extend.
    fn extend_receive(&mut self, n: i32) -> i32 {
        if self.code_bits < n {
            self.grow_buffer();
        }
        if self.code_bits < n {
            return 0;
        }
        let sgn = self.code_buffer >> 31;
        let k = self.code_buffer.rotate_left(n as u32);
        self.code_buffer = k & !BMASK[n as usize];
        let k = k & BMASK[n as usize];
        self.code_bits -= n;
        k.wrapping_add((JBIAS[n as usize] as u32) & sgn.wrapping_sub(1)) as i32
    }

    fn get_bits(&mut self, n: i32) -> i32 {
        if self.code_bits < n {
            self.grow_buffer();
        }
        if self.code_bits < n {
            return 0;
        }
        let k = self.code_buffer.rotate_left(n as u32);
        self.code_buffer = k & !BMASK[n as usize];
        self.code_bits -= n;
        (k & BMASK[n as usize]) as i32
    }

    fn get_bit(&mut self) -> bool {
        if self.code_bits < 1 {
            self.grow_buffer();
        }
        if self.code_bits < 1 {
            return false;
        }
        let k = self.code_buffer;
        self.code_buffer <<= 1;
        self.code_bits -= 1;
        k & 0x8000_0000 != 0
    }

    fn reset(&mut self) {
        self.code_bits = 0;
        self.code_buffer = 0;
        self.nomore = false;
        for c in &mut self.comp {
            c.dc_pred = 0;
        }
        self.marker = MARKER_NONE;
        self.todo = if self.restart_interval > 0 { self.restart_interval } else { 0x7fff_ffff };
        self.eob_run = 0;
    }

    // ---- block decoding --------------------------------------------------

    fn decode_block(&mut self, data: &mut [i16; 64], b: usize) -> Result<(), String> {
        if self.code_bits < 16 {
            self.grow_buffer();
        }
        let (hd, ha, tq) = (self.comp[b].hd, self.comp[b].ha, self.comp[b].tq);
        let t = self.huff_decode(true, hd);
        if !(0..=15).contains(&t) {
            return Err("bad huffman code".into());
        }
        *data = [0; 64];

        let diff = if t != 0 { self.extend_receive(t) } else { 0 };
        let dc = self.comp[b].dc_pred.wrapping_add(diff);
        self.comp[b].dc_pred = dc;
        data[0] = (dc * self.dequant[tq][0] as i32) as i16;

        let mut k = 1usize;
        loop {
            if self.code_bits < 16 {
                self.grow_buffer();
            }
            let c = ((self.code_buffer >> (32 - FAST_BITS)) & ((1 << FAST_BITS) - 1)) as usize;
            let r = self.fast_ac[ha][c] as i32;
            if r != 0 {
                k += ((r >> 4) & 15) as usize;
                let s = r & 15;
                if s > self.code_bits {
                    return Err("bad huffman code".into());
                }
                self.code_buffer <<= s;
                self.code_bits -= s;
                let zig = DEZIGZAG[k];
                k += 1;
                data[zig] = ((r >> 8) * self.dequant[tq][zig] as i32) as i16;
            } else {
                let rs = self.huff_decode(false, ha);
                if rs < 0 {
                    return Err("bad huffman code".into());
                }
                let s = rs & 15;
                let r = rs >> 4;
                if s == 0 {
                    if rs != 0xf0 {
                        break;
                    }
                    k += 16;
                } else {
                    k += r as usize;
                    let zig = DEZIGZAG[k];
                    k += 1;
                    data[zig] = (self.extend_receive(s) * self.dequant[tq][zig] as i32) as i16;
                }
            }
            if k >= 64 {
                break;
            }
        }
        Ok(())
    }

    fn decode_block_prog_dc(&mut self, data: &mut [i16; 64], b: usize) -> Result<(), String> {
        if self.spec_end != 0 {
            return Err("can't merge dc and ac".into());
        }
        if self.code_bits < 16 {
            self.grow_buffer();
        }
        if self.succ_high == 0 {
            *data = [0; 64];
            let t = self.huff_decode(true, self.comp[b].hd);
            if !(0..=15).contains(&t) {
                return Err("can't merge dc and ac".into());
            }
            let diff = if t != 0 { self.extend_receive(t) } else { 0 };
            let dc = self.comp[b].dc_pred.wrapping_add(diff);
            self.comp[b].dc_pred = dc;
            data[0] = (dc * (1 << self.succ_low)) as i16;
        } else if self.get_bit() {
            data[0] = data[0].wrapping_add((1 << self.succ_low) as i16);
        }
        Ok(())
    }

    fn decode_block_prog_ac(&mut self, data: &mut [i16; 64], ha: usize) -> Result<(), String> {
        if self.spec_start == 0 {
            return Err("can't merge dc and ac".into());
        }

        if self.succ_high == 0 {
            let shift = self.succ_low;
            if self.eob_run != 0 {
                self.eob_run -= 1;
                return Ok(());
            }
            let mut k = self.spec_start;
            loop {
                if self.code_bits < 16 {
                    self.grow_buffer();
                }
                let c = ((self.code_buffer >> (32 - FAST_BITS)) & ((1 << FAST_BITS) - 1)) as usize;
                let r = self.fast_ac[ha][c] as i32;
                if r != 0 {
                    k += (r >> 4) & 15;
                    let s = r & 15;
                    if s > self.code_bits {
                        return Err("bad huffman code".into());
                    }
                    self.code_buffer <<= s;
                    self.code_bits -= s;
                    let zig = DEZIGZAG[k as usize];
                    k += 1;
                    data[zig] = ((r >> 8) * (1 << shift)) as i16;
                } else {
                    let rs = self.huff_decode(false, ha);
                    if rs < 0 {
                        return Err("bad huffman code".into());
                    }
                    let s = rs & 15;
                    let r = rs >> 4;
                    if s == 0 {
                        if r < 15 {
                            self.eob_run = (1 << r) - 1;
                            if r != 0 {
                                self.eob_run += self.get_bits(r);
                            }
                            break;
                        }
                        k += 16;
                    } else {
                        k += r;
                        let zig = DEZIGZAG[k as usize];
                        k += 1;
                        data[zig] = (self.extend_receive(s) * (1 << shift)) as i16;
                    }
                }
                if k > self.spec_end {
                    break;
                }
            }
            return Ok(());
        }

        // Refinement pass: every coefficient already placed gets one more bit,
        // and the runs are counted only over the ones still zero.
        let bit = 1i16 << self.succ_low;
        if self.eob_run != 0 {
            self.eob_run -= 1;
            for k in self.spec_start..=self.spec_end {
                let p = &mut data[DEZIGZAG[k as usize]];
                if *p != 0 && self.get_bit() && (*p & bit) == 0 {
                    *p = if *p > 0 { *p + bit } else { *p - bit };
                }
            }
            return Ok(());
        }

        let mut k = self.spec_start;
        loop {
            let rs = self.huff_decode(false, ha);
            if rs < 0 {
                return Err("bad huffman code".into());
            }
            let s = rs & 15;
            let mut r = rs >> 4;
            let mut value = 0i16;
            if s == 0 {
                if r < 15 {
                    self.eob_run = (1 << r) - 1;
                    if r != 0 {
                        self.eob_run += self.get_bits(r);
                    }
                    r = 64; // force the end of the block
                }
                // r == 15 writes sixteen zeroes, which the run below already does
            } else {
                if s != 1 {
                    return Err("bad huffman code".into());
                }
                value = if self.get_bit() { bit } else { -bit };
            }

            while k <= self.spec_end {
                let index = DEZIGZAG[k as usize];
                k += 1;
                if data[index] != 0 {
                    if self.get_bit() && (data[index] & bit) == 0 {
                        data[index] = if data[index] > 0 { data[index] + bit } else { data[index] - bit };
                    }
                } else {
                    if r == 0 {
                        data[index] = value;
                        break;
                    }
                    r -= 1;
                }
            }
            if k > self.spec_end {
                break;
            }
        }
        Ok(())
    }

    // ---- markers ---------------------------------------------------------

    fn get_marker(&mut self) -> u8 {
        if self.marker != MARKER_NONE {
            let m = self.marker;
            self.marker = MARKER_NONE;
            return m;
        }
        let mut x = self.get8();
        if x != 0xff {
            return MARKER_NONE;
        }
        while x == 0xff {
            x = self.get8();
        }
        x
    }

    fn process_marker(&mut self, m: u8) -> Result<bool, String> {
        match m {
            MARKER_NONE => Err("expected marker".into()),
            0xdd => {
                if self.get16be() != 4 {
                    return Err("bad DRI len".into());
                }
                self.restart_interval = self.get16be() as i32;
                Ok(true)
            }
            0xdb => {
                let mut l = self.get16be() as i32 - 2;
                while l > 0 {
                    let q = self.get8();
                    let p = q >> 4;
                    let sixteen = p != 0;
                    let t = (q & 15) as usize;
                    if p > 1 {
                        return Err("bad DQT type".into());
                    }
                    if t > 3 {
                        return Err("bad DQT table".into());
                    }
                    for i in 0..64 {
                        let value = if sixteen { self.get16be() as u16 } else { self.get8() as u16 };
                        self.dequant[t][DEZIGZAG[i]] = value;
                    }
                    l -= if sixteen { 129 } else { 65 };
                }
                Ok(l == 0)
            }
            0xc4 => {
                let mut l = self.get16be() as i32 - 2;
                while l > 0 {
                    let q = self.get8();
                    let tc = q >> 4;
                    let th = (q & 15) as usize;
                    if tc > 1 || th > 3 {
                        return Err("bad DHT header".into());
                    }
                    let mut sizes = [0i32; 16];
                    let mut n = 0usize;
                    for size in &mut sizes {
                        *size = self.get8() as i32;
                        n += *size as usize;
                    }
                    if n > 256 {
                        return Err("bad DHT header".into());
                    }
                    l -= 17;
                    if tc == 0 {
                        self.huff_dc[th].build(&sizes)?;
                        for i in 0..n {
                            self.huff_dc[th].values[i] = self.get8();
                        }
                    } else {
                        self.huff_ac[th].build(&sizes)?;
                        for i in 0..n {
                            self.huff_ac[th].values[i] = self.get8();
                        }
                        let mut fast_ac = [0i16; 1 << FAST_BITS];
                        self.huff_ac[th].build_fast_ac(&mut fast_ac);
                        self.fast_ac[th] = fast_ac;
                    }
                    l -= n as i32;
                }
                Ok(l == 0)
            }
            0xe0..=0xef | 0xfe => {
                let mut l = self.get16be() as i32;
                if l < 2 {
                    return Err("bad APP len".into());
                }
                l -= 2;
                if m == 0xe0 && l >= 5 {
                    let mut ok = true;
                    for tag in b"JFIF\0" {
                        if self.get8() != *tag {
                            ok = false;
                        }
                    }
                    l -= 5;
                    if ok {
                        self.jfif = true;
                    }
                } else if m == 0xee && l >= 12 {
                    let mut ok = true;
                    for tag in b"Adobe\0" {
                        if self.get8() != *tag {
                            ok = false;
                        }
                    }
                    l -= 6;
                    if ok {
                        self.get8();
                        self.get16be();
                        self.get16be();
                        self.app14_colour_transform = self.get8() as i32;
                        l -= 6;
                    }
                }
                self.skip(l.max(0) as usize);
                Ok(true)
            }
            _ => Err("unknown marker".into()),
        }
    }

    fn process_scan_header(&mut self) -> Result<(), String> {
        let ls = self.get16be();
        self.scan_n = self.get8() as usize;
        if self.scan_n < 1 || self.scan_n > 4 || self.scan_n > self.img_n {
            return Err("bad SOS component count".into());
        }
        if ls != 6 + 2 * self.scan_n {
            return Err("bad SOS len".into());
        }
        for i in 0..self.scan_n {
            let id = self.get8();
            let q = self.get8();
            let Some(which) = self.comp.iter().position(|c| c.id == id) else {
                return Err("bad SOS component".into());
            };
            self.comp[which].hd = (q >> 4) as usize;
            self.comp[which].ha = (q & 15) as usize;
            if self.comp[which].hd > 3 || self.comp[which].ha > 3 {
                return Err("bad huff table".into());
            }
            self.order[i] = which;
        }

        self.spec_start = self.get8() as i32;
        self.spec_end = self.get8() as i32;
        let aa = self.get8() as i32;
        self.succ_high = aa >> 4;
        self.succ_low = aa & 15;
        if self.progressive {
            if self.spec_start > 63
                || self.spec_end > 63
                || self.spec_start > self.spec_end
                || self.succ_high > 13
                || self.succ_low > 13
            {
                return Err("bad SOS".into());
            }
        } else {
            if self.spec_start != 0 || self.succ_high != 0 || self.succ_low != 0 {
                return Err("bad SOS".into());
            }
            self.spec_end = 63;
        }
        Ok(())
    }

    fn process_frame_header(&mut self) -> Result<(), String> {
        let lf = self.get16be();
        if lf < 11 {
            return Err("bad SOF len".into());
        }
        if self.get8() != 8 {
            return Err("only 8-bit JPEG is supported".into());
        }
        self.img_y = self.get16be();
        self.img_x = self.get16be();
        if self.img_y == 0 || self.img_x == 0 {
            return Err("bad dimensions".into());
        }
        let c = self.get8() as usize;
        if c != 3 && c != 1 && c != 4 {
            return Err("bad component count".into());
        }
        self.img_n = c;
        if lf != 8 + 3 * c {
            return Err("bad SOF len".into());
        }

        self.comp = vec![Component::default(); c];
        self.rgb_components = 0;
        for i in 0..c {
            self.comp[i].id = self.get8();
            if c == 3 && self.comp[i].id == b"RGB"[i] {
                self.rgb_components += 1;
            }
            let q = self.get8();
            self.comp[i].h = (q >> 4) as usize;
            self.comp[i].v = (q & 15) as usize;
            if self.comp[i].h == 0 || self.comp[i].h > 4 || self.comp[i].v == 0 || self.comp[i].v > 4 {
                return Err("bad sampling factor".into());
            }
            self.comp[i].tq = self.get8() as usize;
            if self.comp[i].tq > 3 {
                return Err("bad TQ".into());
            }
        }

        self.h_max = self.comp.iter().map(|c| c.h).max().unwrap_or(1);
        self.v_max = self.comp.iter().map(|c| c.v).max().unwrap_or(1);
        for comp in &self.comp {
            if self.h_max % comp.h != 0 || self.v_max % comp.v != 0 {
                return Err("fractional sampling ratio".into());
            }
        }

        self.mcu_w = self.h_max * 8;
        self.mcu_h = self.v_max * 8;
        self.mcu_x = (self.img_x + self.mcu_w - 1) / self.mcu_w;
        self.mcu_y = (self.img_y + self.mcu_h - 1) / self.mcu_h;

        let (mcu_x, mcu_y, h_max, v_max) = (self.mcu_x, self.mcu_y, self.h_max, self.v_max);
        let progressive = self.progressive;
        let (img_x, img_y) = (self.img_x, self.img_y);
        for comp in &mut self.comp {
            comp.x = (img_x * comp.h + h_max - 1) / h_max;
            comp.y = (img_y * comp.v + v_max - 1) / v_max;
            // Padded out to whole MCUs; the surplus is dropped at colour
            // conversion rather than here.
            comp.w2 = mcu_x * comp.h * 8;
            comp.h2 = mcu_y * comp.v * 8;
            comp.data = vec![0u8; comp.w2 * comp.h2];
            if progressive {
                comp.coeff_w = comp.w2 / 8;
                comp.coeff_h = comp.h2 / 8;
                comp.coeff = vec![0i16; comp.w2 * comp.h2];
            }
        }
        Ok(())
    }

    fn decode_header(&mut self) -> Result<(), String> {
        self.jfif = false;
        self.app14_colour_transform = -1;
        self.marker = MARKER_NONE;
        if self.get_marker() != 0xd8 {
            return Err("no SOI".into());
        }
        let mut m = self.get_marker();
        while !matches!(m, 0xc0 | 0xc1 | 0xc2) {
            self.process_marker(m)?;
            m = self.get_marker();
            while m == MARKER_NONE {
                if self.at_eof() {
                    return Err("no SOF".into());
                }
                m = self.get_marker();
            }
        }
        self.progressive = m == 0xc2;
        self.process_frame_header()
    }

    /// Some encoders leave junk after the last scan; step over it, but stop on
    /// anything that looks like a real marker.
    fn skip_junk_at_end(&mut self) -> u8 {
        while !self.at_eof() {
            let mut x = self.get8();
            while x == 0xff {
                if self.at_eof() {
                    return MARKER_NONE;
                }
                x = self.get8();
                if x != 0x00 && x != 0xff {
                    return x;
                }
            }
        }
        MARKER_NONE
    }

    fn parse_entropy_coded_data(&mut self) -> Result<(), String> {
        self.reset();

        if !self.progressive {
            if self.scan_n == 1 {
                let n = self.order[0];
                let w = (self.comp[n].x + 7) >> 3;
                let h = (self.comp[n].y + 7) >> 3;
                let mut data = [0i16; 64];
                for j in 0..h {
                    for i in 0..w {
                        self.decode_block(&mut data, n)?;
                        let (w2, offset) = (self.comp[n].w2, self.comp[n].w2 * j * 8 + i * 8);
                        idct_block(&mut self.comp[n].data, offset, w2, &data);
                        self.todo -= 1;
                        if self.todo <= 0 {
                            if self.code_bits < 24 {
                                self.grow_buffer();
                            }
                            if !is_restart(self.marker) {
                                return Ok(());
                            }
                            self.reset();
                        }
                    }
                }
                return Ok(());
            }

            let mut data = [0i16; 64];
            for j in 0..self.mcu_y {
                for i in 0..self.mcu_x {
                    for k in 0..self.scan_n {
                        let n = self.order[k];
                        for y in 0..self.comp[n].v {
                            for x in 0..self.comp[n].h {
                                let x2 = (i * self.comp[n].h + x) * 8;
                                let y2 = (j * self.comp[n].v + y) * 8;
                                self.decode_block(&mut data, n)?;
                                let (w2, offset) = (self.comp[n].w2, self.comp[n].w2 * y2 + x2);
                                idct_block(&mut self.comp[n].data, offset, w2, &data);
                            }
                        }
                    }
                    self.todo -= 1;
                    if self.todo <= 0 {
                        if self.code_bits < 24 {
                            self.grow_buffer();
                        }
                        if !is_restart(self.marker) {
                            return Ok(());
                        }
                        self.reset();
                    }
                }
            }
            return Ok(());
        }

        if self.scan_n == 1 {
            let n = self.order[0];
            let w = (self.comp[n].x + 7) >> 3;
            let h = (self.comp[n].y + 7) >> 3;
            for j in 0..h {
                for i in 0..w {
                    let base = 64 * (i + j * self.comp[n].coeff_w);
                    let mut data: [i16; 64] = self.comp[n].coeff[base..base + 64].try_into().unwrap();
                    if self.spec_start == 0 {
                        self.decode_block_prog_dc(&mut data, n)?;
                    } else {
                        let ha = self.comp[n].ha;
                        self.decode_block_prog_ac(&mut data, ha)?;
                    }
                    self.comp[n].coeff[base..base + 64].copy_from_slice(&data);
                    self.todo -= 1;
                    if self.todo <= 0 {
                        if self.code_bits < 24 {
                            self.grow_buffer();
                        }
                        if !is_restart(self.marker) {
                            return Ok(());
                        }
                        self.reset();
                    }
                }
            }
            return Ok(());
        }

        for j in 0..self.mcu_y {
            for i in 0..self.mcu_x {
                for k in 0..self.scan_n {
                    let n = self.order[k];
                    for y in 0..self.comp[n].v {
                        for x in 0..self.comp[n].h {
                            let x2 = i * self.comp[n].h + x;
                            let y2 = j * self.comp[n].v + y;
                            let base = 64 * (x2 + y2 * self.comp[n].coeff_w);
                            let mut data: [i16; 64] =
                                self.comp[n].coeff[base..base + 64].try_into().unwrap();
                            self.decode_block_prog_dc(&mut data, n)?;
                            self.comp[n].coeff[base..base + 64].copy_from_slice(&data);
                        }
                    }
                }
                self.todo -= 1;
                if self.todo <= 0 {
                    if self.code_bits < 24 {
                        self.grow_buffer();
                    }
                    if !is_restart(self.marker) {
                        return Ok(());
                    }
                    self.reset();
                }
            }
        }
        Ok(())
    }

    /// A progressive image is only dequantized and transformed once every scan
    /// has contributed its bits.
    fn finish_progressive(&mut self) {
        for n in 0..self.img_n {
            let w = (self.comp[n].x + 7) >> 3;
            let h = (self.comp[n].y + 7) >> 3;
            let tq = self.comp[n].tq;
            for j in 0..h {
                for i in 0..w {
                    let base = 64 * (i + j * self.comp[n].coeff_w);
                    let mut data = [0i16; 64];
                    for k in 0..64 {
                        data[k] = self.comp[n].coeff[base + k].wrapping_mul(self.dequant[tq][k] as i16);
                    }
                    let (w2, offset) = (self.comp[n].w2, self.comp[n].w2 * j * 8 + i * 8);
                    idct_block(&mut self.comp[n].data, offset, w2, &data);
                }
            }
        }
    }

    fn decode_image(&mut self) -> Result<(), String> {
        self.restart_interval = 0;
        self.decode_header()?;
        let mut m = self.get_marker();
        while m != 0xd9 {
            if m == 0xda {
                self.process_scan_header()?;
                self.parse_entropy_coded_data()?;
                if self.marker == MARKER_NONE {
                    self.marker = self.skip_junk_at_end();
                }
                m = self.get_marker();
                if is_restart(m) {
                    m = self.get_marker();
                }
            } else if m == 0xdc {
                if self.get16be() != 4 {
                    return Err("bad DNL len".into());
                }
                if self.get16be() != self.img_y {
                    return Err("bad DNL height".into());
                }
                m = self.get_marker();
            } else {
                if !self.process_marker(m)? {
                    return Ok(());
                }
                m = self.get_marker();
            }
            if self.at_eof() && m == MARKER_NONE {
                break;
            }
        }
        if self.progressive {
            self.finish_progressive();
        }
        Ok(())
    }
}

fn is_restart(m: u8) -> bool {
    (0xd0..=0xd7).contains(&m)
}

fn clamp_byte(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}

/// One row or column of the integer IDCT, from jidctint's DCT_ISLOW.
#[allow(clippy::too_many_arguments)]
fn idct_1d(s0: i32, s1: i32, s2: i32, s3: i32, s4: i32, s5: i32, s6: i32, s7: i32) -> [i32; 8] {
    // The constants are the DCT basis scaled by 1<<12.
    const fn f2f(x: f64) -> i32 {
        (x * 4096.0 + 0.5) as i32
    }
    const fn fsh(x: i32) -> i32 {
        x * 4096
    }

    let p2 = s2;
    let p3 = s6;
    let p1 = (p2 + p3).wrapping_mul(f2f(0.5411961));
    let t2 = p1.wrapping_add(p3.wrapping_mul(f2f(-1.847759065)));
    let t3 = p1.wrapping_add(p2.wrapping_mul(f2f(0.765366865)));
    let p2 = s0;
    let p3 = s4;
    let t0 = fsh(p2.wrapping_add(p3));
    let t1 = fsh(p2.wrapping_sub(p3));
    let x0 = t0.wrapping_add(t3);
    let x3 = t0.wrapping_sub(t3);
    let x1 = t1.wrapping_add(t2);
    let x2 = t1.wrapping_sub(t2);

    let mut t0 = s7;
    let mut t1 = s5;
    let mut t2 = s3;
    let mut t3 = s1;
    let p3 = t0.wrapping_add(t2);
    let p4 = t1.wrapping_add(t3);
    let p1 = t0.wrapping_add(t3);
    let p2 = t1.wrapping_add(t2);
    let p5 = (p3.wrapping_add(p4)).wrapping_mul(f2f(1.175875602));
    t0 = t0.wrapping_mul(f2f(0.298631336));
    t1 = t1.wrapping_mul(f2f(2.053119869));
    t2 = t2.wrapping_mul(f2f(3.072711026));
    t3 = t3.wrapping_mul(f2f(1.501321110));
    let p1 = p5.wrapping_add(p1.wrapping_mul(f2f(-0.899976223)));
    let p2 = p5.wrapping_add(p2.wrapping_mul(f2f(-2.562915447)));
    let p3 = p3.wrapping_mul(f2f(-1.961570560));
    let p4 = p4.wrapping_mul(f2f(-0.390180644));
    t3 = t3.wrapping_add(p1.wrapping_add(p4));
    t2 = t2.wrapping_add(p2.wrapping_add(p3));
    t1 = t1.wrapping_add(p2.wrapping_add(p4));
    t0 = t0.wrapping_add(p1.wrapping_add(p3));

    [x0, x1, x2, x3, t0, t1, t2, t3]
}

fn idct_block(out: &mut [u8], offset: usize, stride: usize, data: &[i16; 64]) {
    let mut val = [0i32; 64];

    for i in 0..8 {
        let d = |k: usize| data[i + k * 8] as i32;
        if (1..8).all(|k| d(k) == 0) {
            // A column with only a DC term is flat; skip the transform.
            let dcterm = d(0) * 4;
            for k in 0..8 {
                val[i + k * 8] = dcterm;
            }
        } else {
            let [x0, x1, x2, x3, t0, t1, t2, t3] =
                idct_1d(d(0), d(1), d(2), d(3), d(4), d(5), d(6), d(7));
            // Undo the 1<<12 scaling but keep two extra bits of precision.
            let (x0, x1, x2, x3) = (x0 + 512, x1 + 512, x2 + 512, x3 + 512);
            val[i] = (x0 + t3) >> 10;
            val[i + 56] = (x0 - t3) >> 10;
            val[i + 8] = (x1 + t2) >> 10;
            val[i + 48] = (x1 - t2) >> 10;
            val[i + 16] = (x2 + t1) >> 10;
            val[i + 40] = (x2 - t1) >> 10;
            val[i + 24] = (x3 + t0) >> 10;
            val[i + 32] = (x3 - t0) >> 10;
        }
    }

    for i in 0..8 {
        let v = &val[i * 8..i * 8 + 8];
        let [x0, x1, x2, x3, t0, t1, t2, t3] =
            idct_1d(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]);
        // 1<<12 from the constants, 1<<2 from the first pass and 1<<3 from the
        // two sqrt(8) scalings: 1<<17 to remove, rounded, plus the 128 that
        // turns -128..127 into 0..255.
        let bias = 65536 + (128 << 17);
        let (x0, x1, x2, x3) = (x0 + bias, x1 + bias, x2 + bias, x3 + bias);
        let o = offset + i * stride;
        out[o] = clamp_byte((x0 + t3) >> 17);
        out[o + 7] = clamp_byte((x0 - t3) >> 17);
        out[o + 1] = clamp_byte((x1 + t2) >> 17);
        out[o + 6] = clamp_byte((x1 - t2) >> 17);
        out[o + 2] = clamp_byte((x2 + t1) >> 17);
        out[o + 5] = clamp_byte((x2 - t1) >> 17);
        out[o + 3] = clamp_byte((x3 + t0) >> 17);
        out[o + 4] = clamp_byte((x3 - t0) >> 17);
    }
}

/// How a chroma plane is stretched back up to the luma grid. JPEG's filters
/// are a weighted average of the two nearest samples, not a plain stretch.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Resample {
    None,
    V2,
    H2,
    Hv2,
    Generic,
}

fn div4(x: i32) -> u8 {
    (x >> 2) as u8
}

fn div16(x: i32) -> u8 {
    (x >> 4) as u8
}

fn resample_row(kind: Resample, out: &mut [u8], near: &[u8], far: &[u8], w: usize, hs: usize) -> bool {
    match kind {
        Resample::None => return false,
        Resample::V2 => {
            for i in 0..w {
                out[i] = div4(3 * near[i] as i32 + far[i] as i32 + 2);
            }
        }
        Resample::H2 => {
            if w == 1 {
                out[0] = near[0];
                out[1] = near[0];
            } else {
                out[0] = near[0];
                out[1] = div4(near[0] as i32 * 3 + near[1] as i32 + 2);
                for i in 1..w - 1 {
                    let n = 3 * near[i] as i32 + 2;
                    out[i * 2] = div4(n + near[i - 1] as i32);
                    out[i * 2 + 1] = div4(n + near[i + 1] as i32);
                }
                out[(w - 1) * 2] = div4(near[w - 2] as i32 * 3 + near[w - 1] as i32 + 2);
                out[(w - 1) * 2 + 1] = near[w - 1];
            }
        }
        Resample::Hv2 => {
            if w == 1 {
                let v = div4(3 * near[0] as i32 + far[0] as i32 + 2);
                out[0] = v;
                out[1] = v;
            } else {
                let mut t1 = 3 * near[0] as i32 + far[0] as i32;
                out[0] = div4(t1 + 2);
                for i in 1..w {
                    let t0 = t1;
                    t1 = 3 * near[i] as i32 + far[i] as i32;
                    out[i * 2 - 1] = div16(3 * t0 + t1 + 8);
                    out[i * 2] = div16(3 * t1 + t0 + 8);
                }
                out[w * 2 - 1] = div4(t1 + 2);
            }
        }
        Resample::Generic => {
            for i in 0..w {
                for j in 0..hs {
                    out[i * hs + j] = near[i];
                }
            }
        }
    }
    true
}

/// stb rounds at a 4096 scale and only then shifts up to 1<<20 — not the same
/// value as rounding at 1<<20 directly, and the difference is visible.
fn float2fixed(x: f32) -> i32 {
    (((x * 4096.0 + 0.5) as i32) << 8) as i32
}

fn ycbcr_to_rgb_row(out: &mut [u8], y: &[u8], pcb: &[u8], pcr: &[u8], count: usize) {
    for i in 0..count {
        let y_fixed = ((y[i] as i32) << 20) + (1 << 19); // rounding
        let cr = pcr[i] as i32 - 128;
        let cb = pcb[i] as i32 - 128;
        let r = y_fixed + cr * float2fixed(1.40200);
        // The low sixteen bits of the cb term are masked off, which is stb's,
        // and is part of what the SSE2 kernel reproduces.
        let g = y_fixed + (cr * -float2fixed(0.71414)) + ((cb * -float2fixed(0.34414)) & -65536);
        let b = y_fixed + cb * float2fixed(1.77200);
        out[i * 3] = clamp_byte(r >> 20);
        out[i * 3 + 1] = clamp_byte(g >> 20);
        out[i * 3 + 2] = clamp_byte(b >> 20);
    }
}

/// Multiply two 0..255 values as if they were 0..1, rounded — for CMYK.
fn blinn_8x8(x: u8, y: u8) -> u8 {
    let t = x as u32 * y as u32 + 128;
    ((t + (t >> 8)) >> 8) as u8
}

pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let mut d = Decoder::new(bytes);
    d.decode_image()?;

    let n = 3usize;
    let is_rgb = d.img_n == 3 && (d.rgb_components == 3 || (d.app14_colour_transform == 0 && !d.jfif));
    let decode_n = d.img_n;

    let mut kinds = Vec::with_capacity(decode_n);
    let mut linebufs: Vec<Vec<u8>> = Vec::with_capacity(decode_n);
    let mut state = Vec::with_capacity(decode_n);
    for k in 0..decode_n {
        let hs = d.h_max / d.comp[k].h;
        let vs = d.v_max / d.comp[k].v;
        kinds.push(match (hs, vs) {
            (1, 1) => Resample::None,
            (1, 2) => Resample::V2,
            (2, 1) => Resample::H2,
            (2, 2) => Resample::Hv2,
            _ => Resample::Generic,
        });
        // Room to upsample off the edge by up to four.
        linebufs.push(vec![0u8; d.img_x + 3]);
        state.push(ResampleState { hs, vs, ystep: vs >> 1, w_lores: (d.img_x + hs - 1) / hs, ypos: 0, line0: 0, line1: 0 });
    }

    let mut output = vec![0u8; n * d.img_x * d.img_y];
    let mut rows: Vec<Vec<u8>> = vec![Vec::new(); decode_n];

    for j in 0..d.img_y {
        for k in 0..decode_n {
            let r = &mut state[k];
            let y_bot = r.ystep >= (r.vs >> 1);
            let (near, far) = if y_bot { (r.line1, r.line0) } else { (r.line0, r.line1) };
            let w2 = d.comp[k].w2;
            let near = &d.comp[k].data[near..near + w2];
            let far = &d.comp[k].data[far..far + w2];
            let used = resample_row(kinds[k], &mut linebufs[k], near, far, r.w_lores, r.hs);
            rows[k] = if used { linebufs[k].clone() } else { near.to_vec() };

            r.ystep += 1;
            if r.ystep >= r.vs {
                r.ystep = 0;
                r.line0 = r.line1;
                r.ypos += 1;
                if r.ypos < d.comp[k].y {
                    r.line1 += w2;
                }
            }
        }

        let out = &mut output[n * d.img_x * j..n * d.img_x * (j + 1)];
        match d.img_n {
            3 if is_rgb => {
                for i in 0..d.img_x {
                    out[i * 3] = rows[0][i];
                    out[i * 3 + 1] = rows[1][i];
                    out[i * 3 + 2] = rows[2][i];
                }
            }
            3 => ycbcr_to_rgb_row(out, &rows[0], &rows[1], &rows[2], d.img_x),
            4 if d.app14_colour_transform == 0 => {
                for i in 0..d.img_x {
                    let m = rows[3][i];
                    out[i * 3] = blinn_8x8(rows[0][i], m);
                    out[i * 3 + 1] = blinn_8x8(rows[1][i], m);
                    out[i * 3 + 2] = blinn_8x8(rows[2][i], m);
                }
            }
            4 if d.app14_colour_transform == 2 => {
                ycbcr_to_rgb_row(out, &rows[0], &rows[1], &rows[2], d.img_x);
                for i in 0..d.img_x {
                    let m = rows[3][i];
                    for c in 0..3 {
                        out[i * 3 + c] = blinn_8x8(255 - out[i * 3 + c], m);
                    }
                }
            }
            4 => ycbcr_to_rgb_row(out, &rows[0], &rows[1], &rows[2], d.img_x),
            _ => {
                for i in 0..d.img_x {
                    let v = rows[0][i];
                    out[i * 3] = v;
                    out[i * 3 + 1] = v;
                    out[i * 3 + 2] = v;
                }
            }
        }
    }

    Ok(Image { width: d.img_x, height: d.img_y, rgb: output })
}

struct ResampleState {
    hs: usize,
    vs: usize,
    ystep: usize,
    w_lores: usize,
    ypos: usize,
    line0: usize,
    line1: usize,
}

#[cfg(test)]
mod tests {
    use redcommon::json::Json;

    /// Decoded pixels are compared by hash against what stb_image produces for
    /// the same file, so the corpus costs a few kilobytes rather than a few
    /// megabytes of expected output.
    #[test]
    fn decodes_exactly_what_stb_image_does() {
        let vectors = redcommon::json::parse(include_str!("../../tests/jpeg-vectors.json"))
            .expect("reference vectors are readable");
        let Some(Json::Arr(images)) = vectors.get("images") else { panic!("no images") };

        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/jpeg");
        for image in images {
            let name = image.str_field("name").expect("image name");
            let bytes = std::fs::read(format!("{dir}/{name}")).expect("test image is readable");
            let decoded = crate::material::jpeg::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));

            let expected_w = image.get("width").and_then(Json::as_u64).unwrap() as usize;
            let expected_h = image.get("height").and_then(Json::as_u64).unwrap() as usize;
            assert_eq!((decoded.width, decoded.height), (expected_w, expected_h), "{name}: size");

            let mut hasher = crate::sha256::Sha256::new();
            hasher.update(&decoded.rgb);
            let ours = crate::sha256::hex(&hasher.finish());
            assert_eq!(ours, image.str_field("sha256").unwrap(), "{name}: pixels differ");
        }
        assert!(images.len() >= 30, "corpus shrank");
    }
}
