//! MFS disk image reader.
//!
//! Layout (512-byte sectors, all values big-endian):
//!
//! ```text
//! sectors 0..2:   boot blocks
//! sector  2..:    MDB: 64-byte volume info + 12-bit allocation map
//!                 (map may spill into following sectors)
//! drDirSt..:      flat file directory (drBlLen sectors)
//! drAlBlSt..:     allocation blocks (drAlBlkSiz bytes each), numbered
//!                 from 2 (values 0 and 1 in the map mean FREE and LAST)
//! ```
//!
//! Reference: Inside Macintosh, Volume II, "The File Manager".

use crate::util::decode_macroman;
use anyhow::{anyhow, bail, Result};
use std::fs;
use std::path::Path;

pub const SIGNATURE: u16 = 0xD2D7;
const SECTOR: usize = 512;
/// Offset of the disk data inside a DiskCopy 4.2 container.
const DC42_DATA_OFFSET: usize = 84;

fn be16(d: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([d[off], d[off + 1]])
}
fn be32(d: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]])
}

#[derive(Debug, Clone)]
pub struct VolumeInfo {
    pub name: String,
    pub create_date: u32,
    pub backup_date: u32,
    pub attributes: u16,
    pub num_files: u16,
    pub dir_start: u16,
    pub dir_len: u16,
    pub num_alloc_blks: u16,
    pub alloc_blk_size: u32,
    pub clump_size: u32,
    pub alloc_blk_start: u16,
    pub next_file_num: u32,
    pub free_blks: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fork {
    Data,
    Rsrc,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub locked: bool,
    pub type_code: [u8; 4],
    pub creator: [u8; 4],
    pub finder_flags: u16,
    pub location_v: i16,
    pub location_h: i16,
    pub folder: u16,
    pub file_num: u32,
    pub data_start: u16,
    pub data_len: u32,
    pub data_phys: u32,
    pub rsrc_start: u16,
    pub rsrc_len: u32,
    pub rsrc_phys: u32,
    pub create_date: u32,
    pub mod_date: u32,
    /// Original MacRoman bytes of the name (needed for MacBinary/BinHex).
    pub raw_name: Vec<u8>,
    /// Name decoded to Unicode.
    pub name: String,
}

pub struct Volume {
    data: Vec<u8>,
    /// Byte offset of sector 0 inside the file (0 for raw images,
    /// 84 for DiskCopy 4.2 containers).
    base: usize,
    pub info: VolumeInfo,
    pub files: Vec<FileEntry>,
}

impl Volume {
    pub fn open(path: &Path) -> Result<Self> {
        let data = fs::read(path)
            .map_err(|e| anyhow!("cannot read {}: {e}", path.display()))?;
        Self::from_bytes(data)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        let base = Self::find_base(&data)?;
        let info = Self::parse_mdb(&data, base)?;
        let files = Self::parse_directory(&data, base, &info)?;
        Ok(Self { data, base, info, files })
    }

    fn find_base(data: &[u8]) -> Result<usize> {
        let sig_at = |off: usize| {
            data.len() >= off + 3 * SECTOR && be16(data, off + 2 * SECTOR) == SIGNATURE
        };
        if sig_at(0) {
            return Ok(0);
        }
        // DiskCopy 4.2: 84-byte header, magic 0x0100 at offset 0x52.
        if data.len() > DC42_DATA_OFFSET + 3 * SECTOR
            && be16(data, 0x52) == 0x0100
            && sig_at(DC42_DATA_OFFSET)
        {
            return Ok(DC42_DATA_OFFSET);
        }
        if data.len() >= 3 * SECTOR && be16(data, 2 * SECTOR) == 0x4244 {
            bail!("this is an HFS volume, not MFS (use hfsutils)");
        }
        bail!("no MFS signature found (not an MFS image?)");
    }

    fn parse_mdb(data: &[u8], base: usize) -> Result<VolumeInfo> {
        let m = &data[base + 2 * SECTOR..base + 2 * SECTOR + 64];
        let name_len = (m[0x24] as usize).min(27);
        let info = VolumeInfo {
            name: decode_macroman(&m[0x25..0x25 + name_len]),
            create_date: be32(m, 0x02),
            backup_date: be32(m, 0x06),
            attributes: be16(m, 0x0A),
            num_files: be16(m, 0x0C),
            dir_start: be16(m, 0x0E),
            dir_len: be16(m, 0x10),
            num_alloc_blks: be16(m, 0x12),
            alloc_blk_size: be32(m, 0x14),
            clump_size: be32(m, 0x18),
            alloc_blk_start: be16(m, 0x1C),
            next_file_num: be32(m, 0x1E),
            free_blks: be16(m, 0x22),
        };
        if info.alloc_blk_size == 0 || info.alloc_blk_size % SECTOR as u32 != 0 {
            bail!("bad allocation block size {}", info.alloc_blk_size);
        }
        let dir_end = (info.dir_start as usize + info.dir_len as usize) * SECTOR;
        if base + dir_end > data.len() {
            bail!("directory extends past end of image");
        }
        Ok(info)
    }

