#!/usr/bin/env bash
#
# Build DJI Mic Control for release with all path-privacy safeties enabled.
#
# Builds the shippable bundles for the host OS and copies only those into
# Release/<os>/ :
#     Linux  ->  Release/linux    (.deb, .rpm, .AppImage)
# For Windows, use build-release.ps1. macOS is not a supported target.
#
# "Safeties on" means every absolute filesystem path Rust would otherwise bake
# into panic/backtrace strings (which `strip` does NOT remove) is remapped away,
# so a crash can never leak the build machine's home directory, username, or
# hostname. The build then re-scans the compiled binary and FAILS if anything
# slipped through.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# --- Path-privacy safety -----------------------------------------------------
# Strip the home dir and Cargo registry from embedded paths. We use
# CARGO_ENCODED_RUSTFLAGS (0x1f-separated) rather than RUSTFLAGS so that home
# paths containing spaces are handled correctly. The registry remap has an
# empty replacement; the home remap maps to "/". Covering CARGO_HOME as well
# handles CI hosts that relocate it outside $HOME.
unset RUSTFLAGS 2>/dev/null || true   # avoid "both RUSTFLAGS set" cargo error
CARGO_HOME_DIR="${CARGO_HOME:-$HOME/.cargo}"
US=$'\x1f'
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=${CARGO_HOME_DIR}/=${US}--remap-path-prefix=${HOME}/=/"

# --- Host platform -----------------------------------------------------------
OS="$(uname -s)"
case "$OS" in
  Linux)  PLATFORM="linux"; BUNDLES="deb,rpm,appimage"
          # Let AppImage tooling run by extraction (no FUSE needed on CI hosts).
          export APPIMAGE_EXTRACT_AND_RUN=1
          # linuxdeploy's bundled `strip` (binutils 2.35) can't parse the
          # `.relr.dyn` section modern distro libs ship with, and aborts the
          # whole build. Our binary is already stripped by Cargo's
          # profile.release (strip = true) and the vendored system libs ship
          # pre-stripped, so this second strip pass is redundant anyway.
          export NO_STRIP=1 ;;
  Darwin) echo "ERROR: macOS is not a supported build target in this fork." >&2; exit 1 ;;
  *) echo "ERROR: unsupported OS '$OS' — use build-release.ps1 on Windows." >&2; exit 1 ;;
esac

command -v cargo >/dev/null 2>&1 || { echo "ERROR: cargo not found in PATH." >&2; exit 1; }
command -v npm   >/dev/null 2>&1 || { echo "ERROR: npm not found in PATH." >&2; exit 1; }

OUT="$SCRIPT_DIR/Release/$PLATFORM"
BUNDLE_DIR="$SCRIPT_DIR/target/release/bundle"

echo "==> Building $PLATFORM release (bundles: $BUNDLES)"

# --- Build (Tauri runs the frontend `npm run build` itself) ------------------
(
  cd gui
  npm install --no-audit --no-fund
  npx tauri build --bundles "$BUNDLES"
)

# --- Collect artifacts -------------------------------------------------------
rm -rf "$OUT"
mkdir -p "$OUT"

copy_glob() { # <dir> <glob>
  local dir="$1" pat="$2" found=0 f
  shopt -s nullglob
  for f in "$dir"/$pat; do
    cp -R "$f" "$OUT/"
    echo "    + $(basename "$f")"
    found=1
  done
  shopt -u nullglob
  if [ "$found" -ne 1 ]; then
    echo "ERROR: no artifact matching '$pat' in $dir" >&2
    exit 1
  fi
}

copy_glob "$BUNDLE_DIR/deb"      "*.deb"
copy_glob "$BUNDLE_DIR/rpm"      "*.rpm"
copy_glob "$BUNDLE_DIR/appimage" "*.AppImage"

# --- Verify no build-machine paths leaked into any shipped file --------------
# Scans our own binary plus (on Linux) every file linuxdeploy bundled into the
# AppImage's AppDir, since NO_STRIP=1 above means those files keep whatever
# they shipped with rather than passing through a second strip.
BIN="$SCRIPT_DIR/target/release/djimic-gui"
SCAN_FILES=()
[ -f "$BIN" ] && SCAN_FILES+=("$BIN")
if [ "$PLATFORM" = "linux" ]; then
  APPDIR="$(ls -d "$BUNDLE_DIR"/appimage/*.AppDir 2>/dev/null | head -1)"
  if [ -n "$APPDIR" ]; then
    while IFS= read -r -d '' f; do SCAN_FILES+=("$f"); done < <(find "$APPDIR" -type f -print0)
  fi
