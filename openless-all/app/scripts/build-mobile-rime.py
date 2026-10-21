#!/usr/bin/env python3
"""Build the same pinned librime for OHOS and Android, outside the checkout."""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import urllib.request

APP = Path(__file__).resolve().parents[1]
CACHE = Path(os.environ.get('OPENLESS_CACHE_ROOT', '/data-win/openless-cache')) / 'mobile-rime'
SOURCES = {
    'rime': ('rime/librime', '1.17.0'),
    'yaml': ('jbeder/yaml-cpp', '0.8.0'),
    'leveldb': ('google/leveldb', '1.23'),
    'opencc': ('BYVoid/OpenCC', 'ver.1.1.9'),
    'pinyin': ('rime/rime-pinyin-simp', '0c6861ef7420ee780270ca6d993d18d4101049d0'),
    'ice': ('iDvel/rime-ice', 'da1fbe602e38f26db846fa10120ee64c2b0324c0'),
    'essay': ('rime/rime-essay', '054920de4f54c9e5994276a96a4fc2a35cb51aa3'),
    'prelude': ('rime/rime-prelude', '082425ea0684bca36474415d4a0e8db9b016487e'),
}

REQUIRED_FILES = {
    'rime-1.17.0': ('CMakeLists.txt',),
    'yaml-0.8.0': ('CMakeLists.txt',),
    'leveldb-1.23': ('CMakeLists.txt',),
    'opencc-ver.1.1.9': ('CMakeLists.txt',),
    'pinyin-0c6861ef7420ee780270ca6d993d18d4101049d0': ('pinyin_simp.schema.yaml',),
    'ice-da1fbe602e38f26db846fa10120ee64c2b0324c0': ('rime_ice.dict.yaml', 'rime_ice.schema.yaml'),
    'essay-054920de4f54c9e5994276a96a4fc2a35cb51aa3': ('essay.txt', 'AUTHORS'),
    'prelude-082425ea0684bca36474415d4a0e8db9b016487e': ('default.yaml',),
    'boost-1.85.0': ('boost/core/detail/sp_thread_sleep.hpp',),
}

def fetch(name, url):
    dest = CACHE / 'sources' / name
    archive = CACHE / 'downloads' / (name + '.tar.gz')
    required = tuple(dest / path for path in REQUIRED_FILES.get(name, ()))
    complete = lambda: dest.exists() and all(path.is_file() for path in required)
    if complete():
        return dest
    if dest.exists():
        print('Repair incomplete ' + name, flush=True)
        shutil.rmtree(dest)
        archive.unlink(missing_ok=True)
    archive.parent.mkdir(parents=True, exist_ok=True)
    if not archive.exists():
        print('Download ' + name, flush=True)
        temporary = archive.with_suffix('.partial')
        with urllib.request.urlopen(url, timeout=120) as response, temporary.open('wb') as output:
            shutil.copyfileobj(response, output)
        temporary.rename(archive)
    unpack = CACHE / 'sources' / (name + '-unpack')
    if unpack.exists():
        shutil.rmtree(unpack)
    unpack.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive) as tar:
        if sys.version_info >= (3, 12):
            tar.extractall(unpack, filter='data')
        else:
            # Python 3.11 and older do not support TarFile's data filter. Reject
            # traversal and link entries before extracting with the legacy API.
            root = os.path.realpath(unpack)
            for member in tar.getmembers():
                target = os.path.realpath(os.path.join(root, member.name))
                if os.path.commonpath((root, target)) != root:
                    raise RuntimeError('Unsafe archive path: ' + member.name)
                if member.issym() or member.islnk():
                    raise RuntimeError('Unsafe archive link: ' + member.name)
            tar.extractall(unpack)
    roots = list(unpack.iterdir())
    if len(roots) != 1:
        raise RuntimeError('Unexpected archive structure: ' + name)
    roots[0].rename(dest)
    unpack.rmdir()
    if not complete():
        print('Repair incomplete ' + name, flush=True)
        shutil.rmtree(dest)
        archive.unlink(missing_ok=True)
        return fetch(name, url)
    return dest

