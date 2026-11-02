#!/usr/bin/env python3
"""Actions-only execution of the production owner helper and real Wry lifecycle."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

app = Path(__file__).resolve().parents[1]
vendor = app / 'src-tauri/vendor/tauri-runtime-wry'
helper = vendor / 'src/owner_thread.rs'
parser = argparse.ArgumentParser()
parser.add_argument('--runtime', action='store_true')
args = parser.parse_args()


def cargo(root, *arguments, capture=False):
    env = os.environ.copy()
    env['CARGO_TARGET_DIR'] = str(app / 'src-tauri/target/ownership-harness')
    return subprocess.run(
        ['cargo', *arguments, '--manifest-path', str(root / 'Cargo.toml')],
        check=True, text=True, encoding='utf-8', capture_output=capture, env=env,
    )


with tempfile.TemporaryDirectory(prefix='openless-owner-tests-') as directory:
    root = Path(directory)
    (root / 'src').mkdir()
    if not args.runtime:
        (root / 'Cargo.toml').write_text('''[package]
name = "openless-owner-tests"
version = "0.1.0"
edition = "2024"
''', encoding='utf-8')
        module = '#[path = ' + json.dumps(str(helper)) + ']\nmod owner_thread;\n'
        (root / 'src/lib.rs').write_text(module, encoding='utf-8')
        cargo(root, 'test', '--lib')
        (root / 'src/bin').mkdir()
        failures = {
            'owner_must_not_move': ('let owner = owner_thread::Owner::new(1); '
                                    'std::thread::spawn(move || drop(owner));', 'E0277'),
            'borrow_must_not_escape': ('let owner = owner_thread::Owner::new(1); '
                                      'let handle = owner.handle(); '
                                      'let escaped = handle.with(|value| value); '
                                      'println!("{:?}", escaped);', 'lifetime may not live long enough'),
        }
        for name, (body, diagnostic) in failures.items():
            (root / 'src/bin' / (name + '.rs')).write_text(module + 'fn main() {' + body + '}\n', encoding='utf-8')
            result = subprocess.run(
                ['cargo', 'check', '--manifest-path', str(root / 'Cargo.toml'), '--bin', name],
                text=True, encoding='utf-8', capture_output=True,
                env={**os.environ, 'CARGO_TARGET_DIR': str(app / 'src-tauri/target/ownership-harness')},
            )
            if result.returncode == 0 or diagnostic not in result.stderr:
                raise RuntimeError(name + ' did not fail for the expected ownership reason:\n' + result.stderr)
            print('verified compile-fail ownership boundary: ' + name)
    else:
        if os.name != 'nt':
            raise RuntimeError('real Windows runtime harness requires a Windows Actions runner')
        metadata = json.loads(cargo(app / 'src-tauri', 'metadata', '--locked', '--format-version=1', capture=True).stdout)
        runtime_packages = [p for p in metadata['packages'] if p['name'] == 'tauri-runtime-wry']
        assert len(runtime_packages) == 1
        package = runtime_packages[0]
        assert package['version'] == '2.12.2' and package['source'] is None
        assert Path(package['manifest_path']).resolve() == (vendor / 'Cargo.toml').resolve()
        nodes = {node['id']: node for node in metadata['resolve']['nodes']}
        assert 'tracing' not in nodes[package['id']]['features'], 'tracing Rc storage is outside this Windows patch'
        runtime = next(p for p in metadata['packages'] if p['name'] == 'tauri-runtime')
        features = nodes[package['id']]['features']
        runtime_features = nodes[runtime['id']]['features']
        manifest = '''[package]
name = "openless-runtime-ownership-tests"
version = "0.1.0"
edition = "2024"
[dependencies]
'''
        manifest += 'tauri-runtime = { version = "=2.12.2", default-features = false, features = ' + json.dumps(runtime_features) + ' }\n'
        manifest += 'tauri-runtime-wry = { version = "=2.12.2", default-features = false, features = ' + json.dumps(features) + ' }\n'
        manifest += '[patch.crates-io]\ntauri-runtime-wry = { path = ' + json.dumps(str(vendor)) + ' }\n'
        (root / 'Cargo.toml').write_text(manifest, encoding='utf-8')
        shutil.copyfile(app / 'src-tauri/Cargo.lock', root / 'Cargo.lock')
        shutil.copyfile(app / 'scripts/windows-runtime-target-main.rs', root / 'src/main.rs')
        # A temporary package has a different root entry. Seed with the app lock,
        # resolve once on Actions, then reject any dependency version/source drift.
        isolated = json.loads(cargo(root, 'metadata', '--format-version=1', capture=True).stdout)
        expected = {(p['name'], p['version'], p['source']) for p in metadata['packages']}
        for p in isolated['packages']:
            if p['name'] != 'openless-runtime-ownership-tests':
                assert (p['name'], p['version'], p['source']) in expected, 'runtime fixture dependency drift: ' + p['name']
        patched = [p for p in isolated['packages'] if p['name'] == 'tauri-runtime-wry']
        assert len(patched) == 1 and Path(patched[0]['manifest_path']).resolve() == (vendor / 'Cargo.toml').resolve()
        cargo(root, 'build', '--locked')
        exe = app / 'src-tauri/target/ownership-harness/debug/openless-runtime-ownership-tests.exe'
        for mode in ['unstarted', 'iterations', 'return', 'run']:
            result = subprocess.run([str(exe), mode], timeout=120)
            expected_code = 23 if mode == 'run' else 0
            if result.returncode != expected_code:
                raise RuntimeError('runtime ' + mode + ' exited ' + str(result.returncode))
            print('verified actual Windows runtime lifecycle: ' + mode)
