//! The mfs* command set: mount, umount, pwd, cd, ls, copy.

use crate::mfs::{FileEntry, Fork, Volume};
use crate::mount::{self, MountState};
use crate::{binhex, macbinary, util};
use anyhow::{anyhow, bail, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub fn dispatch(tool: &str, args: &[String]) -> i32 {
    // Die quietly on SIGPIPE like other Unix CLI tools (e.g. under
    // `mfsls | head`) instead of panicking on a stdout write error.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let result = match tool {
        "mount" => cmd_mount(args),
        "umount" => cmd_umount(args),
        "pwd" => cmd_pwd(args),
        "cd" => cmd_cd(args),
        "ls" | "dir" => cmd_ls(args),
        "copy" => cmd_copy(args),
        _ => Err(anyhow!("unknown command \"{tool}\"")),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("mfs{tool}: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------- mount

fn cmd_mount(args: &[String]) -> Result<()> {
    if args.len() != 1 {
        bail!("usage: mfsmount image-path");
    }
    let path = fs::canonicalize(&args[0])
        .map_err(|e| anyhow!("{}: {e}", args[0]))?;
    let vol = Volume::open(&path)?;
    mount::save(&MountState {
        image: path,
        prefix: String::new(),
    })?;
    let i = &vol.info;
    println!("Volume name is \"{}\"", i.name);
    println!("Volume was created on {}", util::mac_date_long(i.create_date));
    if i.backup_date != 0 {
        println!(
            "Volume was last backed up on {}",
            util::mac_date_long(i.backup_date)
        );
    }
    println!("Volume contains {} files", i.num_files);
    println!(
        "Volume has {} KB free ({} of {} allocation blocks)",
        (i.free_blks as u64 * i.alloc_blk_size as u64) / 1024,
        i.free_blks,
        i.num_alloc_blks
    );
    Ok(())
}

fn cmd_umount(_args: &[String]) -> Result<()> {
    mount::clear();
    Ok(())
}

// ------------------------------------------------------------- pwd / cd

fn cmd_pwd(_args: &[String]) -> Result<()> {
    let (vol, st) = mount::open_current()?;
    if st.prefix.is_empty() {
        println!("{}:", vol.info.name);
    } else {
        println!("{}:{}:", vol.info.name, st.prefix);
    }
    Ok(())
}

fn cmd_cd(args: &[String]) -> Result<()> {
    if args.len() > 1 {
        bail!("usage: mfscd [mfs-path]");
    }
    let (vol, mut st) = mount::open_current()?;
    let new_prefix = match args.first() {
        None => String::new(),
        Some(arg) => resolve_prefix(&vol, &st.prefix, arg)?,
    };
    st.prefix = new_prefix;
    mount::save(&st)
}

/// Turn a user-supplied path into a validated folder prefix.
/// Accepted forms: "" or ":" (root), "Vol:sub", ":sub", "sub",
/// "sub:deeper", ".." (up one level).
fn resolve_prefix(vol: &Volume, cur: &str, arg: &str) -> Result<String> {
    let mut p = arg.trim_end_matches(':').to_string();
    if p == ".." {
        return Ok(match cur.rfind(':') {
            Some(i) => cur[..i].to_string(),
            None => String::new(),
        });
    }
    let volpfx = format!("{}:", vol.info.name);
    let absolute = if util::starts_with_ci(&p, &volpfx) {
        p = p[volpfx.len()..].to_string();
        true
    } else if let Some(stripped) = p.strip_prefix(':') {
        p = stripped.to_string();
        true
    } else {
        false
    };
    if p.is_empty() {
        return Ok(String::new());
    }
    let full = if absolute || cur.is_empty() {
        p
    } else {
        format!("{cur}:{p}")
    };
    let needle = format!("{full}:");
    if !vol.files.iter().any(|f| util::starts_with_ci(&f.name, &needle)) {
        bail!("\"{full}\": no such folder");
    }
    // Normalize case to how it appears on disk.
    let canon = vol
        .files
        .iter()
        .find(|f| util::starts_with_ci(&f.name, &needle))
        .map(|f| f.name[..full.len()].to_string())
        .unwrap();
    Ok(canon)
}

// -------------------------------------------------------------------- ls

enum Item<'a> {
    File(&'a FileEntry, String),
    Folder(String, u32),
}

/// Entries visible at the given prefix level: real files plus one
/// pseudo-folder per distinct next path component.
fn level_items<'a>(vol: &'a Volume, prefix: &str) -> Vec<Item<'a>> {
    let pfx = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}:")
    };
    let mut folders: BTreeMap<String, (String, u32)> = BTreeMap::new();
    let mut items = Vec::new();
    for f in &vol.files {
        if !util::starts_with_ci(&f.name, &pfx) {
            continue;
        }
        let rest = &f.name[pfx.len()..];
        match rest.find(':') {
            None => items.push(Item::File(f, rest.to_string())),
            Some(i) => {
                let comp = &rest[..i];
                let e = folders
                    .entry(comp.to_lowercase())
                    .or_insert_with(|| (comp.to_string(), 0));
                e.1 += 1;
            }
        }
    }
    for (_, (name, count)) in folders {
        items.push(Item::Folder(name, count));
    }
    items
}

