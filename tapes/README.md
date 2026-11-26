# Recordings

The GIFs in the README are made from these scripts with [VHS](https://github.com/charmbracelet/vhs).

```sh
brew install vhs            # also installs ttyd and ffmpeg
cargo build --release
vhs tapes/tour.tape         # writes docs/tour.gif; one tape per GIF
for t in tapes/*.tape; do [ "$t" = tapes/style.tape ] || vhs "$t"; done   # all of them
```

- `style.tape` is the look they share (size, theme, font).
- `demo.sh` starts rymus on a fresh database in a temporary folder, with no sound, so a recording never shows
  your own likes or history. `demo.sh history` adds a made-up month of listening (`history.sql`).
- The searches are real, so YouTube Music's answers can change; if a tape lands on the wrong row, adjust its
  `Down` count.
- `light.tape` saves the light-mode stills `docs/look-light*.png`, on a light terminal theme. `DEMO_SET="mode=light accent=teal"` before `tapes/demo.sh` starts with those settings.
- `looks.tape` also saves the three stills `docs/look-*.png` (VHS `Screenshot`).
- `trip.tape` is not in the README yet: the visualizer is being rebuilt. Record it once it is done.
- VHS cannot show real images, so covers appear as coloured blocks, the same as "Pixel art covers" in Config.
