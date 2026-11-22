"""Builds the two release zips from the published folders (run `dotnet publish` for win-x64 and osx-arm64 first, see the README).

    python make-zips.py <dist folder>

<dist folder> holds Editor-win-x64 and Editor-osx-arm64 (the publish output). Writes ElkAccountEditor-win-x64.zip and ElkAccountEditor-mac-arm64.zip next to them.
The Mac zip stores Unix permissions (the app and the launcher are executable) and Unix line endings; the Windows zip uses Windows line endings for Notepad.
Neither zip contains the .pdb debug files or web.config. The Mac app is not signed: the README tells the user how to trust it (xattr and ad-hoc codesign).
"""
import os, sys, time, zipfile

here = os.path.dirname(os.path.abspath(__file__))
dist = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.join(here, "..", "dist")
now = time.localtime()[:6]


def build(out, top, files):
    if os.path.exists(out):
        os.remove(out)
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as z:
        for src, name, mode, eol in files:
            data = open(src, "rb").read()
            if eol:
                data = data.replace(b"\r\n", b"\n").replace(b"\n", eol)
            zi = zipfile.ZipInfo(top + "/" + name, date_time=now)
            zi.compress_type = zipfile.ZIP_DEFLATED
            if mode:
                zi.create_system = 3
                zi.external_attr = (0o100000 | mode) << 16
            z.writestr(zi, data)
    print(out, os.path.getsize(out), "bytes")


win, mac = os.path.join(dist, "Editor-win-x64"), os.path.join(dist, "Editor-osx-arm64")
build(os.path.join(dist, "ElkAccountEditor-win-x64.zip"), "ElkAccountEditor-win-x64", [
    (os.path.join(win, "ElkAccountEditor.exe"), "ElkAccountEditor.exe", None, None),
    (os.path.join(here, "README-windows.txt"), "README.txt", None, b"\r\n"),
])
build(os.path.join(dist, "ElkAccountEditor-mac-arm64.zip"), "ElkAccountEditor-mac-arm64", [
    (os.path.join(mac, "ElkAccountEditor"), "ElkAccountEditor", 0o755, None),
    (os.path.join(mac, "Photino.Native.dylib"), "Photino.Native.dylib", 0o644, None),
    (os.path.join(here, "Start Elk Account Editor.command"), "Start Elk Account Editor.command", 0o755, b"\n"),
    (os.path.join(here, "README-mac.txt"), "README.txt", 0o644, b"\n"),
])
