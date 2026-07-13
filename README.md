# mfsutils

hfsutils-style command-line tools for reading Macintosh MFS disk images
— the original flat filesystem used by the Macintosh 128K/512K
(1984-1985). Lists files and extracts them in Mac-compatible formats
with resource forks and Finder metadata intact.

The companion project [`mkfs-mfs`](mkfs-mfs/) *creates* MFS images;
mfsutils reads them.

## Tools

Like hfsutils, `mfsmount` records the current image in a state file
(`~/.mfscwd`, override with `$MFSCWD`) and the other tools operate on it:

| tool | hfsutils equivalent | purpose |
|------|--------------------|---------|
| `mfsmount <image>` | hmount | select an image, print volume info |
| `mfsumount` | humount | forget the current image |
| `mfsls [-l] [pattern ...]` | hls | list files (globs: `*`, `?`) |
| `mfscd [folder]` | hcd | change pseudo-folder (see below) |
| `mfspwd` | hpwd | show current volume/folder |
| `mfscopy [mode] src ... dest` | hcopy | extract files to Unix |

Everything is also available as one multitool: `mfsutils ls -l`, etc.

## mfscopy modes

Same switches as hcopy:

- `-m` — MacBinary II (`.bin`): both forks + all Finder metadata in one
  file; understood by Mini vMac, Basilisk II, CiderPress2, hfsutils, ...
- `-b` — BinHex 4.0 (`.hqx`): 7-bit-safe text encoding of the same.
- `-t` — text (`.txt`): data fork with CR→LF translation.
- `-r` — raw: data fork, byte for byte.
- `-a` — automatic (default): MacBinary II if the file has a resource
  fork, text if its type is `TEXT`, raw otherwise.

## Example

```console
$ mfsmount MacBASIC.335.dsk
Volume name is "Mac BASIC .335"
Volume was created on Thu May 17 16:16:45 1984
Volume contains 52 files
Volume has 41 KB free (41 of 391 allocation blocks)

$ mfsls -l 'BASIC'
f  APPL/DONN     35328         0 Mar 15 1984 18:45 BASIC

$ mfscopy -m BASIC .        # -> ./BASIC.bin (MacBinary II)
$ mfscopy '*' extracted/    # everything, auto mode
```

## Format notes

- Raw images and DiskCopy 4.2 containers are both detected.
- All MFS structures per *Inside Macintosh, Volume II*: MDB at sector 2,
  12-bit allocation map, flat directory, allocation-block chains
  (fragmented files are followed correctly).
- MFS has no real folders; names may contain `:` as the Finder-era
  illusion of paths. `mfscd`/`mfsls` treat `a:b` names as folder `a`
  containing `b`, matching how `mkfs-mfs -d` flattens directories.
- Filenames are decoded from MacRoman; `/`, `%` and control characters
  are percent-encoded when writing to Unix.

## Building

```bash
cargo build --release   # binaries land in target/release/mfs*
cargo test
```

## Status / roadmap

Read-only for now. Candidates for later, in rough order:

- `mfscat` (dump a fork to stdout), `mfsdel`, `mfsrename`
- writing *into* images (`mfscopy` Unix→MFS, MacBinary import) — the
  write-side logic largely exists in `mkfs-mfs`
- `mfsattrib`, `mfsvol`, `mfsformat` (wrapping mkfs-mfs)
