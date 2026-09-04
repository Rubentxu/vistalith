#!/usr/bin/env bash
# Environment for building the Tauri desktop shell (apps/desktop/src-tauri).
#
# On machines with webkit2gtk-4.1 devel packages installed system-wide
# (Fedora: `sudo dnf install webkit2gtk4.1-devel`), pkg-config finds
# everything and this script is a no-op. On machines where system packages
# cannot be installed, it falls back to a user-local sysroot extracted from
# the Fedora RPMs (no root needed):
#
#   mkdir -p ~/.local/tauri-sysroot /tmp/tauri-rpms
#   dnf download --resolve --destdir=/tmp/tauri-rpms webkit2gtk4.1-devel
#   for rpm in /tmp/tauri-rpms/*.rpm; do
#     rpm2cpio "$rpm" | cpio -idm -D ~/.local/tauri-sysroot
#   done
#   #-devel symlinks dangle (the matching runtime RPMs are already installed
#   #  and therefore not downloaded): repoint them at the system libraries.
#   for so in $(find ~/.local/tauri-sysroot -type l -name '*.so' -xtype l); do
#     target="/usr/lib64/$(basename "$(readlink "$so")")"
#     [ -e "$target" ] && ln -sf "$target" "$so"
#   done
#
# Usage: source scripts/tauri-env.sh && cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
set -u

if pkg-config --exists webkit2gtk-4.1; then
  echo "tauri-env: system webkit2gtk-4.1 found ($(pkg-config --modversion webkit2gtk-4.1))"
  return 0 2>/dev/null || exit 0
fi

SYSROOT="${VISTALITH_TAURI_SYSROOT:-$HOME/.local/tauri-sysroot}"
if [ -f "$SYSROOT/usr/lib64/pkgconfig/webkit2gtk-4.1.pc" ]; then
  export PKG_CONFIG_PATH="$SYSROOT/usr/lib64/pkgconfig:$SYSROOT/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  export PKG_CONFIG_SYSROOT_DIR="$SYSROOT"
  export PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1
  echo "tauri-env: using user-local sysroot at $SYSROOT ($(pkg-config --modversion webkit2gtk-4.1))"
  return 0 2>/dev/null || exit 0
fi

echo "tauri-env: webkit2gtk-4.1 not found; install webkit2gtk4.1-devel or" >&2
echo "           build the user-local sysroot (see header of this script)." >&2
return 1 2>/dev/null || exit 1
