import difflib
import importlib.util
import json
from pathlib import Path
import tempfile
import os
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location("sync_micyou", Path(__file__).with_name("sync-micyou.py"))
sync = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sync)


class SyncTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.sources = {local: b"upstream\n" for local in sync.IMPORTS}
        self.path = "crates/micyou-dsp/src/lib.rs"
        self.sources[self.path] = "".join(f"line {i}\n" for i in range(20)).encode()
        local = self.sources[self.path].replace(b"line 5\n", b"line 5 adapted\n")
        patch = "".join(difflib.unified_diff(self.sources[self.path].decode().splitlines(True), local.decode().splitlines(True), fromfile="a/"+self.path, tofile="b/"+self.path)).encode()
        current = dict(self.sources, **{self.path: local})
        for path, data in {**current, sync.PATCH: patch}.items():
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        self.manifest = {
            "repository": sync.REPOSITORY, "revision": "a"*40,
            "patchSha256": sync.digest(patch),
            "files": {path: {"upstreamSha256": sync.digest(self.sources[path]), "localSha256": sync.digest(data)} for path, data in current.items()},
        }
        (self.root / sync.MANIFEST).write_text(json.dumps(self.manifest))

    def test_same_pin_reproduces_exact_source_without_modifying_manifest(self):
        before = (self.root / sync.MANIFEST).read_bytes()
        with mock.patch.object(sync, "fetch_sources", return_value=self.sources):
            self.assertFalse(sync.sync(self.root, self.manifest, "a"*40, True))
        self.assertEqual(before, (self.root / sync.MANIFEST).read_bytes())

    def test_git_autocrlf_does_not_change_imported_bytes(self):
        # Git for Windows commonly enables autocrlf globally. The staging
        # operation must still produce the canonical bytes in the manifest.
        with mock.patch.dict(os.environ, {"GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "core.autocrlf", "GIT_CONFIG_VALUE_0": "true"}):
            with mock.patch.object(sync, "fetch_sources", return_value=self.sources):
                self.assertFalse(sync.sync(self.root, self.manifest, "a"*40, True))

    def test_new_upstream_fix_preserves_adapter_and_advances_pin(self):
        changed = dict(self.sources)
        changed[self.path] = changed[self.path].replace(b"line 15\n", b"upstream bugfix 15\n")
        with mock.patch.object(sync, "fetch_sources", return_value=changed):
            self.assertTrue(sync.sync(self.root, self.manifest, "b"*40, True))
        content = (self.root / self.path).read_bytes()
        self.assertIn(b"line 5 adapted", content)
        self.assertIn(b"upstream bugfix 15", content)
        manifest = sync.load_manifest(self.root)
        self.assertEqual(manifest["revision"], "b"*40)
        sync.verify_local(self.root, manifest)

    def test_patch_conflict_leaves_every_file_untouched(self):
        before = {p: (self.root / p).read_bytes() for p in [*sync.IMPORTS, sync.MANIFEST]}
        changed = dict(self.sources)
        changed[self.path] = b"incompatible replacement\n"
        with mock.patch.object(sync, "fetch_sources", return_value=changed):
            with self.assertRaisesRegex(ValueError, "no longer applies"):
                sync.sync(self.root, self.manifest, "b"*40, True)
        self.assertEqual(before, {p: (self.root / p).read_bytes() for p in before})

    def test_check_never_changes_files(self):
        before = (self.root / sync.MANIFEST).read_bytes()
        changed = dict(self.sources)
        changed[self.path] += b"upstream addition\n"
        with mock.patch.object(sync, "fetch_sources", return_value=changed):
            self.assertTrue(sync.sync(self.root, self.manifest, "b"*40, False))
        self.assertEqual(before, (self.root / sync.MANIFEST).read_bytes())

    def test_local_changes_and_pinned_corruption_are_not_overwritten(self):
        path = self.root / self.path
        path.write_bytes(path.read_bytes()+b"unrecorded change\n")
        with self.assertRaisesRegex(ValueError, "local imported file changed"):
            sync.verify_local(self.root, self.manifest)
        changed = dict(self.sources)
        changed["crates/micyou-dsp/src/assets/aec7.mcy"] = b"corrupted model"
        path.write_bytes(path.read_bytes().removesuffix(b"unrecorded change\n"))
        with mock.patch.object(sync, "fetch_sources", return_value=changed):
            with self.assertRaisesRegex(ValueError, "integrity mismatch"):
                sync.sync(self.root, self.manifest, "a"*40, True)

    def test_patch_cannot_escape_source_allowlist(self):
        with self.assertRaisesRegex(ValueError, "only the three"):
            sync.apply_patch(self.sources, b"--- a/../../escape\n+++ b/../../escape\n@@ -1 +1 @@\n-old\n+new\n")


if __name__ == "__main__":
    unittest.main()
