"""Regression for Finder's complete picture-mode icon-view schema.

The reference is Obsidian 1.13.7's shipped DS_Store and a macOS 27 Finder A/B:
adding only the three RGB reals made the original background/icon layout render.
These tests use the real DSStore codec; native mounts and image decoding are
covered by that actual UDZO/Finder experiment, not emulated here.
"""

import contextlib
import importlib.util
import io
from pathlib import Path
import plistlib
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from ds_store import DSStore


SPEC = importlib.util.spec_from_file_location(
    "macos_dmg_layout", Path(__file__).with_name("macos-dmg-layout.py")
)
layout = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(layout)
RGB = ("backgroundColorRed", "backgroundColorGreen", "backgroundColorBlue")


class FinderPictureSchemaTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="openless-dmg-schema-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.volume = self.root / "volume"
        contents = self.volume / "OpenLess.app/Contents"
        contents.mkdir(parents=True)
        (contents / "Info.plist").write_bytes(
            plistlib.dumps({"CFBundleShortVersionString": "1.2.3"})
        )
        (self.volume / "Applications").symlink_to("/Applications")
        background = self.root / "src-tauri/dmg/background.tiff"
        background.parent.mkdir(parents=True)
        background.write_bytes(b"controlled-background-fixture")
        self.dmg = {
            "windowPosition": {"x": 120, "y": 120},
            "windowSize": {"width": 768, "height": 512},
            "appPosition": {"x": 216, "y": 253},
            "applicationFolderPosition": {"x": 552, "y": 253},
        }
        self.style = {"iconSize": 128, "textSize": 14, "retinaBackground": "dmg/background.tiff"}
        mocks = contextlib.ExitStack()
        self.addCleanup(mocks.close)
        mocks.enter_context(patch.object(layout, "APP_ROOT", self.root))
        mocks.enter_context(patch.object(layout, "settings", return_value=(
            {"productName": "OpenLess", "version": "1.2.3"}, self.dmg, self.style
        )))
        mocks.enter_context(patch.object(layout.Alias, "for_file", return_value=
            SimpleNamespace(to_bytes=lambda: b"controlled-alias-fixture")))
        mocks.enter_context(patch.object(layout.Alias, "from_bytes", return_value=
            SimpleNamespace(target=SimpleNamespace(posix_path="/.background/installer-background.tiff"))))
        mocks.enter_context(patch.object(layout, "mounted", side_effect=
            lambda _image: contextlib.nullcontext(self.volume)))
        mocks.enter_context(patch.object(layout, "verify_retina_background"))

    def verify(self):
        with contextlib.redirect_stdout(io.StringIO()):
            layout.verify_layout(self.root / "fixture.dmg")

    def replace_component(self, key, value, remove=False):
        layout.write_layout(self.volume)
        with DSStore.open(str(self.volume / ".DS_Store"), "r+") as store:
            icon = store["."]["icvp"]
            if remove:
                del icon[key]
            else:
                icon[key] = value
            store["."]["icvp"] = icon

    def test_writer_preserves_version_one_and_complete_rgb_real_values(self):
        layout.write_layout(self.volume)
        with DSStore.open(str(self.volume / ".DS_Store"), "r") as store:
            icon = store["."]["icvp"]
            self.assertEqual(icon["viewOptionsVersion"], 1)
            self.assertEqual(store["."]["icvl"], (b"type", b"icnv"))
            for key in RGB:
                self.assertIs(type(icon[key]), float)
                self.assertEqual(icon[key], 1.0)
        self.verify()

    def test_each_missing_rgb_component_is_rejected(self):
        for key in RGB:
            with self.subTest(key=key):
                self.replace_component(key, None, remove=True)
                with self.assertRaisesRegex(ValueError, "RGB real components"):
                    self.verify()

    def test_non_real_or_non_white_component_is_rejected(self):
        for value in (1, True, "1.0", 0.0):
            with self.subTest(value=value):
                self.replace_component("backgroundColorRed", value)
                with self.assertRaisesRegex(ValueError, "RGB real components"):
                    self.verify()


if __name__ == "__main__":
    unittest.main()