fn item_name(it: &Item) -> String {
    match it {
        Item::File(_, n) => n.clone(),
        Item::Folder(n, _) => n.clone(),
    }
}

fn cmd_ls(args: &[String]) -> Result<()> {
    let mut long = false;
    let mut patterns: Vec<&String> = Vec::new();
    for a in args {
        match a.as_str() {
            "-l" => long = true,
            s if s.starts_with('-') => bail!("usage: mfsls [-l] [pattern ...]"),
            _ => patterns.push(a),
        }
    }
    let (vol, st) = mount::open_current()?;
    let mut items = level_items(&vol, &st.prefix);
    if !patterns.is_empty() {
        items.retain(|it| {
            let n = item_name(it);
            patterns
                .iter()
                .any(|p| util::glob_match_ci(p, &n) || util::glob_match_ci(p, &format!("{n}:")))
        });
    }
    items.sort_by_key(|it| item_name(it).to_lowercase());

    for it in &items {
        match it {
            Item::File(f, name) => {
                if long {
                    println!(
                        "{}  {}/{} {:>9} {:>9} {} {}",
                        if f.locked { "F" } else { "f" },
                        util::fourcc(&f.type_code),
                        util::fourcc(&f.creator),
                        f.rsrc_len,
                        f.data_len,
                        util::mac_date_short(f.mod_date),
                        name
                    );
                } else {
                    println!("{name}");
                }
            }
            Item::Folder(name, count) => {
                if long {
                    println!(
                        "d            {:>9} {:>9} {:17} {}:",
                        count, "", "", name
                    );
                } else {
                    println!("{name}:");
                }
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ copy

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    MacBinary,
    BinHex,
    Text,
    Raw,
    Auto,
}

impl Mode {
    fn extension(self) -> &'static str {
        match self {
            Mode::MacBinary => ".bin",
            Mode::BinHex => ".hqx",
            Mode::Text => ".txt",
            Mode::Raw | Mode::Auto => "",
        }
    }
}

fn cmd_copy(args: &[String]) -> Result<()> {
    let mut mode = Mode::Auto;
    let mut paths: Vec<&String> = Vec::new();
    for a in args {
        match a.as_str() {
            "-m" => mode = Mode::MacBinary,
            "-b" => mode = Mode::BinHex,
            "-t" => mode = Mode::Text,
            "-r" => mode = Mode::Raw,
            "-a" => mode = Mode::Auto,
            s if s.starts_with('-') && s.len() > 1 => {
                bail!("unknown option \"{s}\"\nusage: mfscopy [-m|-b|-t|-r|-a] mfs-path ... unix-dest")
            }
            _ => paths.push(a),
        }
    }
    if paths.len() < 2 {
        bail!("usage: mfscopy [-m|-b|-t|-r|-a] mfs-path ... unix-dest");
    }
    let dest = paths.pop().unwrap();
    let (vol, st) = mount::open_current()?;

    if dest.contains(':') {
        bail!(
            "destination \"{dest}\" looks like an MFS path; \
             copying into an image is not implemented yet (mkfs-mfs can build populated images)"
        );
    }

    // Expand source patterns against the volume.
    let mut selected: Vec<&FileEntry> = Vec::new();
    for pat in &paths {
        let matches = match_files(&vol, &st.prefix, pat);
        if matches.is_empty() {
            bail!("\"{pat}\": no such file");
        }
        for m in matches {
            if !selected.iter().any(|s| s.file_num == m.file_num) {
                selected.push(m);
            }
        }
    }

    let dest_path = Path::new(dest);
    let dest_is_dir = dest_path.is_dir();
    if selected.len() > 1 && !dest_is_dir {
        bail!("\"{dest}\" is not a directory (needed to copy {} files)", selected.len());
    }

    for f in selected {
        let out_path = if dest_is_dir {
            output_name(dest_path, f, mode)
        } else {
            dest_path.to_path_buf()
        };
        extract(&vol, f, mode, &out_path)?;
    }
    Ok(())
}

/// Match a user pattern (globs allowed) against full on-disk names.
/// Patterns are relative to the current prefix; a leading ':' or
/// leading volume name makes them relative to the root.
fn match_files<'a>(vol: &'a Volume, prefix: &str, pattern: &str) -> Vec<&'a FileEntry> {
    let mut p = pattern.to_string();
    let volpfx = format!("{}:", vol.info.name);
    let absolute = if util::starts_with_ci(&p, &volpfx) {
        p = p[volpfx.len()..].to_string();
        true
    } else if let Some(stripped) = p.strip_prefix(':') {
        p = stripped.to_string();
        true
    } else {
        false
    };
    let full = if absolute || prefix.is_empty() {
        p
    } else {
        format!("{prefix}:{p}")
    };
    vol.files
        .iter()
        .filter(|f| util::glob_match_ci(&full, &f.name))
        .collect()
}

