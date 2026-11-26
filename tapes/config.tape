# Config: sections on the left, ↑ ↓ through them and → into one; crossfade, loudness, mono and the equalizer's window
# in Sound, then the look. Each change is kept for next time.
Output docs/config.gif
Source tapes/style.tape

Hide
Type "tapes/demo.sh"
Enter
Sleep 3s
Show

Sleep 1s
# to Config: the cursor is on the sections, each shown as it is reached
Tab@300ms 7
Sleep 1.5s
Down@900ms 4
Sleep 1s
Up@400ms 4
Sleep 800ms
# into Sound: crossfade 4 s
Right
Sleep 1s
Right@700ms 2
Sleep 1.5s
# even out loudness: a switch
Down
Enter
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
Escape
Sleep 800ms
# back to the sections, then Look: a fixed color
Escape
Sleep 600ms
Down
Right
Sleep 600ms
Down
Right@500ms 7
Sleep 2s

Hide
Type "q"
Sleep 1s
