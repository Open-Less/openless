#!/usr/bin/env python3
"""Mirror published releases, then publish rolling manifests after binary upload.
Migrated from aeoform/openless-workspace/tools and mirror-release.yml.
No remote pruning: existing release objects and historical GitHub assets are untouched.
"""
from __future__ import annotations
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import urllib.parse
import urllib.request

REPO = 'Open-Less/openless'
BASE = 'https://dl.aeoform.com'
INDEX = 'updater-signatures.json'
TARGETS = {
    'darwin-aarch64': r'^OpenLess_aarch64\.app\.tar\.gz$',
    'darwin-x86_64': r'^OpenLess_x86_64\.app\.tar\.gz$',
    'windows-x86_64': r'^OpenLess_\d.*_x64-setup\.exe$',
    'android-aarch64': r'^OpenLess_\d.*_arm64-v8a\.apk$',
    'android-armv7': r'^OpenLess_\d.*_armeabi-v7a\.apk$',
    'android-i686': r'^OpenLess_\d.*_x86\.apk$',
    'android-x86_64': r'^OpenLess_\d.*_x86_64\.apk$',
}

def tag_version(tag):
    match = re.fullmatch(r'v(\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?)-tauri', tag)
    if not match:
        raise ValueError('Invalid release tag')
    return match[1]

def signature_for(directory, tag, name):
    index = directory / INDEX
    if index.exists():
        value = json.loads(index.read_text())
        if value.get('schema') != 1 or value.get('release_tag') != tag:
            raise ValueError('Signature index schema/tag mismatch')
        entry = value['artifacts'][name]
        data = (directory / name).read_bytes()
        if entry['size'] != len(data) or entry['sha256'] != hashlib.sha256(data).hexdigest():
            raise ValueError('Index payload digest mismatch')
        return entry['signature'].strip()
    return (directory / (name + '.sig')).read_text().strip()

def signed_version(signature, name, tag, legacy_version=None):
    text = base64.b64decode(signature, validate=True).decode()
    comment = next(line[len('trusted comment: '):] for line in text.splitlines() if line.startswith('trusted comment: '))
    fields = dict(part.split(':', 1) for part in comment.split('\t') if ':' in part)
    version = fields.get('version')
    if name.endswith('.apk'):
        if fields.get('file') != name:
            raise ValueError('APK signed filename mismatch')
        match = re.fullmatch(r'OpenLess_(.+)_(?:arm64-v8a|armeabi-v7a|x86_64|x86)\.apk', name)
        version = match[1] if match else None
    if not version and legacy_version is not None:
        # Historical Tauri signatures have no version field. Prefer their signed
        # filename; old mac archives used the unversioned OpenLess.app.tar.gz.
        match = re.fullmatch(r'OpenLess_(.+)_(?:x64-setup\.exe|arm64-v8a\.apk|armeabi-v7a\.apk|x86_64\.apk|x86\.apk)', fields.get('file', ''))
        version = match[1] if match else legacy_version if name.endswith('.app.tar.gz') else None
    if not version or version.split('+', 1)[0] != tag_version(tag):
        raise ValueError('Signed version does not match release tag')
    return version

def generate_manifests(directory, tag, channel, published_at, output, verify):
    expected = tag_version(tag)
    if channel not in ('stable', 'beta') or (channel == 'beta') != ('-' in expected):
        raise ValueError('Release channel/tag mismatch')
    names = [p.name for p in directory.iterdir() if p.is_file()]
    documents = {}
    legacy_version = None
    if not (directory / INDEX).exists():
        versions = set()
        for name in names:
            match = re.fullmatch(r'OpenLess_(\d[^_]+)_(?:x64-setup\.exe|arm64-v8a\.apk|armeabi-v7a\.apk|x86_64\.apk|x86\.apk)', name)
            if match and match[1].split('+', 1)[0] == expected:
                versions.add(match[1])
        if len(versions) != 1:
            raise ValueError('Historical release has inconsistent full versions')
        legacy_version = versions.pop()
    for target, pattern in TARGETS.items():
        matches = [name for name in names if re.fullmatch(pattern, name)]
        if len(matches) != 1:
            raise ValueError(f'{tag}: expected one payload for {target}, found {matches}')
        name = matches[0]
        signature = signature_for(directory, tag, name)
        verify(directory / name, signature)  # includes the trusted comment's global signature
        version = signed_version(signature, name, tag, legacy_version)
        documents[target] = dict(version=version, pub_date=published_at,
            url=f'{BASE}/releases/{urllib.parse.quote(tag, safe="")}/{urllib.parse.quote(name, safe="")}', signature=signature)
    # Build every document before writing anything; never advance a partial channel.
    output.mkdir(parents=True, exist_ok=True)
    for target, document in documents.items():
        stem = f'latest-{target}' + ('-beta' if channel == 'beta' else '')
        for suffix in ('', '-mirror'):
            (output / f'{stem}{suffix}.json').write_text(json.dumps(document, indent=2) + '\n')

