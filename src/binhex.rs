//! BinHex 4.0 (.hqx) encoder.
//!
//! Stream = header (name, type/creator, flags, fork lengths) + CRC,
//! data fork + CRC, resource fork + CRC; the whole stream is RLE90
//! compressed, then 6-bit encoded with the BinHex alphabet and wrapped
//! at 64 columns between ':' delimiters.

use crate::mfs::FileEntry;
use crate::util::crc16;

pub const ALPHABET: &[u8; 64] =
    b"!\"#$%&'()*+,-012345689@ABCDEFGHIJKLMNPQRSTUVXYZ[`abcdefhijklmpqr";

const MARKER: u8 = 0x90;

/// RLE90: runs of 4..=255 identical bytes become `byte 0x90 count`;
/// a literal 0x90 is escaped as `0x90 0x00`.
fn rle90(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        let mut run = 1;
        while i + run < input.len() && input[i + run] == b && run < 255 {
            run += 1;
        }
        if b == MARKER {
            // Always escape the marker literally; never RLE it.
            for _ in 0..run {
                out.push(MARKER);
                out.push(0x00);
            }
        } else if run >= 4 {
            out.push(b);
            out.push(MARKER);
            out.push(run as u8);
        } else {
            for _ in 0..run {
                out.push(b);
            }
        }
        i += run;
    }
    out
}

/// 6-bit encode: 3 bytes -> 4 alphabet characters, zero-padded at the end.
fn six_bit(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() * 4 / 3 + 4);
    for chunk in input.chunks(3) {
        let mut buf = [0u8; 3];
        buf[..chunk.len()].copy_from_slice(chunk);
        let v = ((buf[0] as u32) << 16) | ((buf[1] as u32) << 8) | buf[2] as u32;
        let nchars = match chunk.len() {
            1 => 2,
            2 => 3,
            _ => 4,
        };
        for k in 0..nchars {
            out.push(ALPHABET[((v >> (18 - 6 * k)) & 0x3F) as usize]);
        }
    }
    out
}

pub fn encode(f: &FileEntry, data: &[u8], rsrc: &[u8]) -> Vec<u8> {
    let mut s: Vec<u8> = Vec::new();
    let n = f.raw_name.len().min(63);
    s.push(n as u8);
    s.extend_from_slice(&f.raw_name[..n]);
    s.push(0); // version
    s.extend_from_slice(&f.type_code);
    s.extend_from_slice(&f.creator);
    s.extend_from_slice(&f.finder_flags.to_be_bytes());
    s.extend_from_slice(&(data.len() as u32).to_be_bytes());
    s.extend_from_slice(&(rsrc.len() as u32).to_be_bytes());
    let hcrc = crc16(&s, 0);
    s.extend_from_slice(&hcrc.to_be_bytes());
    s.extend_from_slice(data);
    s.extend_from_slice(&crc16(data, 0).to_be_bytes());
    s.extend_from_slice(rsrc);
    s.extend_from_slice(&crc16(rsrc, 0).to_be_bytes());

    let encoded = six_bit(&rle90(&s));

    let mut out = b"(This file must be converted with BinHex 4.0)\n\n".to_vec();
    let mut col = 0;
    out.push(b':');
    col += 1;
    for &c in &encoded {
        if col == 64 {
            out.push(b'\n');
            col = 0;
        }
        out.push(c);
        col += 1;
    }
    if col == 64 {
        out.push(b'\n');
    }
    out.push(b':');
    out.push(b'\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfs::FileEntry;

    fn entry(name: &str) -> FileEntry {
        FileEntry {
            locked: false,
            type_code: *b"TEXT",
            creator: *b"ttxt",
            finder_flags: 0,
            location_v: 0,
            location_h: 0,
            folder: 0,
            file_num: 1,
            data_start: 0,
            data_len: 0,
            data_phys: 0,
            rsrc_start: 0,
            rsrc_len: 0,
            rsrc_phys: 0,
            create_date: 0,
            mod_date: 0,
            raw_name: name.as_bytes().to_vec(),
            name: name.into(),
        }
    }

    // --- decoder (test-only) to verify the encoder round-trips ---

    fn unsix(input: &[u8]) -> Vec<u8> {
        let mut bits = 0u32;
        let mut nbits = 0;
        let mut out = Vec::new();
        for &c in input {
            let v = ALPHABET.iter().position(|&a| a == c).expect("bad char") as u32;
            bits = (bits << 6) | v;
            nbits += 6;
            if nbits >= 8 {
                nbits -= 8;
                out.push((bits >> nbits) as u8);
            }
        }
        out
    }

    fn unrle(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < input.len() {
            if input[i] == MARKER {
                let count = input[i + 1];
                if count == 0 {
                    out.push(MARKER);
                } else {
                    let last = *out.last().expect("run with no previous byte");
                    for _ in 1..count {
                        out.push(last);
                    }
                }
                i += 2;
            } else {
                out.push(input[i]);
                i += 1;
            }
        }
        out
    }

    #[test]
    fn roundtrip() {
        // Data with a long run to exercise RLE, and 0x90 bytes to
        // exercise marker escaping.
        let data: Vec<u8> = [b"hello \x90\x90 world".to_vec(), vec![0xAA; 40]].concat();
        let rsrc = vec![0u8; 300];
        let out = encode(&entry("My File"), &data, &rsrc);

        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("(This file must be converted with BinHex 4.0)"));
        for line in text.lines().skip(2) {
            assert!(line.len() <= 64, "line too long: {}", line.len());
        }
        let stream: String = text
            .lines()
            .skip(2)
            .collect::<String>()
            .trim_start_matches(':')
            .trim_end_matches(':')
            .to_string();
        let raw = unrle(&unsix(stream.as_bytes()));

        // Parse header back.
        let n = raw[0] as usize;
        assert_eq!(&raw[1..1 + n], b"My File");
        let mut p = 1 + n;
        assert_eq!(raw[p], 0);
        p += 1;
        assert_eq!(&raw[p..p + 4], b"TEXT");
        assert_eq!(&raw[p + 4..p + 8], b"ttxt");
        let dlen = u32::from_be_bytes(raw[p + 10..p + 14].try_into().unwrap()) as usize;
        let rlen = u32::from_be_bytes(raw[p + 14..p + 18].try_into().unwrap()) as usize;
        assert_eq!(dlen, data.len());
        assert_eq!(rlen, rsrc.len());
        let hcrc = u16::from_be_bytes(raw[p + 18..p + 20].try_into().unwrap());
        assert_eq!(hcrc, crc16(&raw[..p + 18], 0));
        p += 20;
        assert_eq!(&raw[p..p + dlen], &data[..]);
        let dcrc = u16::from_be_bytes(raw[p + dlen..p + dlen + 2].try_into().unwrap());
        assert_eq!(dcrc, crc16(&data, 0));
        p += dlen + 2;
        assert_eq!(&raw[p..p + rlen], &rsrc[..]);
        let rcrc = u16::from_be_bytes(raw[p + rlen..p + rlen + 2].try_into().unwrap());
        assert_eq!(rcrc, crc16(&rsrc, 0));
        assert_eq!(p + rlen + 2, raw.len());
    }

    #[test]
    fn alphabet_is_64_unique_chars() {
        let mut seen = [false; 256];
        for &c in ALPHABET.iter() {
            assert!(!seen[c as usize], "duplicate char {c}");
            seen[c as usize] = true;
        }
    }
}
