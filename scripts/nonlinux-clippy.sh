#!/usr/bin/env bash
# Clippy sheaf-app's tests as macOS and Windows build them. sheaf-app's
# GPUI tests are Linux-only (cfg(target_os = "linux")), so an import or a
# helper only they use is dead code there, which clippy -D warnings fails
# on CI's macOS and Windows runners. This switches those gates off in the
# test code, runs clippy, and restores every file it touched from its own
# copy, checked with cmp (never git checkout: the tree may hold work in
# progress). Don't run it while anything else edits sheaf-app.
set -u
cd "$(git -C "$(dirname "$0")" rev-parse --show-toplevel)" || exit 2
mkdir -p target
backup=$(mktemp -d target/nonlinux.XXXX)
files=$(grep -rlE 'cfg\((all\(test, )?target_os = "linux"\)?\)' crates/sheaf-app/src || true)
for f in $files; do
    mkdir -p "$backup/$(dirname "$f")"
    cp -p "$f" "$backup/$f"
done
restore() {
    for f in $files; do
        cp -p "$backup/$f" "$f"
        cmp -s "$backup/$f" "$f" || { echo "RESTORE FAILED: $f (copy in $backup)"; exit 3; }
    done
    rm -rf "$backup"
}
trap restore EXIT
for f in $files; do
    # Linux-only test helpers anywhere in the file.
    sed -i -e 's/#\[cfg(all(test, target_os = "linux"))\]/#[cfg(all(test, any()))]/' "$f"
    # Linux-only items inside a test module, whatever it's called or
    # whoever may see it; file_dialog.rs's Linux gates are platform code.
    [ "$f" = crates/sheaf-app/src/file_dialog.rs ] && continue
    python3 - "$f" <<'PY'
import re, sys
p = sys.argv[1]
s = open(p).read()
m = re.search(r'#\[cfg\(test\)\]\n(pub(\([a-z]+\))? )?mod \w+ \{', s)
if m:
    i = m.start()
    s = s[:i] + s[i:].replace('#[cfg(target_os = "linux")]', '#[cfg(any())]')
open(p, 'w').write(s)
PY
done
cargo clippy -p sheaf-app --all-targets -- -D warnings
