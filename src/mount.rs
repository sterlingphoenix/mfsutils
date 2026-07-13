//! Mount state, hfsutils-style: `mfsmount` records the current image
//! (and working "folder" prefix) in a small state file so the other
//! tools know what to operate on. Default is `~/.mfscwd`; override with
//! the MFSCWD environment variable.

use crate::mfs::Volume;
use anyhow::{anyhow, Result};
use std::env;
use std::fs;
use std::path::PathBuf;

pub struct MountState {
    pub image: PathBuf,
    /// Current pseudo-folder prefix ("" = volume root). MFS is flat,
    /// but names may contain ':' as a folder illusion.
    pub prefix: String,
}

pub fn state_path() -> PathBuf {
    if let Ok(p) = env::var("MFSCWD") {
        return PathBuf::from(p);
    }
    let home = env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".mfscwd")
}

pub fn save(state: &MountState) -> Result<()> {
    fs::write(
        state_path(),
        format!("{}\n{}\n", state.image.display(), state.prefix),
    )?;
    Ok(())
}

pub fn load() -> Result<MountState> {
    let text = fs::read_to_string(state_path())
        .map_err(|_| anyhow!("no volume is mounted (run mfsmount <image-path> first)"))?;
    let mut lines = text.lines();
    let image = lines
        .next()
        .filter(|l| !l.is_empty())
        .ok_or_else(|| anyhow!("mount state file is corrupt; run mfsmount again"))?;
    let prefix = lines.next().unwrap_or("").to_string();
    Ok(MountState {
        image: PathBuf::from(image),
        prefix,
    })
}

pub fn clear() {
    let _ = fs::remove_file(state_path());
}

/// Open the currently mounted volume.
pub fn open_current() -> Result<(Volume, MountState)> {
    let st = load()?;
    let vol = Volume::open(&st.image)
        .map_err(|e| anyhow!("{}: {e}", st.image.display()))?;
    Ok((vol, st))
}
