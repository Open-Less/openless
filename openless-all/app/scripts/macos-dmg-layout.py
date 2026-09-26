#!/usr/bin/env python3
"""Add Finder metadata to Tauri's temporary DMG before its final conversion/signing."""

import contextlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile

from ds_store import DSStore
from mac_alias import Alias

HDIUTIL = "/usr/bin/hdiutil"
APP_ROOT = Path(__file__).resolve().parent.parent
BACKGROUND_COLOR_KEYS = ("backgroundColorRed", "backgroundColorGreen", "backgroundColorBlue")


def settings():
    tauri = APP_ROOT / "src-tauri"
    app = json.loads((tauri / "tauri.conf.json").read_text())
    dmg = json.loads((tauri / "tauri.macos-mlx.conf.json").read_text())["bundle"]["macOS"]["dmg"]
    style = json.loads((tauri / "dmg/layout.json").read_text())
    return app, dmg, style


def conversion_source(args):
    """Only the expected native ARM64 bundle can be altered by this scoped wrapper."""
    app, _, _ = settings()
    folder = (APP_ROOT / "src-tauri/target/release/bundle/macos").resolve()
    expected_name = f'{app["productName"]}_{app["version"]}_aarch64.dmg'
    if len(args) < 2 or args[0] != "convert":
        raise ValueError("Expected a Tauri DMG conversion")
    try:
        output = Path(args[args.index("-o") + 1]).resolve()
        image_format = args[args.index("-format") + 1]
    except (ValueError, IndexError) as error:
        raise ValueError("Unrecognized Tauri DMG conversion arguments") from error
    source = Path(args[1]).resolve()
    if (
        image_format != "UDZO"
        or output != folder / expected_name
        or source.parent != folder
        or not re.fullmatch(r"rw\.\d+\." + re.escape(expected_name), source.name)
        or not source.is_file()
    ):
        raise ValueError("Refusing DMG layout changes outside the current Tauri bundle")
    return source


@contextlib.contextmanager
def mounted(image, writable=False):
    # Do not recursively delete this directory: it may still contain a mounted volume.
    mountpoint = Path(tempfile.mkdtemp(prefix="openless-dmg-layout-", dir="/tmp")).resolve()
    device = None
    try:
        result = subprocess.run(
            [HDIUTIL, "attach", str(image), "-readwrite" if writable else "-readonly",
             "-noautoopen", "-nobrowse", "-mountpoint", str(mountpoint), "-plist"],
            check=True, stdout=subprocess.PIPE,
        )
        entities = plistlib.loads(result.stdout)["system-entities"]
        device = next(
            item["dev-entry"] for item in entities
            if item.get("mount-point") and Path(item["mount-point"]).resolve() == mountpoint
        )
        yield mountpoint
    finally:
        if device or os.path.ismount(mountpoint):
            subprocess.run([HDIUTIL, "detach", device or str(mountpoint)], check=True, stdout=subprocess.PIPE)
        mountpoint.rmdir()


def ensure_targets(volume, app_name):
    if not (volume / app_name).is_dir():
        raise ValueError("Tauri app is missing from the DMG")
    applications = volume / "Applications"
    if not applications.is_symlink() or os.readlink(applications) != "/Applications":
        raise ValueError("Applications must be the real /Applications drag target")


def write_layout(volume):
    app, dmg, style = settings()
    app_name = app["productName"] + ".app"
    ensure_targets(volume, app_name)
    background_source = APP_ROOT / "src-tauri" / style["retinaBackground"]
    if not background_source.is_file():
        raise ValueError("Retina DMG background is missing")
    background = volume / ".background/installer-background.tiff"
    background.parent.mkdir(exist_ok=True)
    shutil.copyfile(background_source, background)
    # mac_alias requires a canonical path to avoid /var vs /private/var alias drift.
    alias = Alias.for_file(str(background.resolve())).to_bytes()
    position, size = dmg["windowPosition"], dmg["windowSize"]
    bounds = "{{%d, %d}, {%d, %d}}" % (position["x"], position["y"], size["width"], size["height"])
    with DSStore.open(str(volume / ".DS_Store"), "w+") as store:
        store["."]["vSrn"] = ("long", 1)
        store["."]["icvl"] = ("type", b"icnv")
        store["."]["bwsp"] = {
            "WindowBounds": bounds, "ShowToolbar": False, "ShowStatusBar": False,
            "ShowPathbar": False, "ShowSidebar": False, "ShowTabView": False,
            "ContainerShowSidebar": False, "SidebarWidth": 0,
        }
        store["."]["icvp"] = {
            "viewOptionsVersion": 1, "iconSize": float(style["iconSize"]),
            "textSize": float(style["textSize"]), "arrangeBy": "none",
            "labelOnBottom": True, "showItemInfo": False, "showIconPreview": False,
            "gridOffsetX": 0.0, "gridOffsetY": 0.0, "gridSpacing": 64.0,
            "scrollPositionX": 0.0, "scrollPositionY": 0.0,
            "backgroundType": 2, "backgroundImageAlias": alias,
            # Finder requires the complete RGB tuple even for picture mode.
            # Without it macOS 27 ignores this icvp, including iconSize/alias.
            "backgroundColorRed": 1.0, "backgroundColorGreen": 1.0, "backgroundColorBlue": 1.0,
        }
        for name, key in [(app_name, "appPosition"), ("Applications", "applicationFolderPosition")]:
            store[name]["Iloc"] = (dmg[key]["x"], dmg[key]["y"])


