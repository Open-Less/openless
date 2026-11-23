#!/usr/bin/env python3
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

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

    def test_each_file_uses_its_own_bounded_copy_with_visible_progress(self):
        with mock.patch.object(mirror.subprocess, 'run') as run:
            mirror.copy_files(self.directory, 'oss:fixture/releases/tag', 'public, immutable', 600)
        self.assertEqual(run.call_count, len(self.names))
        for call in run.call_args_list:
            command = call.args[0]
            self.assertEqual(command[:2], ['rclone', 'copyto'])
            self.assertEqual(command[3], f'oss:fixture/releases/tag/{Path(command[2]).name}')
            self.assertIn('--checksum', command)
            self.assertEqual(command[command.index('--stats-log-level') + 1], 'NOTICE')
            self.assertTrue(call.kwargs['check'])
            self.assertGreater(call.kwargs['timeout'], 0)
            self.assertLessEqual(call.kwargs['timeout'], mirror.FILE_TRANSFER_SECONDS)

    def test_all_payloads_finish_before_any_rolling_manifest_upload(self):
        self.save_index()
        self.generate()
        with mock.patch.object(mirror.subprocess, 'run') as run:
            mirror.publish_verified_release(self.directory, self.output, 'oss:fixture', self.tag, 'beta')
        destinations = [call.args[0][3] for call in run.call_args_list]
        first_manifest = next(i for i, name in enumerate(destinations) if '/updates/' in name)
        self.assertEqual(first_manifest, len(list(self.directory.iterdir())))
        self.assertTrue(all('/releases/' in name for name in destinations[:first_manifest]))
        self.assertTrue(all('/updates/beta/' in name for name in destinations[first_manifest:]))

    def test_payload_timeout_never_starts_rolling_uploads(self):
        self.save_index()
        self.generate()
        failure = mirror.subprocess.TimeoutExpired(['rclone', 'copyto'], 300)
        with mock.patch.object(mirror.subprocess, 'run', side_effect=failure) as run:
            with self.assertRaises(mirror.subprocess.TimeoutExpired):
                mirror.publish_verified_release(self.directory, self.output, 'oss:fixture', self.tag, 'beta')
        self.assertEqual(run.call_count, 1)
        self.assertNotIn('/updates/', run.call_args.args[0][3])

    def test_exhausted_phase_budget_does_not_start_another_request(self):
        with mock.patch.object(mirror.time, 'monotonic', side_effect=[0, 61]), mock.patch.object(mirror.subprocess, 'run') as run:
            with self.assertRaises(TimeoutError):
                mirror.copy_files(self.directory, 'oss:fixture', 'public', 60)
        run.assert_not_called()

    def test_beta_recovery_does_not_select_stable_or_drafts(self):
        release = lambda tag, pre, published, draft=False: dict(tag_name=tag, prerelease=pre, published_at=published, draft=draft)
        releases = [release('v2.0.2-tauri', False, '2026-11-20'),
            release('v2.0.3-Beta.2-tauri', True, '2026-11-22'),
            release('v2.0.3-Beta.3-tauri', True, '2026-11-23'),
            release('v2.0.3-Beta.4-tauri', True, '2026-11-24', True)]
        selected = mirror.select_releases(releases, 'beta')
        self.assertEqual(list(selected), ['beta'])
        self.assertEqual(selected['beta']['tag_name'], 'v2.0.3-Beta.3-tauri')
        self.assertEqual(set(mirror.select_releases(releases)), {'stable', 'beta'})
        with self.assertRaises(ValueError): mirror.select_releases(releases, 'unknown')

if __name__ == '__main__': unittest.main()
