# Lyrics that follow the song and scroll by hand, then karaoke: the line being sung in big braille letters.
Output docs/lyrics.gif
Source tapes/style.tape

Hide
Type "tapes/demo.sh"
Enter
Sleep 3s
Type "/"
Type "queen bohemian rhapsody"
Enter
Sleep 4s
# past the albums, onto the first song: Queen's
Down 9
Enter
# wait, hidden, until the singing starts; hidden time is not in the GIF
Sleep 8s
Show

# the words
Type "y"
Sleep 5s
# scroll ahead…
Down@300ms 8
Sleep 2s
# …and they come back to the song
Sleep 4s
# karaoke
Type "Y"
Sleep 14s

Hide
Type "q"
Sleep 1s