def verify_retina_background(background, size):
    result = subprocess.run(
        ["/usr/bin/tiffutil", "-info", str(background)],
        check=True, capture_output=True, text=True,
    )
    metadata = result.stdout + result.stderr
    dimensions = [(int(w), int(h)) for w, h in re.findall(r"Image Width: (\d+) Image Length: (\d+)", metadata)]
    resolutions = [(float(x), float(y)) for x, y in re.findall(r"Resolution: ([\d.]+), ([\d.]+)", metadata)]
    expected = [(size["width"], size["height"]), (size["width"] * 2, size["height"] * 2)]
    if dimensions != expected or resolutions != [(72.0, 72.0), (144.0, 144.0)]:
        raise ValueError("DMG background must contain matching 1x and 2x Retina representations")


def verify_layout(image):
    app, dmg, style = settings()
    with mounted(image) as volume:
        app_name = app["productName"] + ".app"
        ensure_targets(volume, app_name)
        with DSStore.open(str(volume / ".DS_Store"), "r") as store:
            icon = store["."]["icvp"]
            view = store["."]["bwsp"]
            if any(type(icon.get(key)) is not float or icon[key] != 1.0 for key in BACKGROUND_COLOR_KEYS):
                raise ValueError("DMG Finder icon view requires all three white RGB real components")
            position, size = dmg["windowPosition"], dmg["windowSize"]
            expected_bounds = "{{%d, %d}, {%d, %d}}" % (position["x"], position["y"], size["width"], size["height"])
            if view["WindowBounds"] != expected_bounds or any(view[key] for key in ["ShowToolbar", "ShowStatusBar", "ShowPathbar", "ShowSidebar", "ShowTabView"]):
                raise ValueError("DMG window geometry does not match its design")
            if (icon["iconSize"] != style["iconSize"] or icon["textSize"] != style["textSize"]
                    or icon["backgroundType"] != 2 or icon["arrangeBy"] != "none"
                    or not icon["labelOnBottom"] or store["."]["icvl"] != (b"type", b"icnv")):
                raise ValueError("DMG icon size, label size or background mode is incorrect")
            for name, key in [(app_name, "appPosition"), ("Applications", "applicationFolderPosition")]:
                if tuple(store[name]["Iloc"]) != (dmg[key]["x"], dmg[key]["y"]):
                    raise ValueError("DMG drag target positions are incorrect")
            alias = Alias.from_bytes(icon["backgroundImageAlias"])
            if alias.target.posix_path != "/.background/installer-background.tiff":
                raise ValueError("DMG background alias escapes the installer volume")
        background = volume / ".background/installer-background.tiff"
        if background.read_bytes() != (APP_ROOT / "src-tauri" / style["retinaBackground"]).read_bytes():
            raise ValueError("DMG background does not match its Retina source")
        verify_retina_background(background, dmg["windowSize"])
        bundled_info = plistlib.loads((volume / app_name / "Contents/Info.plist").read_bytes())
        if bundled_info["CFBundleShortVersionString"] != app["version"]:
            raise ValueError("DMG contains a different app version")
    print("✓ DMG: Retina background, large icons, window layout and Applications target verified")


def main():
    if len(sys.argv) < 2:
        raise ValueError("Expected wrap or verify")
    if sys.argv[1] == "verify":
        verify_layout(Path(sys.argv[2]).resolve())
    elif sys.argv[1] == "wrap":
        args = sys.argv[2:]
        if args and args[0] == "convert":
            image = conversion_source(args)
            with mounted(image, writable=True) as volume:
                write_layout(volume)
            with Path(os.environ["OPENLESS_DMG_LAYOUT_STAMP"]).open("x") as stamp:
                stamp.write(str(image))
            print("✓ Prepared Finder layout in Tauri's temporary DMG", file=sys.stderr)
        os.execv(HDIUTIL, [HDIUTIL, *args])
    else:
        raise ValueError("Unknown DMG layout command")


if __name__ == "__main__":
    main()
