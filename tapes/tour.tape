# The first impression: search, an artist's page, play a song, the big player.
Output docs/tour.gif
Source tapes/style.tape

Hide
Type "tapes/demo.sh"
Enter
Sleep 3s
Show

Sleep 1.5s
Type "/"
Sleep 300ms
Type "daft punk"
Sleep 500ms
Enter
Sleep 4s
# the first artist found (its picture loads while hidden)
Enter
Sleep 1.5s
Hide
Sleep 4s
Show
Sleep 1.5s
# past "This is …", onto the second popular song
Down@400ms 2
Sleep 800ms
# play it: a moment of loading, then (hidden) until it is playing
Enter
Sleep 2s
Hide
Sleep 9s
Show
Sleep 3s
# the big player
Type "v"
Sleep 5s
Type "v"
Sleep 2s

Hide
Type "q"
Sleep 1s
