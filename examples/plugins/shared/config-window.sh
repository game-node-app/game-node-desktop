#!/bin/sh
# A small plugin-owned native GTK dialog. Its process is the window handle the plugin focuses or
# closes. `zenity` is an optional native package dependency for this example.
exec zenity --entry \
  --title='GameNode plugin preferences' \
  --text='Preferred catalog label' \
  --entry-text="${1-}" \
  --width=420
