# Maintainer: werdxz

pkgname=portty
pkgver=0.1.0
pkgrel=1
pkgdesc="XDG Desktop Portal backend for TTY environments"
arch=('x86_64')
url="https://github.com/werdxz/portty"
license=('MIT')
depends=('xdg-desktop-portal')
makedepends=('cargo')
source=()

build() {
    cd "$srcdir/.."
    cargo build --release --locked
}

package() {
    cd "$srcdir/.."

    # Install daemon
    install -Dm755 "target/release/porttyd" "$pkgdir/usr/lib/portty/porttyd"

    # Install builtin
    install -Dm755 "target/release/portty-builtin" "$pkgdir/usr/lib/portty/portty-builtin"

    # Install portal file
    install -Dm644 "misc/tty.portal" "$pkgdir/usr/share/xdg-desktop-portal/portals/tty.portal"

    # Install systemd service
    install -Dm644 "misc/portty.service" "$pkgdir/usr/lib/systemd/user/portty.service"

    # Install license
    install -Dm644 "LICENSE" "$pkgdir/usr/share/licenses/$pkgname/LICENSE"

    # Install example config
    install -Dm644 "misc/config.toml.example" "$pkgdir/usr/share/doc/$pkgname/config.toml.example"
}
