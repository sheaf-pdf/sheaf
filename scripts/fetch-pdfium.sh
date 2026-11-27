#!/usr/bin/env bash
# Download PDFium's prebuilt library from bblanchon/pdfium-binaries into
# vendor/pdfium/<os>-<arch>/, where sheaf-render binds it.
# Usage: scripts/fetch-pdfium.sh [linux-x64|linux-arm64|win-x64|mac-arm64|mac-x64]
#
# The build is the one pdfium-render's `pdfium_*` feature binds (the
# workspace's Cargo.toml): a binding refuses a library that lacks any
# function of its build's API. crates/sheaf-render/tests/pdfium_build.rs
# holds that feature, this script's default and the VERSION it keeps
# beside the library to one number, so a move of the build edits the
# feature, `default_build` and the digests below together.
#
# The archive is checked against its sha256 before it is unpacked: the
# build is sheaf's benchmark, and an archive replaced under the same tag
# would move pixels unsaid. PDFIUM_BUILD=<n> fetches another build, which
# has no digest here: unverified, and said so, unless PDFIUM_SHA256=<hex>
# gives the digest to hold it to.
#
# Each file is written into the directory under another name and renamed
# over the old one, never copied onto it: a process that has the old
# library loaded (a running sheaf, a test) keeps the file it mapped.
set -euo pipefail

default_build=8125
PDFIUM_BUILD="${PDFIUM_BUILD:-$default_build}"
cd "$(dirname "$0")/.."

case "${1:-}" in
  "")
    case "$(uname -s)-$(uname -m)" in
      Linux-x86_64)  target=linux-x64;  dir=linux-x86_64 ;;
      Linux-aarch64) target=linux-arm64; dir=linux-aarch64 ;;
      Darwin-arm64)  target=mac-arm64;  dir=macos-aarch64 ;;
      Darwin-x86_64) target=mac-x64;    dir=macos-x86_64 ;;
      MINGW64*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64) target=win-x64; dir=windows-x86_64 ;;
      *) echo "unknown host; pass a target explicitly" >&2; exit 1 ;;
    esac ;;
  linux-x64)  target=linux-x64;  dir=linux-x86_64 ;;
  linux-arm64) target=linux-arm64; dir=linux-aarch64 ;;
  win-x64)    target=win-x64;    dir=windows-x86_64 ;;
  mac-arm64)  target=mac-arm64;  dir=macos-aarch64 ;;
  mac-x64)    target=mac-x64;    dir=macos-x86_64 ;;
  *) echo "unknown target $1" >&2; exit 1 ;;
esac

case "$target" in
  linux-*) lib=libpdfium.so;    member=lib/libpdfium.so ;;
  mac-*)   lib=libpdfium.dylib; member=lib/libpdfium.dylib ;;
  win-*)   lib=pdfium.dll;      member=bin/pdfium.dll ;;
esac

# default_build's archives, as its release (chromium/8125) lists them.
case "$target" in
  linux-x64)   pinned=e1c3ce6e270cb5e0c20816b379ad9f2c1309972697c505f508de92bf5e7d6659 ;;
  linux-arm64) pinned=8b8818f2ec57d1b213d952ca6d7f0ffeca9d0110487d0e20ad83ba918bc92efa ;;
  mac-arm64)   pinned=4939bd935ac83ddc09a3a4f038bc43a19cea1660316a3667f26cb07a070e139d ;;
  mac-x64)     pinned=8a5193535b80c7215d8932f6647ae5097465b6dba2811602c21e120c2af112b0 ;;
  win-x64)     pinned=9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b ;;
esac

if [[ -n "${PDFIUM_SHA256:-}" ]]; then
  want=$(printf '%s' "$PDFIUM_SHA256" | tr '[:upper:]' '[:lower:]')
elif [[ "$PDFIUM_BUILD" == "$default_build" ]]; then
  want=$pinned
else
  want=
fi

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  else shasum -a 256 "$1" | awk '{print $1}'; fi
}

url="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F${PDFIUM_BUILD}/pdfium-${target}.tgz"
dest="vendor/pdfium/${dir}"

# The archive is downloaded and unpacked beside $dest, not in it, so a
# fetch that fails leaves $dest as it was.
mkdir -p vendor/pdfium
work="$(mktemp -d vendor/pdfium/.fetch.XXXXXX)"
new=
trap 'rm -rf "$work"; [[ -z "$new" ]] || rm -f "$new"' EXIT

echo "fetching $url"
curl -fsSL "$url" -o "$work/pdfium.tgz"
got=$(sha256 "$work/pdfium.tgz")
if [[ -z "$want" ]]; then
  echo "warning: chromium/${PDFIUM_BUILD} has no digest here: fetched unverified (sha256 $got; PDFIUM_SHA256=<hex> holds it to one)" >&2
elif [[ "$got" != "$want" ]]; then
  echo "pdfium-${target}.tgz: sha256 $got, not $want; nothing installed" >&2
  exit 1
else
  echo "sha256 $got verified"
fi

mkdir -p "$work/x" && tar -xzf "$work/pdfium.tgz" -C "$work/x"
[[ -f "$work/x/$member" ]] || { echo "pdfium-${target}.tgz has no $member; nothing installed" >&2; exit 1; }

# put <file> <name>: write the file into $dest under another name, then
# rename it over $dest/<name>.
put() {
  new=$(mktemp "$dest/.$2.XXXXXX")
  cp "$1" "$new"
  chmod 644 "$new"
  mv -f "$new" "$dest/$2"
  new=
}
mkdir -p "$dest"
# The library is all sheaf needs at run time; LICENSE is kept for
# attribution, and VERSION says which build it is (`just doctor` prints it).
put "$work/x/$member" "$lib"
for f in LICENSE VERSION; do
  if [[ -f "$work/x/$f" ]]; then put "$work/x/$f" "$f"
  else rm -f "$dest/$f"; echo "warning: pdfium-${target}.tgz has no $f" >&2; fi
done
echo "installed chromium/${PDFIUM_BUILD} into $dest:"; ls -l "$dest"
