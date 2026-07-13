//! MacBinary II encoder (the format hcopy -m produces).
//!
//! 128-byte header, then the data fork and resource fork, each padded
//! with zeros to a 128-byte boundary. Header bytes 124-125 hold a
//! CRC-16/XMODEM of bytes 0-123.

use crate::mfs::FileEntry;
use crate::util::crc16;

fn pad128(v: &mut Vec<u8>) {
    while v.len() % 128 != 0 {
        v.push(0);
    }
}

pub fn encode(f: &FileEntry, data: &[u8], rsrc: &[u8]) -> Vec<u8> {
    let mut h = [0u8; 128];
    let n = f.raw_name.len().min(63);
    h[1] = n as u8;
    h[2..2 + n].copy_from_slice(&f.raw_name[..n]);
    h[65..69].copy_from_slice(&f.type_code);
    h[69..73].copy_from_slice(&f.creator);
    h[73] = (f.finder_flags >> 8) as u8;
    h[75..77].copy_from_slice(&f.location_v.to_be_bytes());
    h[77..79].copy_from_slice(&f.location_h.to_be_bytes());
    h[79..81].copy_from_slice(&f.folder.to_be_bytes());
    h[81] = f.locked as u8;
    h[83..87].copy_from_slice(&(data.len() as u32).to_be_bytes());
    h[87..91].copy_from_slice(&(rsrc.len() as u32).to_be_bytes());
    h[91..95].copy_from_slice(&f.create_date.to_be_bytes());
    h[95..99].copy_from_slice(&f.mod_date.to_be_bytes());
    h[101] = (f.finder_flags & 0xFF) as u8;
    h[122] = 129; // written by MacBinary II
    h[123] = 129; // minimum version to extract
    let crc = crc16(&h[..124], 0);
    h[124..126].copy_from_slice(&crc.to_be_bytes());

    let mut out = h.to_vec();
    out.extend_from_slice(data);
    pad128(&mut out);
    out.extend_from_slice(rsrc);
    pad128(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfs::FileEntry;

    fn entry() -> FileEntry {
        FileEntry {
            locked: false,
            type_code: *b"APPL",
            creator: *b"DONN",
            finder_flags: 0x2100,
            location_v: 10,
            location_h: 20,
            folder: 0,
            file_num: 7,
            data_start: 0,
            data_len: 3,
            data_phys: 1024,
            rsrc_start: 0,
            rsrc_len: 5,
            rsrc_phys: 1024,
            create_date: 0x12345678,
            mod_date: 0x23456789,
            raw_name: b"Test File".to_vec(),
            name: "Test File".into(),
        }
    }

    #[test]
    fn header_layout_and_crc() {
        let out = encode(&entry(), b"abc", b"ryryr");
        assert_eq!(out.len(), 128 + 128 + 128); // header + 1 data + 1 rsrc chunk
        assert_eq!(out[0], 0);
        assert_eq!(out[1], 9);
        assert_eq!(&out[2..11], b"Test File");
        assert_eq!(&out[65..69], b"APPL");
        assert_eq!(&out[69..73], b"DONN");
        assert_eq!(out[73], 0x21);
        assert_eq!(out[101], 0x00);
        assert_eq!(&out[83..87], &3u32.to_be_bytes());
        assert_eq!(&out[87..91], &5u32.to_be_bytes());
        assert_eq!(out[122], 129);
        let crc = u16::from_be_bytes([out[124], out[125]]);
        assert_eq!(crc, crc16(&out[..124], 0));
        assert_eq!(&out[128..131], b"abc");
        assert_eq!(&out[256..261], b"ryryr");
    }
}
