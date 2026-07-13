//! `mfsutils` multitool: `mfsutils <command> [args...]` is equivalent
//! to running the standalone mfs<command> binary.

fn usage() {
    eprintln!(
        "usage: mfsutils <command> [args...]\n\
         \n\
         commands:\n\
         \x20 mount  <image>                      mount an MFS disk image (mfsmount)\n\
         \x20 umount                              unmount (mfsumount)\n\
         \x20 ls     [-l] [pattern ...]           list files (mfsls)\n\
         \x20 cd     [folder]                     change pseudo-folder (mfscd)\n\
         \x20 pwd                                 show current volume/folder (mfspwd)\n\
         \x20 copy   [-m|-b|-t|-r|-a] src... dst  extract files (mfscopy)\n\
         \n\
         copy modes: -m MacBinary II, -b BinHex 4.0, -t text, -r raw data fork,\n\
         \x20           -a automatic (default)"
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = match args.next() {
        Some(c) => c,
        None => {
            usage();
            std::process::exit(1);
        }
    };
    if cmd == "-h" || cmd == "--help" || cmd == "help" {
        usage();
        return;
    }
    let tool = cmd.strip_prefix("mfs").unwrap_or(&cmd).to_string();
    let rest: Vec<String> = args.collect();
    std::process::exit(mfsutils::commands::dispatch(&tool, &rest));
}