def run(*args):
    subprocess.run([str(x) for x in args], check=True)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('platform', choices=['harmony', 'android'])
    parser.add_argument('--ndk', default=os.environ.get('ANDROID_NDK_HOME'))
    parser.add_argument('--jobs', type=int, default=2)
    parser.add_argument('--abi', choices=['arm64-v8a', 'armeabi-v7a', 'x86', 'x86_64'], default='arm64-v8a')
    parser.add_argument('--stage', action='store_true', help='Stage JNI and data into the generated Android app')
    opts = parser.parse_args()
    toolchain = Path(os.environ.get('OPENLESS_HARMONY_TOOLCHAIN', Path.home() / '.local/share/harmony/current'))
    cmake = toolchain / 'sdk/default/openharmony/native/build-tools/cmake/bin/cmake'
    if not cmake.exists():
        cmake = Path(shutil.which('cmake') or 'cmake')
    if opts.platform == 'harmony':
        cross = ['-DCMAKE_TOOLCHAIN_FILE=' + str(toolchain / 'sdk/default/openharmony/native/build/cmake/ohos.toolchain.cmake'),
                 '-DOHOS_ARCH=arm64-v8a', '-DOHOS_PLATFORM=OHOS', '-DOHOS_STL=c++_shared']
    else:
        if not opts.ndk:
            parser.error('Android requires --ndk or ANDROID_NDK_HOME')
        cross = ['-DCMAKE_TOOLCHAIN_FILE=' + str(Path(opts.ndk) / 'build/cmake/android.toolchain.cmake'),
                 '-DANDROID_ABI=' + opts.abi, '-DANDROID_PLATFORM=android-24', '-DANDROID_STL=c++_shared']
    CACHE.mkdir(parents=True, exist_ok=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        futures = {name: pool.submit(fetch, name + '-' + ref, f'https://codeload.github.com/{repo}/tar.gz/{ref}')
                   for name, (repo, ref) in SOURCES.items()}
        boost_future = pool.submit(fetch, 'boost-1.85.0', 'https://archives.boost.io/release/1.85.0/source/boost_1_85_0.tar.bz2')
        src = {name: future.result() for name, future in futures.items()}
        boost = boost_future.result()
    # OHOS, like Android, has no pthread cancellation API. Keep atomic
    # shared_ptr synchronization; skip only cancellation around nanosleep.
    compatibility = boost / 'boost/core/detail/sp_thread_sleep.hpp'
    code = compatibility.read_text()
    if '&& !defined(__OHOS__)' not in code:
        code = code.replace('defined(BOOST_HAS_PTHREADS) && !defined(__ANDROID__)',
                            'defined(BOOST_HAS_PTHREADS) && !defined(__ANDROID__) && !defined(__OHOS__)')
    if compatibility.read_text() != code:
        compatibility.write_text(code)
    root = CACHE / opts.platform
    if opts.platform == 'android':
        root = root / opts.abi
    prefix = root / 'install'
    (prefix / 'lib').mkdir(parents=True, exist_ok=True)
    (prefix / 'include').mkdir(exist_ok=True)
    common = cross + ['-DCMAKE_BUILD_TYPE=Release', '-DCMAKE_POSITION_INDEPENDENT_CODE=ON',
                      '-DBUILD_SHARED_LIBS=OFF', '-DCMAKE_INSTALL_PREFIX=' + str(prefix),
                      '-DCMAKE_PREFIX_PATH=' + str(prefix)]

    def build(name, extra, target=None, install=True):
        directory = root / ('build-' + name)
        run(cmake, '-S', src[name], '-B', directory, *common, *extra)
        command = [cmake, '--build', directory, '--parallel', opts.jobs]
        if target:
            command += ['--target', target]
        run(*command)
        if install:
            run(cmake, '--install', directory)
        return directory

    build('yaml', ['-DYAML_CPP_BUILD_TESTS=OFF', '-DYAML_CPP_BUILD_TOOLS=OFF'])
    build('leveldb', ['-DLEVELDB_BUILD_TESTS=OFF', '-DLEVELDB_BUILD_BENCHMARKS=OFF', '-DCMAKE_CXX_FLAGS=-Wno-unused-command-line-argument'])
    directory = build('opencc', ['-DENABLE_GTEST=OFF', '-DENABLE_BENCHMARK=OFF'], 'libopencc', False)
    for name in ['libopencc.a', 'libmarisa.a']:
        matches = list(directory.rglob(name))
        if not matches:
            raise RuntimeError('Missing dependency ' + name)
        shutil.copy2(matches[0], prefix / 'lib' / name)
    headers = prefix / 'include/opencc'
    headers.mkdir(exist_ok=True)
    for path in list((src['opencc'] / 'src').glob('*.h')) + list((src['opencc'] / 'src').glob('*.hpp')):
        shutil.copy2(path, headers / path.name)
    for path in directory.rglob('opencc_export.h'):
        shutil.copy2(path, headers / path.name)
    marisa_headers = src['opencc'] / 'deps/marisa-0.2.6/include'
    shutil.copytree(marisa_headers, prefix / 'include', dirs_exist_ok=True)
    directory = build('rime', ['-DBUILD_SHARED_LIBS=OFF', '-DBUILD_STATIC=ON', '-DBUILD_TEST=OFF',
                             '-DBUILD_DATA=OFF', '-DENABLE_LOGGING=OFF', '-DENABLE_EXTERNAL_PLUGINS=OFF',
                             '-DCMAKE_CXX_FLAGS=-D__OHOS__' if opts.platform == 'harmony' else '-DCMAKE_CXX_FLAGS=',
                             '-DBOOST_ROOT=' + str(boost), '-DBoost_INCLUDE_DIR=' + str(boost),
                             '-DYamlCpp_INCLUDE_PATH=' + str(prefix / 'include'), '-DYamlCpp_NEW_API=' + str(prefix / 'include'), '-DYamlCpp_LIBRARY=' + str(prefix / 'lib/libyaml-cpp.a'),
                             '-DLevelDb_INCLUDE_PATH=' + str(prefix / 'include'), '-DLevelDb_LIBRARY=' + str(prefix / 'lib/libleveldb.a'),
                             '-DMarisa_INCLUDE_PATH=' + str(prefix / 'include'), '-DMarisa_LIBRARY=' + str(prefix / 'lib/libmarisa.a'),
                             '-DOpencc_INCLUDE_PATH=' + str(prefix / 'include'), '-DOpencc_LIBRARY=' + str(prefix / 'lib/libopencc.a')], 'rime-static', False)
    shutil.copy2(directory / 'lib/librime.a', prefix / 'lib/librime.a')
    shutil.copy2(src['rime'] / 'src/rime_api.h', prefix / 'include/rime_api.h')
    data = CACHE / 'data'
    data.mkdir(exist_ok=True)
    for name in ['pinyin', 'prelude']:
        for path in src[name].glob('*.yaml'):
            shutil.copy2(path, data / path.name)
        for filename in ['LICENSE', 'LICENSE.txt', 'COPYING']:
            if (src[name] / filename).exists():
                shutil.copy2(src[name] / filename, data / (name + '-' + filename))
    # The bundled stroke dictionary is already attributed by this repository.
    shutil.copy2(APP / 'third_party/rime-stroke/stroke.dict.yaml', data / 'stroke.dict.yaml')
    (data / 'stroke.schema.yaml').write_text('schema:\n  schema_id: stroke\n  name: 笔画\nengine:\n  translators: [table_translator]\ntranslator:\n  dictionary: stroke\n', encoding='utf-8')
    (data / 'default.custom.yaml').write_text('patch:\n  schema_list:\n    - schema: pinyin_simp\n  menu/page_size: 8\n', encoding='utf-8')
    # Native Rime dictionary/spelling from upstream Ice. Lua utility processors
    # are not required for its dictionaries and are intentionally not advertised.
    ice = src['ice']
    notice = '# OpenLess modification (2026-10-17): flattened import names for mobile assets.\n'
    dictionary = notice + (ice / 'rime_ice.dict.yaml').read_text()
    for table in ['8105', 'base', 'ext', 'tencent', 'others']:
        dictionary = dictionary.replace('cn_dicts/' + table, 'ice_' + table)
        content = (ice / 'cn_dicts' / (table + '.dict.yaml')).read_text()
        content = content.replace('name: ' + table, 'name: ice_' + table, 1)
        content = content.replace('name: cn_dicts/' + table, 'name: ice_' + table, 1)
        (data / ('ice_' + table + '.dict.yaml')).write_text(notice + content)
    (data / 'rime_ice.dict.yaml').write_text(dictionary)
    shutil.copy2(ice / 'LICENSE', data / 'rime-ice-LICENSE')
    shutil.copy2(src['essay'] / 'essay.txt', data / 'essay.txt')
    shutil.copy2(src['essay'] / 'LICENSE', data / 'essay-LICENSE')
    shutil.copy2(src['essay'] / 'AUTHORS', data / 'essay-AUTHORS')
    base_schema = (data / 'pinyin_simp.schema.yaml').read_text()
    import re
    ice_schema = (ice / 'rime_ice.schema.yaml').read_text()
    speller = ice_schema[ice_schema.index('\nspeller:') + 1:]
    base_schema = re.sub(r'(?ms)^speller:\n.*?(?=^translator:)', lambda _: speller + '\n', base_schema)
    base_schema = base_schema.replace('  dictionary: pinyin_simp', '  dictionary: rime_ice')
    base_schema = base_schema.replace('\ntranslator:\n', '\ntranslator:\n  enable_word_completion: true\n')
    base_schema = base_schema.replace('  segmentors:', '  filters: [uniquifier]\n  segmentors:')
    (data / 'openless-data-version.txt').write_text('ice-da1fbe60-v1\n')
    # Both platforms share the same user dictionary and stock Rime learning.
    # No platform-specific frequency overlay competes with native ranking.
    learning = base_schema.replace('schema_id: pinyin_simp', 'schema_id: openless_pinyin_learning')
    learning = learning.replace('\ntranslator:\n', '\ntranslator:\n  enable_user_dict: true\n  user_dict: pinyin_simp\n  enable_sentence: true\n  enable_completion: true\n')
    (data / 'openless_pinyin_learning.schema.yaml').write_text(learning)
    private = base_schema.replace('schema_id: pinyin_simp', 'schema_id: openless_pinyin_private')
    private = private.replace('\ntranslator:\n', '\ntranslator:\n  enable_user_dict: false\n  contextual_suggestions: false\n')
    (data / 'openless_pinyin_private.schema.yaml').write_text(private)
    (data / 'manifest.json').write_text(json.dumps([p.name for p in data.iterdir() if p.is_file() and p.name != 'manifest.json']))
    shutil.copy2(APP / 'third_party/rime-stroke/LICENSE', data / 'stroke-LICENSE')
    shutil.copy2(src['rime'] / 'LICENSE', data / 'librime-LICENSE')
    for name, path in [('boost', boost / 'LICENSE_1_0.txt'),
                       ('yaml-cpp', src['yaml'] / 'LICENSE'),
                       ('leveldb', src['leveldb'] / 'LICENSE'),
                       ('opencc', src['opencc'] / 'LICENSE'),
                       ('marisa', src['opencc'] / 'deps/marisa-0.2.6/COPYING.md')]:
        if not path.is_file():
            raise RuntimeError('Dependency license missing: ' + str(path))
        shutil.copy2(path, data / (name + '-LICENSE'))
    (data / 'manifest.json').write_text(json.dumps([p.name for p in data.iterdir() if p.is_file() and p.name != 'manifest.json']))
    (CACHE / 'sources.json').write_text(json.dumps(SOURCES, indent=2) + '\n')
    print('Rime prefix: ' + str(prefix), flush=True)
    print('Rime data: ' + str(data), flush=True)
    if opts.platform == 'android':
        bridge = root / 'build-bridge'
        run(cmake, '-S', APP / 'mobile-ime', '-B', bridge, *common,
            '-DOPENLESS_RIME_PREFIX=' + str(prefix))
        run(cmake, '--build', bridge, '--parallel', opts.jobs)
        if not opts.stage:
            print('Android JNI: ' + str(bridge / 'libopenless_rime.so'), flush=True)
            return
        destination = APP / 'src-tauri/gen/android/app/src/main'
        if not destination.exists():
            raise RuntimeError('Run tauri android init before staging Rime Android assets')
        libraries = destination / ('jniLibs/' + opts.abi)
        libraries.mkdir(parents=True, exist_ok=True)
        shutil.copy2(bridge / 'libopenless_rime.so', libraries)
        triple = {'arm64-v8a':'aarch64-linux-android', 'armeabi-v7a':'arm-linux-androideabi', 'x86':'i686-linux-android', 'x86_64':'x86_64-linux-android'}[opts.abi]
        stl = list((Path(opts.ndk) / 'toolchains/llvm/prebuilt').glob('*/sysroot/usr/lib/' + triple + '/libc++_shared.so'))
        if not stl:
            raise RuntimeError('Android libc++_shared.so missing')
        shutil.copy2(stl[0], libraries)
        shutil.copytree(data, destination / 'assets/rime', dirs_exist_ok=True)

if __name__ == '__main__':
    main()
