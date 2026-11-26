#!/bin/sh
# Starts rymus for a recording. It never touches your own data: a fresh database in a temporary folder, no sound,
# and plain icons (the font VHS draws with has no Nerd Font glyphs).
#   tapes/demo.sh            an empty rymus, as on first run
#   tapes/demo.sh history    with a month of made-up listening (tapes/history.sql), for Home and Stats
#   DEMO_SET="mode=light"    settings it starts with
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dir=$(mktemp -d)
export YM_DB="$dir/demo.db" YM_MPV_ARGS="--ao=null" YM_PLAIN=1
unset TERM_PROGRAM KITTY_WINDOW_ID 2>/dev/null || true # covers are drawn with coloured blocks, which VHS can record
if [ "${1:-}" = history ]; then
    sqlite3 "$YM_DB" < "$here/history.sql"
fi
# settings to start with, e.g. DEMO_SET="mode=light accent=teal"
for kv in ${DEMO_SET:-}; do
    sqlite3 "$YM_DB" "CREATE TABLE IF NOT EXISTS settings(k TEXT PRIMARY KEY, v TEXT); INSERT OR REPLACE INTO settings VALUES('${kv%%=*}', '${kv#*=}');"
done
exec "${RYMUS:-$here/../target/release/rymus}"
