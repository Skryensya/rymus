# Make it yours: in Config, a fixed color instead of the cover's, the player on top, other borders. Also the stills in docs/.
Output docs/looks.gif
Source tapes/style.tape

Hide
Type "tapes/demo.sh history"
Enter
Sleep 3s
Type "/"
Type "daft punk"
Enter
Sleep 4s
Enter
Sleep 4s
Down 2
Enter
Sleep 8s
Escape
Sleep 500ms
Escape
Sleep 2s
Show

# as it comes, on Home: the color of the cover, the player below, rounded frames
Sleep 2s
Screenshot docs/look-cover.png
# Config, onto Color
Tab@150ms 5
Sleep 1s
Up@100ms 10
Down@300ms 5
Sleep 800ms
# purple, the player on top, heavy borders
Right@700ms 3
Sleep 600ms
Down
Right
Sleep 1s
Down
Right@700ms 2
Sleep 1s
Tab@150ms 2
Sleep 2.5s
Screenshot docs/look-top.png
# white, back below, double lines
Tab@150ms 5
Sleep 800ms
Up@100ms 10
Down@200ms 5
Right@300ms 5
Sleep 600ms
Down
Right
Sleep 600ms
Down
Right
Sleep 1s
Tab@150ms 2
Sleep 2.5s
Screenshot docs/look-double.png

Hide
Type "q"
Sleep 1s
