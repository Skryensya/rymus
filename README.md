# ymus

Search YouTube Music and play it inside your terminal. No account needed.

- Search artists, albums and songs; queue, likes and dislikes
- Cover art, spectrum visualizer, synced lyrics (LRCLIB)
- Discover: new songs and mixes by style; radio from any song, album or artist
- Listening stats by week, month and year
- Keyboard first, mouse optional

## Install

Needs Node.js 24 or newer, plus `mpv`, `yt-dlp` and `ffmpeg`. Developed on macOS with Ghostty; other systems are untested.

```sh
brew install mpv yt-dlp ffmpeg
git clone https://github.com/Skryensya/ymus.git
cd ymus
npm install
npm link
ymus
```

Cover images use the kitty graphics protocol (Ghostty, kitty, WezTerm) and fall back to colored blocks. Icons are Nerd Font glyphs; run `YM_PLAIN=1 ymus` without them.

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
| `V` · `[ ]` | trip: full screen visualizer · change its mode |
| `?` `q` | all keys, quit |

## Your data

Likes, history and settings stay in `~/.ymus/ymus.db`. The services used receive only what a request needs:

- **YouTube Music**: your searches and the songs you play or ask recommendations for
- **LRCLIB**: title, artist, album and length, when you open lyrics
- **MusicBrainz / ListenBrainz**: artist names and ids, for Discover and radio

## Tests

`npm test`

## Notes

An unofficial project, not affiliated with or endorsed by YouTube or Google. Inspired by [involvex/youtube-music-cli](https://github.com/involvex/youtube-music-cli).

MIT license.
