# Light mode, for a terminal with a light background: the stills docs/look-light*.png.
Output docs/look-light.gif
Source tapes/style.tape
Set Theme "Catppuccin Latte"

Hide
Type 'DEMO_SET="mode=light" tapes/demo.sh history'
Enter
Sleep 3s
Type "/"
Type "daft punk"
Enter
Sleep 4s
Left
Sleep 500ms
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
Sleep 1s
Screenshot docs/look-light.png
Hide
Type "q"
Sleep 1s
Type 'DEMO_SET="mode=light accent=teal player=top border=square" tapes/demo.sh history'
Enter
Sleep 3s
Type "/"
Type "rumours fleetwood mac"
Enter
Sleep 4s
Right
Sleep 500ms
Enter
Sleep 4s
Enter
Sleep 8s
Show
Sleep 1s
Screenshot docs/look-light-teal.png
Hide
Type "q"
Sleep 1s
