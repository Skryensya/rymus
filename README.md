# rymus

ymus rewritten in Rust. The original JavaScript version is frozen in the [`js`](https://github.com/Skryensya/ymus/tree/js) branch; both share the database in `~/.ymus`.

Search YouTube Music and play it inside your terminal. No account needed.

- Search artists, albums and songs; queue, likes and dislikes
- Cover art, spectrum visualizer, synced lyrics (LRCLIB)
- Discover: new songs and mixes by style; radio from any song, album or artist
- Listening stats by week, month and year
- Keyboard first, mouse optional

## Install

Needs `mpv`, `yt-dlp` and `ffmpeg` to run. Developed on macOS with Ghostty; other systems are untested.

On a Mac with Apple silicon, download the binary from [Releases](https://github.com/Skryensya/ymus/releases). It is not signed, so macOS may need `xattr -d com.apple.quarantine rymus` before the first run.

To build it yourself you need a Rust toolchain:

```sh
brew install mpv yt-dlp ffmpeg
git clone https://github.com/Skryensya/ymus.git
cd ymus
cargo install --path .
rymus
```

Cover images use the kitty graphics protocol (Ghostty, kitty, WezTerm) and fall back to colored blocks. Icons are Nerd Font glyphs; run `YM_PLAIN=1 rymus` without them.

## Keys

| Key | Action |
|---|---|
| `/` | search |
| `Enter` / `Space` | open, play, or pause the playing song |
| `Tab` | Home, Liked, Queue, Stats |
| `Esc` | back |
| `n` `p` · `Shift ← →` · `+` `-` | next, previous · seek · volume |
| `l` `x` | like, dislike |
| `a` `m` | add to queue, actions menu |
| `D` `R` | discover, radio |
| `y` `v` `w` | lyrics, full player, visualizer |
| `V` | trip: full screen visualizer that follows the song |
| `?` `q` | all keys, quit |

## Your data

Likes, history and settings stay in `~/.ymus/ymus.db`. The services used receive only what a request needs:

- **YouTube Music**: your searches and the songs you play or ask recommendations for
- **LRCLIB**: title, artist, album and length, when you open lyrics
- **MusicBrainz / ListenBrainz**: artist names and ids, for Discover and radio

## Tests

`cargo test`

## Benchmark

`python3 bench/bench.py <command> [<command>…]` compares builds in a pseudo terminal, with audio output off. For example `python3 bench/bench.py ./target/release/rymus`.

## Notes

An unofficial project, not affiliated with or endorsed by YouTube or Google. Inspired by [involvex/youtube-music-cli](https://github.com/involvex/youtube-music-cli).

MIT license.
