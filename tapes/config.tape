# Config: the language first, then crossfade, loudness, mono and the equalizer's window; each kept for next time.
Output docs/config.gif
Source tapes/style.tape

Hide
Type "tapes/demo.sh"
Enter
Sleep 3s
Show

Sleep 1s
# to Config
Tab@300ms 7
Sleep 2s
# the language: Spanish, and back
Right
Sleep 2.5s
Left
Sleep 1.5s
# crossfade 4 s
Down
Right@700ms 2
Sleep 1.5s
# even out loudness: on
Down
Right
Sleep 1s
# mono
Down
Right
Sleep 1s
# the equalizer, a window of its own
Down
Sleep 800ms
Enter
Sleep 1.5s
Down@700ms 5
Sleep 1s
# onto its sliders
Enter
Right@400ms 3
Sleep 2s
Escape
Sleep 500ms
Escape
Sleep 2s

Hide
Type "q"
Sleep 1s
