# Local PKGBUILD: builds the mfsutils checkout this file sits in.
# Usage: makepkg -si   (build and install)
#        makepkg -f    (rebuild package only)
pkgname=mfsutils
pkgver=0.1.0
pkgrel=1
pkgdesc="hfsutils-style tools for reading Macintosh MFS disk images"
arch=('x86_64' 'aarch64')
license=('MIT')
depends=('gcc-libs' 'glibc')
makedepends=('cargo')
options=('!lto')
source=()

# NOTE: makepkg's $srcdir is ./src, which collides with the Rust source
# directory, so build into the project's usual target/ instead.
build() {
  cargo build --release --locked \
    --manifest-path "$startdir/Cargo.toml" \
    --target-dir "$startdir/target"
}

check() {
  cargo test --release --locked \
    --manifest-path "$startdir/Cargo.toml" \
    --target-dir "$startdir/target"
}

package() {
  local bin
  for bin in mfsutils mfsmount mfsumount mfsls mfscd mfspwd mfscopy; do
    install -Dm755 "$startdir/target/release/$bin" "$pkgdir/usr/bin/$bin"
  done
  install -Dm644 "$startdir/README.md" "$pkgdir/usr/share/doc/$pkgname/README.md"
}
