ELK ACCOUNT EDITOR - Windows (64-bit)
=====================================

Edits Elk M1 / M1G account files (the ElkRP "Export Account" format) and can read, compare,
back up, send to and watch a panel directly over the network, so ElkRP is optional.

WHAT YOU NEED
  - Windows 10 or 11, 64-bit.
  - The Microsoft Edge WebView2 runtime. Windows 11 has it; current Windows 10 normally has it
    through Edge. If the window does not open, install the "Evergreen" WebView2 runtime from
    Microsoft.
  - Nothing else to install: the program is a single self-contained file.

HOW TO START
  1. Extract the zip first (right-click > Extract All). Do not run it from inside the zip.
     If the zip came from a download or an email, right-click the zip > Properties and tick
     "Unblock" before extracting, to avoid warnings.
  2. Double-click ElkAccountEditor.exe. The editor opens in its own window.
  3. Windows may say "Windows protected your PC" because the program is not signed. Click
     "More info", then "Run anyway". You only need to do this the first time.

  You can keep the folder anywhere (for example on the Desktop or in Documents). Closing the
  editor window ends the program. It needs no firewall rule: it only talks to its own window
  and to the panel you choose.

USING IT
  Open export file...   Open an ElkRP "Export Account" file (or drag it onto the window).
  New blank account     Start from a blank M1G account.
  Save edited export... Save the result, then in ElkRP use File > Import Account.
  Panel...              Read from, compare with and send to a panel. Compare changes nothing.
                        Send makes a backup of the panel first and reads it back afterwards.
                        A progress bar shows how far a read, compare, send or backup has got.
  Live                  (tab at the right) status, event log, bus modules, panel time and date,
                        and changing the RP access code - all over one open connection.
  Theme button          Auto (follows Windows), light or dark.
  Zones tab             Import names from, and export zones to, Excel (.xlsx) or CSV.
  Data bus tab          Plan the M1 data bus (direct chain, M1DBH hub or M1DBHR retrofit hub): a wiring
                        diagram, a check against ELK's rules and an install checklist.

CONNECTING TO A PANEL
  Address      The M1XEP's IP address.
  Connection   "Secure port, TLS" (port 2601) needs the panel's serial number (8 hex digits,
               on the control board and in ElkRP's account details). No passphrase is needed.
               "Non-secure" (port 2101) needs the M1XEP to allow non-secure connections.
  RP access    Under "Advanced" in the Panel dialog. The factory default is 246801. It is
  code         not the keypad installer code and not a user code.
  Only one program can be logged in to the panel at a time, so close ElkRP (and any other copy
  of this editor) first.

IF SOMETHING GOES WRONG
  The window does not open
      Install the WebView2 runtime (see above), then try again.
  "Connection refused" or login timeouts when talking to a panel
      Check the address, the port and the connection type. Make sure nothing else is logged in
      to the panel. If the panel's network module stops answering, power-cycle the M1XEP.
  A second copy will not start properly
      Close the first copy. Both want the same local port.

WHERE THINGS ARE KEPT
  Panel backups are saved in your Documents folder, in "ElkEditor backups". Keep them.
  The accounts library (the saved accounts and folders you see on the start page and under
  "Library...") is kept in the folder "ElkEditor data" NEXT TO ElkAccountEditor.exe (the file
  library.sqlite, a settings file and backups of the library). The zip does not contain this
  folder, so an updated or re-extracted program finds your library again as long as the new
  program is put in the same place. If the program's folder cannot be written to (for example
  Program Files), the library is kept in %APPDATA%\ElkEditor instead; the Library window shows
  which folder is in use. To use another folder start the program with
  --data-dir "D:\some folder" or set the environment variable ELKEDITOR_DATA. Copy that folder
  to move or back up everything ("Back up library" in the Library window makes a zip).
  Your export files are saved wherever you choose in the Save dialog.

Take care: Send and "reset records" change what is in the panel. Make a backup first (Send does
this automatically) and try changes on a spare panel before a customer's.

UPDATING
  In the editor: About / Updates > Check for updates. If a newer release exists you see its
  notes; "Download and install" then needs one more click. The download is checked against the
  release's SHA256SUMS before anything is replaced, the old program is kept next to the new one
  (ElkAccountEditor.exe.old), and the editor restarts. Your library ("ElkEditor data") is never
  touched. Nothing is checked or downloaded unless you click, and nothing is sent anywhere
  except the request to the release page.
  By hand: download the new zip, close the editor, replace ElkAccountEditor.exe (and elk.exe)
  with the new ones and keep the "ElkEditor data" folder where it is.
  With git (developers): rust\packaging\update-from-git.ps1 pulls and rebuilds.

  Panel connection and RP access code in the library: when you save an account to the library the
  dialog ticks "Also store the panel connection" and "Remember the RP access code" by default. A
  remembered code is stored as plain text (not encrypted) in library.sqlite in the data folder, so
  anyone who can read that folder can read it. Untick the box to keep the code out of the library.
  The code is never written to exports, panel backup files or logs. Panel backups and restore are in
  the Library (Backups... on an account that has a stored connection).
