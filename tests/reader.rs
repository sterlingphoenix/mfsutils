//! Reader tests against a synthetic MFS image, including a fragmented
//! file whose allocation chain is out of order on disk.

use mfsutils::mfs::{Fork, Volume};

const SECTOR: usize = 512;
const ABS: usize = 1024; // allocation block size
const DIR_START: usize = 4;
const DIR_LEN: usize = 2;
const ALLOC_START: usize = 8; // sector of allocation block #2
const NUM_ALLOC: usize = 100;

fn be16(buf: &mut [u8], off: usize, v: u16) {
    buf[off..off + 2].copy_from_slice(&v.to_be_bytes());
}
fn be32(buf: &mut [u8], off: usize, v: u32) {
    buf[off..off + 4].copy_from_slice(&v.to_be_bytes());
}

/// Write 12-bit map entry for allocation block `blk` (numbered from 2).
fn set_map(img: &mut [u8], blk: usize, val: u16) {
    let idx = blk - 2;
    let off = 2 * SECTOR + 0x40 + (idx / 2) * 3;
    if idx % 2 == 0 {
        img[off] = (val >> 4) as u8;
        img[off + 1] = (img[off + 1] & 0x0F) | ((val & 0x0F) << 4) as u8;
    } else {
        img[off + 1] = (img[off + 1] & 0xF0) | ((val >> 8) & 0x0F) as u8;
        img[off + 2] = (val & 0xFF) as u8;
    }
}

struct DirWriter<'a> {
    img: &'a mut [u8],
    sector: usize,
    used: usize,
    next_fnum: u32,
}

impl<'a> DirWriter<'a> {
    fn add(
        &mut self,
        name: &str,
        type_code: &[u8; 4],
        data: (u16, u32),
        rsrc: (u16, u32),
    ) {
        let mut esize = 0x33 + name.len();
        esize += esize & 1;
        if self.used + esize > SECTOR {
            self.sector += 1;
            self.used = 0;
        }
        let off = self.sector * SECTOR + self.used;
        let e = &mut self.img[off..off + esize];
        e[0] = 0x80;
        e[0x02..0x06].copy_from_slice(type_code);
        e[0x06..0x0A].copy_from_slice(b"UNIT");
        be32(e, 0x12, self.next_fnum);
        be16(e, 0x16, data.0);
        be32(e, 0x18, data.1);
        be32(e, 0x1C, data.1.div_ceil(ABS as u32) * ABS as u32);
        be16(e, 0x20, rsrc.0);
        be32(e, 0x22, rsrc.1);
        be32(e, 0x26, rsrc.1.div_ceil(ABS as u32) * ABS as u32);
        be32(e, 0x2A, 0x1234);
        be32(e, 0x2E, 0x5678);
        e[0x32] = name.len() as u8;
        e[0x33..0x33 + name.len()].copy_from_slice(name.as_bytes());
        self.next_fnum += 1;
        self.used += esize;
    }
}

fn fill_block(img: &mut [u8], blk: usize, byte: u8) {
    let off = ALLOC_START * SECTOR + (blk - 2) * ABS;
    img[off..off + ABS].fill(byte);
}