fn output_name(dir: &Path, f: &FileEntry, mode: Mode) -> PathBuf {
    let base = f.name.rsplit(':').next().unwrap_or(&f.name);
    let mode = effective_mode(f, mode);
    let name = format!("{}{}", util::sanitize_unix_name(base), mode.extension());
    dir.join(name)
}

fn effective_mode(f: &FileEntry, mode: Mode) -> Mode {
    if mode != Mode::Auto {
        return mode;
    }
    if f.rsrc_len > 0 {
        Mode::MacBinary
    } else if &f.type_code == b"TEXT" {
        Mode::Text
    } else {
        Mode::Raw
    }
}

fn extract(vol: &Volume, f: &FileEntry, mode: Mode, out_path: &Path) -> Result<()> {
    let mode = effective_mode(f, mode);
    let bytes = match mode {
        Mode::MacBinary => {
            let data = vol.read_fork(f, Fork::Data)?;
            let rsrc = vol.read_fork(f, Fork::Rsrc)?;
            macbinary::encode(f, &data, &rsrc)
        }
        Mode::BinHex => {
            let data = vol.read_fork(f, Fork::Data)?;
            let rsrc = vol.read_fork(f, Fork::Rsrc)?;
            binhex::encode(f, &data, &rsrc)
        }
        Mode::Text => {
            let mut data = vol.read_fork(f, Fork::Data)?;
            for b in data.iter_mut() {
                if *b == b'\r' {
                    *b = b'\n';
                }
            }
            data
        }
        Mode::Raw => vol.read_fork(f, Fork::Data)?,
        Mode::Auto => unreachable!(),
    };
    fs::write(out_path, &bytes)
        .map_err(|e| anyhow!("cannot write {}: {e}", out_path.display()))?;
    Ok(())
}
