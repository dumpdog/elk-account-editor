ELK ACCOUNT EDITOR - macOS (Apple Silicon or Intel)
===================================================

Edits Elk M1 / M1G account files (the ElkRP "Export Account" format) and can read, compare,
back up, send to and watch a panel directly over the network, so ElkRP is optional.

WHAT YOU NEED
  - A Mac running macOS 11 or newer. The "arm64" zip is for Apple chips (M1, M2, M3 ...),
    the "universal" zip works on both Apple chips and Intel Macs.
  - Nothing to install: the app is self-contained and opens its own window.

FIRST-TIME SETUP (about a minute)
  macOS blocks apps that were downloaded and are not from the App Store, and this app is not
  notarized by Apple (it carries an ad-hoc signature only). One command tells your Mac you
  trust it. You only do this once.

  1. Unzip. You get a folder with "Elk Account Editor.app" in it. Move the app somewhere
     permanent, for example your Applications folder.

  2. Open Terminal (Applications > Utilities > Terminal) and paste this line (change the path
     if you put the app elsewhere), then press Return:

       xattr -dr com.apple.quarantine "/Applications/Elk Account Editor.app"

  3. Start the app by double-clicking it. If macOS still refuses, right-click the app, choose
     Open, then Open again (or System Settings > Privacy & Security > Open Anyway).

  The editor window opens. Closing the window (or Cmd+Q) ends the program.

IF THE APP LOSES ITS SIGNATURE (for example it was copied by a tool that changes files)
  Sign it again on your Mac:

       codesign -s - --force --deep "/Applications/Elk Account Editor.app"

USING YOUR BROWSER INSTEAD OF THE WINDOW
  If the window does not open, or you prefer Safari/Chrome, start the editor from Terminal:

       "/Applications/Elk Account Editor.app/Contents/MacOS/elkaccounteditor" --browser

  or double-click "Start in browser.command" (right-click > Open the first time; it must stay
  next to the app). It prints an address like http://127.0.0.1:51980 and opens it in your
  default browser. Press Ctrl+C in the Terminal window to quit. You can add an export file
  name at the end to open it at once, and --no-open to skip opening the browser.

IF SOMETHING GOES WRONG
  "cannot be opened because the developer cannot be verified" / "is damaged"
      Run the xattr line from step 2 again.
  "zsh: killed" or the app closes at once
      Run the codesign line above, then start it again.
  Nothing happens / window does not appear
      Start it with --browser (above) and tell the developer what the Terminal prints.
  "Connection refused" or login timeouts when talking to a panel
      Check the address (the M1XEP's IP), the port and the connection type (below).
      Only one program can be logged in to the panel at a time, so close ElkRP (and any other
      copy of this editor) first. If the panel's network module stops answering, power-cycle
      the M1XEP. macOS may ask whether the app may find devices on your local network: allow it.
  A second copy will not start properly
      Close the first copy. Both want the same local port.

USING IT
  Open export file...   Open an ElkRP "Export Account" file (or drag it onto the window).
  New blank account     Start from a blank M1G account.
  Save edited export... Save the result, then in ElkRP use File > Import Account.
  Panel...              Read from, compare with and send to a panel. Compare changes nothing.
                        Send makes a backup of the panel first and reads it back afterwards.
  Live                  (tab at the right) status, event log, bus modules, panel time and date,
                        and changing the RP access code - all over one open connection.
  Theme button          Auto (follows your Mac), light or dark.
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

WHERE THINGS ARE KEPT
  Panel backups are saved in your Documents folder, in "ElkEditor backups". Keep them.
  The accounts library (the saved accounts and folders on the start page and under "Library...")
  is kept in a folder "ElkEditor data" NEXT TO "Elk Account Editor.app" (the file library.sqlite,
  a settings file and backups of the library). The zip does not contain it, so an updated app
  put in the same place finds your library again. If that place cannot be written to (the app
  is run from a disk image or from the Downloads folder after a first launch, which macOS can
  make read-only), the library is kept in ~/Library/Application Support/ElkEditor instead; the
  Library window shows which folder is in use. To use another folder start the program with
  --data-dir "/some/folder" or set the environment variable ELKEDITOR_DATA. "Back up library"
  in the Library window makes a zip of it.
  (macOS may ask once whether the app may access your Documents folder: allow it.)
  Your export files are saved wherever you choose in the Save dialog.

Take care: Send and "reset records" change what is in the panel. Make a backup first (Send does
this automatically) and try changes on a spare panel before a customer's.

UPDATING
  In the app: About / Updates > Check for updates. If a newer release exists you see its notes;
  "Download and install" then needs one more click. The download is checked against the
  release's SHA256SUMS before anything is replaced, the old app is kept next to the new one
  ("Elk Account Editor.app.previous"), and the app restarts. Your library ("ElkEditor data") is
  never touched. Nothing is checked or downloaded unless you click, and nothing is sent anywhere
  except the request to the release page. The app must be in a folder you can write to
  (Applications is fine; not a disk image).
  By hand: download the new zip, quit the app, replace "Elk Account Editor.app" and keep the
  "ElkEditor data" folder where it is.
  With git (developers): rust/packaging/update-from-git.sh pulls and rebuilds.

  Panel connection and RP access code in the library: when you save an account to the library the
  dialog ticks "Also store the panel connection" and "Remember the RP access code" by default. A
  remembered code is stored as plain text (not encrypted) in library.sqlite in the data folder, so
  anyone who can read that folder can read it. Untick the box to keep the code out of the library.
  The code is never written to exports, panel backup files or logs. Panel backups and restore are in
  the Library (Backups... on an account that has a stored connection).