fn build_image() -> (Vec<u8>, usize) {
    let mut img = vec![0u8; 200 * 1024];

    // Files:
    //  "Plain": data 1500 bytes, blocks 2 -> 3
    //  "Frag":  data 2100 bytes, blocks 4 -> 7 -> 5 (out of order!)
    //  "Rsrc":  rsrc 100 bytes, block 6; empty data fork
    //  "Empty": no forks (lands in the second directory sector)
    set_map(&mut img, 2, 3);
    set_map(&mut img, 3, 1);
    set_map(&mut img, 4, 7);
    set_map(&mut img, 7, 5);
    set_map(&mut img, 5, 1);
    set_map(&mut img, 6, 1);
    fill_block(&mut img, 2, b'A');
    fill_block(&mut img, 3, b'B');
    fill_block(&mut img, 4, b'1');
    fill_block(&mut img, 7, b'2');
    fill_block(&mut img, 5, b'3');
    fill_block(&mut img, 6, b'R');

    let mut w = DirWriter {
        img: &mut img,
        sector: DIR_START,
        used: 0,
        next_fnum: 1,
    };
    w.add("Plain", b"TEXT", (2, 1500), (0, 0));
    w.add("Frag", b"BINA", (4, 2100), (0, 0));
    w.add("Rsrc", b"APPL", (0, 0), (6, 100));
    // Pad the rest of the sector so the next entry starts in sector 5.
    w.sector = DIR_START + 1;
    w.used = 0;
    w.add("Empty", b"    ", (0, 0), (0, 0));

    // MDB (written after the map so we can share the buffer freely).
    be16(&mut img, 2 * SECTOR, 0xD2D7);
    be32(&mut img, 2 * SECTOR + 0x02, 0x11111111); // create date
    be16(&mut img, 2 * SECTOR + 0x0C, 4); // num files
    be16(&mut img, 2 * SECTOR + 0x0E, DIR_START as u16);
    be16(&mut img, 2 * SECTOR + 0x10, DIR_LEN as u16);
    be16(&mut img, 2 * SECTOR + 0x12, NUM_ALLOC as u16);
    be32(&mut img, 2 * SECTOR + 0x14, ABS as u32);
    be16(&mut img, 2 * SECTOR + 0x1C, ALLOC_START as u16);
    be16(&mut img, 2 * SECTOR + 0x22, (NUM_ALLOC - 6) as u16);
    let name = b"Test Vol";
    img[2 * SECTOR + 0x24] = name.len() as u8;
    img[2 * SECTOR + 0x25..2 * SECTOR + 0x25 + name.len()].copy_from_slice(name);

    (img, 4)
}

#[test]
fn parses_volume_and_directory() {
    let (img, nfiles) = build_image();
    let vol = Volume::from_bytes(img).unwrap();
    assert_eq!(vol.info.name, "Test Vol");
    assert_eq!(vol.info.num_files as usize, nfiles);
    assert_eq!(vol.files.len(), nfiles);
    let names: Vec<&str> = vol.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Plain", "Frag", "Rsrc", "Empty"]);
    assert_eq!(&vol.files[0].type_code, b"TEXT");
    assert_eq!(vol.files[2].rsrc_len, 100);
}

#[test]
fn reads_contiguous_fork() {
    let (img, _) = build_image();
    let vol = Volume::from_bytes(img).unwrap();
    let data = vol.read_fork(&vol.files[0], Fork::Data).unwrap();
    assert_eq!(data.len(), 1500);
    assert!(data[..1024].iter().all(|&b| b == b'A'));
    assert!(data[1024..].iter().all(|&b| b == b'B'));
}

#[test]
fn follows_fragmented_chain_in_disk_order() {
    let (img, _) = build_image();
    let vol = Volume::from_bytes(img).unwrap();
    let data = vol.read_fork(&vol.files[1], Fork::Data).unwrap();
    assert_eq!(data.len(), 2100);
    // Logical order is 4 -> 7 -> 5, i.e. '1' then '2' then '3', even
    // though the blocks sit on disk as 4, 5, 7.
    assert!(data[..1024].iter().all(|&b| b == b'1'));
    assert!(data[1024..2048].iter().all(|&b| b == b'2'));
    assert!(data[2048..].iter().all(|&b| b == b'3'));
}

#[test]
fn reads_resource_fork_and_empty_forks() {
    let (img, _) = build_image();
    let vol = Volume::from_bytes(img).unwrap();
    let rsrc = vol.read_fork(&vol.files[2], Fork::Rsrc).unwrap();
    assert_eq!(rsrc.len(), 100);
    assert!(rsrc.iter().all(|&b| b == b'R'));
    assert!(vol.read_fork(&vol.files[2], Fork::Data).unwrap().is_empty());
    assert!(vol.read_fork(&vol.files[3], Fork::Data).unwrap().is_empty());
}

#[test]
fn rejects_non_mfs_data() {
    assert!(Volume::from_bytes(vec![0u8; 4096]).is_err());
}
