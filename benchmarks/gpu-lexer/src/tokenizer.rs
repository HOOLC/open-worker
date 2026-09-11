//! Matches gpu-lexer 0.0.2 UTF-16 feature encoding, with UTF-8 output ranges.
#[derive(Debug)]
pub struct Tokens {
    pub features: Vec<u32>,
    pub ranges: Vec<(usize, usize)>,
    pub utf16_ranges: Vec<u32>,
}
fn word(c: u16) -> bool {
    c > 127 || c == 95 || (c as u8).is_ascii_alphanumeric()
}
fn space(c: u16) -> bool {
    matches!(c, 9 | 11 | 12 | 32)
}
fn normalized(c: u16) -> u32 {
    if c > 127 { 95 } else { c as u32 }
}
fn pair(a: u32, b: u32) -> u32 {
    [
        (47, 47),
        (47, 42),
        (42, 47),
        (45, 45),
        (61, 62),
        (58, 58),
        (60, 47),
        (123, 123),
        (36, 123),
        (125, 125),
        (45, 62),
        (63, 63),
        (63, 46),
        (60, 62),
    ]
    .iter()
    .position(|p| *p == (a, b))
    .map_or(0, |i| i as u32 + 1)
}
pub fn tokenize(code: &str) -> Tokens {
    let mut units = Vec::with_capacity(code.len());
    let mut bytes = Vec::with_capacity(code.len() + 1);
    for (offset, c) in code.char_indices() {
        for u in c.encode_utf16(&mut [0; 2]) {
            units.push(*u);
            bytes.push(offset);
        }
    }
    bytes.push(code.len());
    let mut result = Tokens {
        features: Vec::new(),
        ranges: Vec::new(),
        utf16_ranges: Vec::new(),
    };
    let (mut pos, mut bol, mut previous, mut previous_kind) = (0, true, u32::MAX, u32::MAX);
    while pos < units.len() {
        let start = pos;
        let first = units[pos];
        let kind = if matches!(first, 10 | 13) {
            2
        } else if space(first) {
            1
        } else if word(first) {
            0
        } else {
            3
        };
        let mut flags = if bol { 16 } else { 0 };
        let a = normalized(first);
        let mut last = a;
        let (mut hash, mut hash2) = (0u32, 0u32);
        match kind {
            0 => {
                hash = 2166136261;
                hash2 = 2654435769;
                while pos < units.len() && word(units[pos]) {
                    let t = normalized(units[pos]);
                    last = t;
                    hash = (hash ^ t).wrapping_mul(16777619);
                    hash2 = (hash2 ^ t).wrapping_mul(2246822519);
                    flags |= match t {
                        97..=122 => 1,
                        65..=90 => 2,
                        48..=57 => 4,
                        95 => 8,
                        _ => 0,
                    };
                    pos += 1;
                }
            }
            1 => {
                while pos < units.len() && space(units[pos]) {
                    last = normalized(units[pos]);
                    if units[pos] == 9 {
                        flags |= 32
                    };
                    pos += 1;
                }
            }
            2 => {
                pos += 1;
                if first == 13 && units.get(pos) == Some(&10) {
                    last = 10;
                    pos += 1;
                }
            }
            _ => {
                if first == 92 {
                    flags |= 64
                };
                pos += 1;
            }
        }
        let length = (pos - start) as u32;
        let f0 = kind
            | (31 - length.leading_zeros()).min(7) << 2
            | a << 5
            | last << 12
            | (hash & 255) << 19;
        let mut f1 = flags | if kind == 0 { (hash2 & 127) << 15 } else { 0 };
        if !result.features.is_empty() {
            let p = pair(previous, a);
            f1 |= p << 7;
            let prior = result.features.last_mut().unwrap();
            *prior |= p << 11;
            if kind == 3 || previous_kind == 3 {
                let p = 1 + ((previous.wrapping_add(1).wrapping_mul(131) ^ a) % 31);
                f1 |= p << 22;
                *prior |= p << 27;
            }
        }
        result.features.extend([f0, f1]);
        result.ranges.push((bytes[start], bytes[pos]));
        result.utf16_ranges.extend([start as u32, pos as u32]);
        previous = last;
        previous_kind = kind;
        if kind == 2 {
            bol = true
        } else if kind != 1 {
            bol = false
        }
    }
    result
}
