#!/usr/bin/env bash
# Runs a command (cargo test, usually) with every test binary it starts
# under a memory cap: a runaway test once took the whole machine's memory.
# On Linux with a user systemd, each binary runs in its own scope with
# MemoryMax $SHEAF_TEST_MEMORY_MAX (48G) and no swap. Elsewhere, or with
# SHEAF_UNCAPPED=1, the command runs as it is.
set -euo pipefail
if [ -z "${SHEAF_UNCAPPED:-}" ] && [ "$(uname -s)" = Linux ] \
    && command -v systemd-run >/dev/null \
    && systemctl --user show-environment >/dev/null 2>&1; then
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="systemd-run --user --scope --quiet -p MemoryMax=${SHEAF_TEST_MEMORY_MAX:-48G} -p MemorySwapMax=0 --"
fi
exec "$@"
