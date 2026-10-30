#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOURCE="$ROOT/.vendor/librime"
PLATFORM="${1:-all}"
build_one() {
  local platform="$1"
  local work="$ROOT/.build/rime-$platform" prefix="$ROOT/.build/rime-$platform/install"
  mkdir -p "$prefix"
  local args=(-DCMAKE_BUILD_TYPE=Release -DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DCMAKE_MACOSX_BUNDLE=OFF
    -DBUILD_SHARED_LIBS=OFF "-DCMAKE_INSTALL_PREFIX=$prefix" "-DCMAKE_PREFIX_PATH=$prefix"
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DCMAKE_OSX_ARCHITECTURES=arm64
    "-DCMAKE_CXX_FLAGS=-I\"$prefix/include\"")
  if [ "$platform" != host ]; then
    local sdk=iphoneos
    [ "$platform" != simulator ] || sdk=iphonesimulator
    args+=(-DCMAKE_SYSTEM_NAME=iOS "-DCMAKE_OSX_SYSROOT=$(xcrun --sdk "$sdk" --show-sdk-path)"
      -DCMAKE_OSX_DEPLOYMENT_TARGET=16.0 -DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=BOTH
      -DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=BOTH -DCMAKE_FIND_ROOT_PATH_MODE_PACKAGE=BOTH)
  fi
  for dep in marisa-trie yaml-cpp leveldb; do
    cmake -S "$SOURCE/deps/$dep" -B "$work/$dep" "${args[@]}" \
      -DYAML_CPP_BUILD_TESTS=OFF -DYAML_CPP_BUILD_TOOLS=OFF \
      -DLEVELDB_BUILD_TESTS=OFF -DLEVELDB_BUILD_BENCHMARKS=OFF -DHAVE_SNAPPY=OFF
    cmake --build "$work/$dep" --parallel 6
    cmake --install "$work/$dep"
  done
  cmake -S "$SOURCE/deps/opencc" -B "$work/opencc" "${args[@]}" -DUSE_SYSTEM_MARISA=ON \
    -DENABLE_GTEST=OFF -DENABLE_BENCHMARK=OFF -DBUILD_DOCUMENTATION=OFF
  cmake --build "$work/opencc" --target libopencc --parallel 6
  # Only the library is needed. Cross-compiling OpenCC's data generators would
  # execute an iPhone executable on the Mac; no conversion dictionaries ship.
  mkdir -p "$prefix/include/opencc" "$prefix/lib"
  cp "$work/opencc/src/libopencc.a" "$prefix/lib/"
  cp "$SOURCE"/deps/opencc/src/*.hpp "$SOURCE"/deps/opencc/src/*.h "$prefix/include/opencc/"
  cp "$work"/opencc/src/*config.h "$work"/opencc/src/*Export.h "$prefix/include/opencc/"

  local shared=OFF
  [ "$platform" != host ] || shared=ON
  cmake -S "$SOURCE" -B "$work/rime" "${args[@]}" -DBUILD_SHARED_LIBS="$shared" \
    -DBUILD_STATIC=ON -DBUILD_TEST=OFF -DENABLE_LOGGING=OFF -DENABLE_EXTERNAL_PLUGINS=OFF \
    -DENABLE_TIMESTAMP=OFF -DCMAKE_POLICY_DEFAULT_CMP0167=OLD -DBoost_NO_BOOST_CMAKE=ON \
    "-DBoost_INCLUDE_DIR=$ROOT/.vendor/boost_1_85_0"
  cmake --build "$work/rime" --parallel 6
  cmake --install "$work/rime"
  if [ "$platform" != host ]; then
    mkdir -p "$ROOT/Frameworks/$platform"
    xcrun libtool -static -o "$ROOT/Frameworks/$platform/libOpenLessRime.a" \
      "$prefix/lib/librime.a" "$prefix/lib/libyaml-cpp.a" "$prefix/lib/libleveldb.a" \
      "$prefix/lib/libmarisa.a" "$prefix/lib/libopencc.a"
    python3 "$ROOT/scripts/validate-deployment.py" "$ROOT/Frameworks/$platform/libOpenLessRime.a"
  fi
}
if [ "$PLATFORM" = all ]; then
  build_one host; build_one device; build_one simulator
else
  build_one "$PLATFORM"
fi
if [ -x "$ROOT/.build/rime-host/install/bin/rime_deployer" ]; then
  mkdir -p "$ROOT/Resources/Rime" "$ROOT/Resources/Licenses"
  python3 "$ROOT/scripts/build-predictions.py"
  DYLD_LIBRARY_PATH="$ROOT/.build/rime-host/install/lib" "$ROOT/.build/rime-host/install/bin/rime_deployer" --build "$ROOT/Resources/Rime" "$ROOT/Resources/Rime" "$ROOT/Resources/Rime/build"
  # Deployment metadata is generated on the build machine, not user data to ship.
  rm -f "$ROOT/Resources/Rime/user.yaml"
  cp "$SOURCE/LICENSE" "$ROOT/Resources/Licenses/librime-LICENSE.txt"
  cp "$SOURCE/deps/marisa-trie/COPYING.md" "$ROOT/Resources/Licenses/marisa-trie-LICENSE.txt"
  cp "$ROOT/.vendor/rime-pinyin-simp/LICENSE" "$ROOT/Resources/Licenses/pinyin-simp-LICENSE.txt"
  cp "$ROOT/.vendor/boost_1_85_0/LICENSE_1_0.txt" "$ROOT/Resources/Licenses/boost-LICENSE.txt"
  for dep in marisa-trie yaml-cpp leveldb; do
    license="$(find "$SOURCE/deps/$dep" -maxdepth 1 -iname '*license*' -type f | head -1)"
    [ -z "$license" ] || cp "$license" "$ROOT/Resources/Licenses/$dep-LICENSE.txt"
  done
  cp "$SOURCE/deps/opencc/LICENSE" "$ROOT/Resources/Licenses/opencc-LICENSE.txt"
fi
