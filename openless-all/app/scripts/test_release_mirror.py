#!/usr/bin/env python3
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('release_mirror', Path(__file__).with_name('mirror-release.py'))
mirror = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mirror)

class MirrorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name) / 'release'
        self.directory.mkdir()
        self.output = Path(self.temp.name) / 'channel'
        self.tag = 'v2.0.2-Beta.7-tauri'
        self.version = '2.0.2-Beta.7+build.fixture'
        self.names = ['OpenLess_aarch64.app.tar.gz', 'OpenLess_x86_64.app.tar.gz',
            f'OpenLess_{self.version}_x64-setup.exe',
            *[f'OpenLess_{self.version}_{abi}.apk' for abi in ('arm64-v8a', 'armeabi-v7a', 'x86', 'x86_64')]]
        self.index = dict(schema=1, release_tag=self.tag, artifacts={})
        for name in self.names:
            data = b'fixture binary'
            (self.directory / name).write_bytes(data)
            comment = f'timestamp:1\tfile:{name}' + ('' if name.endswith('.apk') else f'\tversion:{self.version}')
            signature = base64.b64encode(f'untrusted comment: fixture\nplaceholder\ntrusted comment: {comment}\nplaceholder\n'.encode()).decode()
            self.index['artifacts'][name] = dict(signature=signature, size=len(data), sha256=hashlib.sha256(data).hexdigest())
    def generate(self, verify=lambda *_: None):
        mirror.generate_manifests(self.directory, self.tag, 'beta', '2026-11-10T00:00:00Z', self.output, verify)
    def save_index(self):
        (self.directory / mirror.INDEX).write_text(json.dumps(self.index))
    def test_index_generates_all_platforms_without_detached_files(self):
        self.save_index()
        verified = []
        self.generate(lambda payload, signature: verified.append(payload.name))
        self.assertEqual(len(verified), 7)
        self.assertEqual(len(list(self.output.glob('*.json'))), 14)
        for path in self.output.glob('*.json'):
            document = json.loads(path.read_text())
            self.assertEqual(document['version'], self.version)
            self.assertIn('%2Bbuild.fixture', document['url']) if 'android' in path.name or 'windows' in path.name else None
            self.assertTrue(document['signature'])
    def test_legacy_signatures_remain_supported(self):
        for name, entry in self.index['artifacts'].items():
            (self.directory / (name + '.sig')).write_text(entry['signature'])
        self.generate()
        self.assertEqual(len(list(self.output.glob('*.json'))), 14)
    def test_historical_tauri_without_version_comment(self):
        for name, entry in self.index['artifacts'].items():
            decoded = base64.b64decode(entry['signature']).decode().replace(f'\tversion:{self.version}', '')
            (self.directory / (name + '.sig')).write_text(base64.b64encode(decoded.encode()).decode())
        self.generate()
        document = json.loads((self.output / 'latest-darwin-aarch64-beta.json').read_text())
        self.assertEqual(document['version'], self.version)
    def test_index_digest_mismatch_never_advances_channel(self):
        self.save_index()
        (self.directory / self.names[0]).write_bytes(b'corrupt')
        with self.assertRaises(ValueError): self.generate()
        self.assertFalse(self.output.exists())
    def test_partial_release_never_advances_channel(self):
        self.save_index()
        (self.directory / self.names[-1]).unlink()
        with self.assertRaises(ValueError): self.generate()
        self.assertFalse(self.output.exists())
    def test_signature_verification_failure_never_advances_channel(self):
        self.save_index()
        def reject(*_): raise ValueError('minisign verification failed')
        with self.assertRaises(ValueError): self.generate(reject)
        self.assertFalse(self.output.exists())
    def test_index_cannot_cross_tags_or_channels(self):
        self.index['release_tag'] = 'v9.0.0-tauri'
        self.save_index()
        with self.assertRaises(ValueError): self.generate()
        with self.assertRaises(ValueError):
            mirror.generate_manifests(self.directory, self.tag, 'stable', '', self.output, lambda *_: None)

if __name__ == '__main__': unittest.main()