fi

if [ "${#SCAN_FILES[@]}" -gt 0 ] && command -v strings >/dev/null 2>&1; then
  HOST="$(hostname -s 2>/dev/null || hostname 2>/dev/null || true)"
  USER_NAME="$(id -un 2>/dev/null || whoami 2>/dev/null || true)"
  LEAK_REPORT=""
  for f in "${SCAN_FILES[@]}"; do
    # Grep only the file's own string content (not `strings -f`'s filename
    # prefix) — the AppDir path itself lives under $HOME and would otherwise
    # false-positive against the very patterns we're checking for.
    m="$(strings -a "$f" 2>/dev/null \
          | grep -aF -e "$HOME/" -e "$CARGO_HOME_DIR/" \
                 ${HOST:+-e "$HOST"} ${USER_NAME:+-e "$USER_NAME"} \
          | sort -u || true)"
    [ -n "$m" ] && LEAK_REPORT+=$'\n'"-- $f --"$'\n'"$m"$'\n'
  done
  if [ -n "$LEAK_REPORT" ]; then
    echo "SECURITY: build-machine paths leaked into a shipped file:" >&2
    printf '%s\n' "$LEAK_REPORT" | head -50 >&2
    exit 1
  fi
  echo "==> Leak scan clean: no home path, Cargo path, username, or hostname in ${#SCAN_FILES[@]} scanned file(s)."
else
  echo "WARN: skipped leak scan ('strings' unavailable or binary missing)." >&2
fi

# --- Verify logo is embedded, and (Linux) that the udev rule + reload script --
# --- ship in BOTH the .deb and .rpm, so the two never drift apart ------------
UDEV_RULE="60-dji-mic.rules"

# List a package's payload paths, using whatever tooling the host has.
deb_contents() {
  if command -v dpkg-deb >/dev/null 2>&1; then
    dpkg-deb -c "$1" | awk '{print $NF}'
  elif command -v ar >/dev/null 2>&1; then
    ar p "$1" "$(ar t "$1" | grep -m1 '^data\.tar')" | tar tf - 2>/dev/null
  fi
}
rpm_contents() {
  if command -v rpm >/dev/null 2>&1; then
    rpm -qlp "$1" 2>/dev/null
  elif command -v rpm2cpio >/dev/null 2>&1 && command -v cpio >/dev/null 2>&1; then
    rpm2cpio "$1" | cpio -t 2>/dev/null
  fi
}

DEB="$(ls "$OUT"/*.deb 2>/dev/null | head -1)"
RPM="$(ls "$OUT"/*.rpm 2>/dev/null | head -1)"
DEB_LIST="$(deb_contents "$DEB")"
RPM_LIST="$(rpm_contents "$RPM")"

assert_in() { # <label> <listing> <needle> <what>
  local label="$1" listing="$2" needle="$3" what="$4"
  if [ -z "$listing" ]; then
    echo "WARN: cannot inspect $label (no tool); skipped $what check." >&2
    return 0
  fi
  if printf '%s\n' "$listing" | grep -q "$needle"; then
    echo "==> $label includes $what"
  else
    echo "ERROR: $label is missing $what ($needle)" >&2
    exit 1
  fi
}

# udev rule parity — the whole point: present in BOTH packages.
assert_in ".deb" "$DEB_LIST" "$UDEV_RULE"      "the udev rule"
assert_in ".rpm" "$RPM_LIST" "$UDEV_RULE"      "the udev rule"
# Same logo shipped by each Linux bundle (hicolor PNG).
assert_in ".deb" "$DEB_LIST" "hicolor/128x128" "the app logo"
assert_in ".rpm" "$RPM_LIST" "hicolor/128x128" "the app logo"

# The udev-reload maintainer script must ship in the .deb too (rpm scriptlets
# are embedded in the header and always travel with it).
if command -v dpkg-deb >/dev/null 2>&1 && [ -n "$DEB" ]; then
  if dpkg-deb --ctrl-tarfile "$DEB" 2>/dev/null | tar t 2>/dev/null | grep -q 'postinst'; then
    echo "==> .deb ships a postinst (udev-reload) script"
  else
    echo "ERROR: .deb has no postinst script — the udev reload would not run" >&2
    exit 1
  fi
fi

echo "==> Done. Artifacts in Release/$PLATFORM:"
ls -1 "$OUT"
