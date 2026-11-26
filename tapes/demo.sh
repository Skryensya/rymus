#!/bin/sh
# Starts rymus for a recording. It never touches your own data: a fresh database in a temporary folder, no sound,
# and plain icons (the font VHS draws with has no Nerd Font glyphs).
#   tapes/demo.sh            an empty rymus, as on first run
#   tapes/demo.sh history    with a month of made-up listening (tapes/history.sql), for Home and Stats
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dir=$(mktemp -d)
export YM_DB="$dir/demo.db" YM_MPV_ARGS="--ao=null" YM_PLAIN=1
unset TERM_PROGRAM KITTY_WINDOW_ID 2>/dev/null || true # covers are drawn with coloured blocks, which VHS can record
if [ "${1:-}" = history ]; then
    sqlite3 "$YM_DB" < "$here/history.sql"
fi
exec "${RYMUS:-$here/../target/release/rymus}"
