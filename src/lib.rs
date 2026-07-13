//! mfsutils — read Macintosh MFS (1984 flat filesystem) disk images.
//!
//! Modeled on hfsutils: `mfsmount` records the current image in a state
//! file (`~/.mfscwd`), and the other tools (`mfsls`, `mfscd`, `mfspwd`,
//! `mfscopy`, `mfsumount`) operate on it.
//!
//! Format reference: Apple, "Inside Macintosh, Volume II" (1985).

pub mod binhex;
pub mod commands;
pub mod macbinary;
pub mod mfs;
pub mod mount;
pub mod util;
