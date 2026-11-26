# Prompt: the new Trip (full screen visualizer, key `V`)

You are working on **rymus**, a YouTube Music player for the terminal written in Rust (`src/`). Rebuild the full screen
visualizer that `V` opens (today `src/trip.rs`, drawn from `src/render/`) into a **set of scenes in the spirit of the
Windows Media Player visualizations** that people remember fondly, with rymus' own touch. It must feel unique, fun and
clearly driven by the song that plays, but never chaotic.

Read `README.md` ("How it works", "Where things are", "Making changes", "Tests") and `src/trip.rs`, `src/viz.rs`,
`src/theme.rs`, `src/cover.rs` (palette) and the trip parts of `src/app/` and `src/render/` before writing code.

## What we are evoking

The Windows Media Player visualizations (versions 7 to 12) that people remember:

- **Battery** (WMP 8): hypnotic feedback, where each frame is the last one zoomed, turned and faded a little, with new
  shapes drawn on top. The result is tunnels, spirals and trails that seem endless. It had many presets with strange,
  poetic names, shown briefly when they changed. This is the heart of the nostalgia.
- **Alchemy** (WMP 9): geometric shapes and patterns that slowly turn into one another, with a feeling of mystery.
  WMP changed presets by itself.
- **Ambience**: soft swirls and clouds of color that warp and flow. Very calm.
- **Bars and Waves** (WMP 7): the classic spectrum bars and the oscilloscope line. Plain and readable.

What made them loved, and what to keep:

1. They were hypnotic rather than busy: continuous movement, with the music shaping it.
2. They were full of surprises: many scenes, each with its own character.
3. They had names you remembered.
4. You did not have to manage them: they changed by themselves, but you could take control.

## The experience the user asked for

- **All four families**: Battery, Alchemy, Ambience and Bars and Waves. Each family has several scenes; aim for about 12
  scenes in total, at least 2 per family.
- **How it changes (all of these)**:
  - Each song picks its opening scene from its id, so the same song always opens the same way. Different songs should
    usually open differently.
  - During a song the scene changes on strong moments (a drop, a big rise in energy after a quiet part), with a smooth
    transition of about 1.5 to 2 s. It changes at most about every 30 s, and never in the first 15 s of a song.
  - Keys to take control: `]` next scene, `[` previous scene, `.` locks the current scene (no automatic changes) or
    unlocks it. Show the lock in the title of the frame.
- **Drawing technique: whatever suits each scene.**
  - Half blocks `▀`, with 2 colored pixels per cell, for feedback, trails, clouds and gradients.
  - Braille, with 8 dots per cell, for fine lines: the oscilloscope and thin geometry.
  - ASCII for scenes whose charm is retro.
  - A scene may mix techniques. Every row must still be exactly the width of the screen.
- **Intensity: hypnotic and fluid.** Movement is continuous and smooth. Beats show as pulses (a zoom kick, a brightness
  swell, a ripple) and short glows that last under 300 ms, never hard flicker.
  - Photosensitivity rule: never more than 3 full screen brightness flashes per second, and never full white flashes.
  - Quiet songs look calm; loud ones look fuller, but still fluid.
- **Color**: by default the palette of the cover (`cover::palette`, up to 4 colors dark to light, already used by the
  Trip). Some scenes may bring their own classic palette, the way WMP presets did. When the song changes, the colors
  move to the new palette gradually and do not jump.
- **Scene names: strange and poetic, in the spirit of Battery's presets.** They are in English, invented by us, and
  not copied from WMP. Examples of the tone: "my tornado is resting", "the lake remembers", "spider in the bathtub",
  "a quiet engine". When a scene starts, its name fades in at the bottom left, stays about 3 s and fades out.
- **On top of the image**:
  - **Title and artist**: discreet, when the song starts; they fade out after about 4 s.
  - **Scene name**: as above.
  - **Synced lyrics**: the current line, soft and centered near the bottom. They are **off by default**; `y` toggles
    them while the Trip is open. That choice is remembered in the settings table. Lyrics come from the existing LRCLIB
    code (`src/lyrics.rs`); without synced lyrics, show nothing.
  - Overlays must stay readable on any scene: draw them on a darkened band, not straight over bright pixels.
- **Budget: up to about 10 % CPU** for the app in a large terminal (for example 200×60) at about 25 frames per second,
  measured with `bench/bench.py`. Today the Trip uses about 5–6 %. Only changed lines are sent to the terminal, so
  scenes that hold still are cheap.

## What drives the scenes (already measured in the app)

- 32 bands of the spectrum in sync with what is heard (`viz::Feed`, `app.bands`, 0..1 per band, already scaled per
  band).
- The real loudness of the music (`viz::energy`, `app.energy`), which is low for soft songs.
- The playback clock (`trip_t`), which stops when paused.
- What the current Trip already derives: tempo from bass hits, and a "look" per song id (`trip::look`).
- Build on these. Add a small, smoothed set of song features that every scene reads: bass, mids, highs, energy, a beat
  pulse that decays, tempo, and a slow "section" value. That keeps scenes consistent and keeps them from reacting to
  noise.

## Rules of this codebase (do not break them)

- **Never add or remove an audio filter while music plays** (`af add` or `af remove` on an existing one): the user hears
  a gap. Everything here is drawing only; do not touch the audio filters.
- Every screen row is exactly the terminal width; rows are joined with `\r\n` (raw mode).
- The visualizer must not flicker when paused: the picture holds still and nothing new is written.
- Smallest complete change; no new dependencies; comment only the why; shortcuts get a `shortcut:` comment.
- Other Claude sessions may be editing this folder: re-read a file just before editing it, and do not commit unless the
  user asks.
- Keyboard first. `Space` is play/pause everywhere, `Esc` or `V` closes the Trip, and `/` (search) takes over the screen.

## Done means

1. About 12 scenes across the four families, each visibly driven by the song, none chaotic.
2. Automatic scene changes on strong moments with smooth transitions; `[` `]` `.` work, and the lock shows.
3. Overlays as described; lyrics toggle with `y`, off by default, and remembered.
4. Tests:
   - Every scene fills the screen exactly at several sizes, with silence and with loud input.
   - The same song opens the same scene.
   - Scene changes respect the 15 s and 30 s rules.
   - The flash limit holds: count full screen brightness jumps over a simulated loud song.
   - A paused picture holds still.
   - Keys and the lyrics toggle, through `src/flows.rs`.
5. CPU measured silently with `bench/bench.py`, staying under about 10 %; report the number.
6. `README.md` and the `?` page list the new keys.
7. Show a few frames of each family to the user (rendered silently) before calling it done, and say what was not seen
   in a real terminal.