def main():
    if not all(os.environ.get(key) for key in ('OSS_BUCKET', 'OSS_AK', 'OSS_SK')):
        raise ValueError('Configure OSS_BUCKET, OSS_ACCESS_KEY_ID and OSS_ACCESS_KEY_SECRET in the main repository')
    releases = json.loads(subprocess.check_output(['gh', 'api', f'repos/{REPO}/releases?per_page=100']))
    releases = [r for r in releases if not r['draft'] and re.fullmatch(r'v\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?-tauri', r['tag_name'])]
    selected = {}
    for channel, prerelease in [('stable', False), ('beta', True)]:
        candidates = [r for r in releases if r['prerelease'] == prerelease]
        if candidates:
            selected[channel] = max(candidates, key=lambda r: r['published_at'])
    if not selected:
        raise ValueError('No published desktop releases')
    os.environ.update(RCLONE_CONFIG_OSS_TYPE='s3', RCLONE_CONFIG_OSS_PROVIDER='Alibaba',
        RCLONE_CONFIG_OSS_ACCESS_KEY_ID=os.environ['OSS_AK'], RCLONE_CONFIG_OSS_SECRET_ACCESS_KEY=os.environ['OSS_SK'],
        RCLONE_CONFIG_OSS_ENDPOINT=os.environ.get('OSS_ENDPOINT', 'oss-cn-zhangjiakou.aliyuncs.com'))
    root = Path(os.environ.get('RUNNER_TEMP', tempfile.gettempdir())) / 'openless-release-mirror'
    root.mkdir(parents=True, exist_ok=True)
    pubkey = json.loads((Path(__file__).parent / '../src-tauri/tauri.conf.json').read_text())['plugins']['updater']['pubkey']
    public = root / 'updater.pub'
    public.write_bytes(base64.b64decode(pubkey, validate=True))
    def verify(payload, signature):
        sig = root / 'verify.minisig'
        sig.write_bytes(base64.b64decode(signature, validate=True))
        subprocess.run(['minisign', '-Vm', str(payload), '-p', str(public), '-x', str(sig), '-q'], check=True)
    def sync_channel(channel, release):
        tag = release['tag_name']
        directory = root / 'stage' / tag
        directory.mkdir(parents=True, exist_ok=True)
        for asset in release['assets']:
            name = asset['name']
            if Path(name).name != name or '\\' in name or name in ('.', '..'):
                raise ValueError('Unsafe release asset name')
            if not (re.search(r'\.(dmg|exe|msi|deb|rpm|apk|hap|sig|tar\.gz)$', name) or name in (INDEX, 'SHA256SUMS')):
                continue
            url = asset['browser_download_url']
            expected = f'https://github.com/{REPO}/releases/download/{tag}/'
            if not url.startswith(expected) or urllib.parse.unquote(url[len(expected):]) != name:
                raise ValueError('Unexpected release download URL')
            path = directory / name
            subprocess.run(['curl', '-fL', '--retry', '3', '--connect-timeout', '20', '--max-time', '600', '-o', str(path) + '.download', url], check=True)
            downloaded = Path(str(path) + '.download')
            if downloaded.stat().st_size != asset['size']:
                raise ValueError('Incomplete release download')
            if (asset.get('digest') or '').startswith('sha256:') and hashlib.sha256(downloaded.read_bytes()).hexdigest() != asset['digest'][7:]:
                raise ValueError('Release asset SHA-256 mismatch')
            downloaded.replace(path)
        channels = root / 'channels' / channel
        generate_manifests(directory, tag, channel, release['published_at'], channels, verify)
        destination = f'oss:{os.environ["OSS_BUCKET"]}'
        common = ['--checksum', '--transfers', '4', '--retries', '5', '--retries-sleep', '10s']
        # Copy binaries first; if any transfer fails, never advance the rolling JSON.
        subprocess.run(['rclone', 'copy', str(directory), f'{destination}/releases/{tag}', *common,
            '--header-upload', 'Cache-Control: public, max-age=31536000, immutable'], check=True)
        subprocess.run(['rclone', 'copy', str(channels), f'{destination}/updates/{channel}', *common,
            '--header-upload', 'Cache-Control: public, max-age=60'], check=True)
        print(f'Mirrored {channel}: {tag}', flush=True)
    failures = []
    for channel, release in selected.items():
        try:
            sync_channel(channel, release)
        except Exception as error:
            failures.append(channel)
            print(f'{channel} mirror failed: {error}', file=sys.stderr, flush=True)
    if failures:
        raise RuntimeError(f'Mirror incomplete for: {", ".join(failures)}')

if __name__ == '__main__':
    main()
