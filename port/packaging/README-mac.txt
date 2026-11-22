ELK ACCOUNT EDITOR - macOS (Apple Silicon)
==========================================

Edits Elk M1 / M1G account files (the ElkRP "Export Account" format) and can read, compare,
back up, send to and watch a panel directly over the network, so ElkRP is optional.

WHAT YOU NEED
  - A Mac with an Apple chip (M1, M2, M3 ...). Intel Macs are not supported by this build.
  - Nothing to install: the app is self-contained and opens its own window.

FIRST-TIME SETUP (about a minute)
  macOS blocks apps that were downloaded and are not from the App Store, and this app is not
  signed by Apple. Two commands tell your Mac you trust it. You only do this once.

  1. Unzip. You get a folder called ElkAccountEditor-mac-arm64. Put it somewhere permanent,
     for example your Documents or Applications folder.

  2. Open Terminal (Applications > Utilities > Terminal) and go into the folder. Type "cd "
     (with a space), drag the folder from Finder into the Terminal window, press Return.

  3. Paste these four lines and press Return:

       chmod +x ElkAccountEditor
       xattr -dr com.apple.quarantine .
       codesign -s - --force Photino.Native.dylib
       codesign -s - --force ElkAccountEditor

  4. Start it:

       ./ElkAccountEditor

  The editor window opens. Leave the Terminal window open while you use it; closing the editor
  window ends the program.

EASIER AFTER THAT
  Double-click "Start Elk Account Editor.command". The first time, macOS may refuse to open it:
  right-click it, choose Open, then Open again. It repeats the setup steps if needed and starts
  the editor.

IF SOMETHING GOES WRONG
  "cannot be opened because the developer cannot be verified" / "is damaged"
      Run the xattr and codesign lines from step 3 again (you must be inside the folder).
  "zsh: killed ./ElkAccountEditor"
      The codesign lines were skipped. Run them, then start it again.
  "zsh: permission denied"
      Run: chmod +x ElkAccountEditor
  Nothing happens / window does not appear
      Make sure Photino.Native.dylib is in the same folder as ElkAccountEditor.
  "Connection refused" or login timeouts when talking to a panel
      Check the address (the M1XEP's IP), the port and the connection type (below).
      Only one program can be logged in to the panel at a time, so close ElkRP (and any other
      copy of this editor) first. If the panel's network module stops answering, power-cycle
      the M1XEP.

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
  Your export files are saved wherever you choose in the Save dialog.

Take care: Send and "reset records" change what is in the panel. Make a backup first (Send does
this automatically) and try changes on a spare panel before a customer's.