    fn parse_directory(data: &[u8], base: usize, info: &VolumeInfo) -> Result<Vec<FileEntry>> {
        let mut files = Vec::new();
        for sect in info.dir_start..info.dir_start + info.dir_len {
            let sec_start = base + sect as usize * SECTOR;
            let sec_end = sec_start + SECTOR;
            let mut off = sec_start;
            // Entries never cross sector boundaries; a clear in-use bit
            // marks the end of entries within a sector.
            while off + 0x33 <= sec_end {
                if data[off] & 0x80 == 0 {
                    break;
                }
                let name_len = data[off + 0x32] as usize;
                let mut esize = 0x33 + name_len;
                esize += esize & 1; // entries are padded to even length
                if off + esize > sec_end {
                    break; // malformed entry; don't run off the sector
                }
                let e = &data[off..off + esize];
                let raw_name = e[0x33..0x33 + name_len].to_vec();
                files.push(FileEntry {
                    locked: e[0x00] & 0x01 != 0,
                    type_code: e[0x02..0x06].try_into().unwrap(),
                    creator: e[0x06..0x0A].try_into().unwrap(),
                    finder_flags: be16(e, 0x0A),
                    location_v: be16(e, 0x0C) as i16,
                    location_h: be16(e, 0x0E) as i16,
                    folder: be16(e, 0x10),
                    file_num: be32(e, 0x12),
                    data_start: be16(e, 0x16),
                    data_len: be32(e, 0x18),
                    data_phys: be32(e, 0x1C),
                    rsrc_start: be16(e, 0x20),
                    rsrc_len: be32(e, 0x22),
                    rsrc_phys: be32(e, 0x26),
                    create_date: be32(e, 0x2A),
                    mod_date: be32(e, 0x2E),
                    name: decode_macroman(&raw_name),
                    raw_name,
                });
                off += esize;
            }
        }
        Ok(files)
    }

    /// Value of 12-bit allocation map entry for allocation block number
    /// `blk` (block numbers start at 2; map index is blk - 2).
    fn map_entry(&self, blk: u16) -> Result<u16> {
        if blk < 2 || blk - 2 >= self.info.num_alloc_blks {
            bail!("allocation block {blk} out of range");
        }
        let idx = (blk - 2) as usize;
        let off = self.base + 2 * SECTOR + 0x40 + (idx / 2) * 3;
        if off + 3 > self.data.len() {
            bail!("allocation map truncated");
        }
        let d = &self.data;
        Ok(if idx % 2 == 0 {
            ((d[off] as u16) << 4) | ((d[off + 1] as u16) >> 4)
        } else {
            (((d[off + 1] & 0x0F) as u16) << 8) | d[off + 2] as u16
        })
    }

    /// Read a fork's logical content, following the allocation-block
    /// chain (files may be fragmented).
    pub fn read_fork(&self, f: &FileEntry, fork: Fork) -> Result<Vec<u8>> {
        let (start, len) = match fork {
            Fork::Data => (f.data_start, f.data_len as usize),
            Fork::Rsrc => (f.rsrc_start, f.rsrc_len as usize),
        };
        let mut out = Vec::with_capacity(len);
        if len == 0 {
            return Ok(out);
        }
        if start < 2 {
            bail!("\"{}\": fork has length {len} but no start block", f.name);
        }
        let abs = self.info.alloc_blk_size as usize;
        let area = self.base + self.info.alloc_blk_start as usize * SECTOR;
        let mut blk = start;
        let mut hops = 0u32;
        loop {
            hops += 1;
            if hops > self.info.num_alloc_blks as u32 + 2 {
                bail!("\"{}\": allocation chain loops", f.name);
            }
            if blk < 2 || blk - 2 >= self.info.num_alloc_blks {
                bail!("\"{}\": allocation chain leaves the volume", f.name);
            }
            let off = area + (blk as usize - 2) * abs;
            if off + abs > self.data.len() {
                bail!("\"{}\": allocation block {blk} past end of image", f.name);
            }
            let take = abs.min(len - out.len());
            out.extend_from_slice(&self.data[off..off + take]);
            if out.len() >= len {
                return Ok(out);
            }
            let next = self.map_entry(blk)?;
            if next == 1 {
                bail!(
                    "\"{}\": chain ended after {} bytes, expected {len}",
                    f.name,
                    out.len()
                );
            }
            if next == 0 {
                bail!("\"{}\": chain runs into a free block", f.name);
            }
            blk = next;
        }
    }
}
