#!/usr/bin/env python3
"""Exercise correction-card IPC and window destruction in a signed Tauri fixture.

Build: MACOSX_DEPLOYMENT_TARGET=14.0 CARGO_PROFILE_RELEASE_STRIP=none cargo build --locked --release \
         --manifest-path src-tauri/Cargo.toml --example card-lifecycle
The fixture uses real bundled frontend assets, synthetic suggestions, and no
user backend, microphone, dictionary or preferences. Requires a macOS GUI login.
"""
import argparse
import os
import signal
import pathlib
import plistlib
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=pathlib.Path,
                        default=pathlib.Path("src-tauri/target/release/examples/card-lifecycle"))
    parser.add_argument("--debug-exception", action="store_true",
                        help="stop at the first Objective-C exception and print its backtrace")
    parser.add_argument("--manual-first", action="store_true",
                        help="wait for a real mouse click on the first card's cross")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="openless-card-lifecycle-") as directory:
        bundle = pathlib.Path(directory) / "CardLifecycle.app"
        executable = bundle / "Contents/MacOS/card-lifecycle"
        executable.parent.mkdir(parents=True)
        shutil.copy2(args.binary, executable)
        with (bundle / "Contents/Info.plist").open("wb") as output:
            plistlib.dump({
                "CFBundleExecutable": "card-lifecycle",
                "CFBundleIdentifier": "com.openless.card-lifecycle-test",
                "CFBundleName": "OpenLess Card Lifecycle Test",
                "CFBundlePackageType": "APPL",
                "LSUIElement": True,
                "NSHighResolutionCapable": True,
            }, output)
        entitlements = pathlib.Path(directory) / "debug-entitlements.plist"
        with entitlements.open("wb") as output:
            plistlib.dump({"com.apple.security.get-task-allow": True}, output)
        subprocess.run(["codesign", "--force", "--deep", "--sign", "-", "--entitlements",
                        str(entitlements), str(bundle)], check=True)
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(bundle)], check=True)
        if args.debug_exception:
            command = ["lldb", "--batch", "-o", "breakpoint set -n objc_exception_throw",
                       "-o", "run", "-o", "expression -l objc -O -- (id)$x0",
                       "-o", "bt 25", "-o", "process kill", "--", str(executable)]
        else:
            command = [str(executable)]
        environment = os.environ.copy()
        if args.manual_first:
            environment["OPENLESS_CARD_MANUAL_FIRST"] = "1"
        with subprocess.Popen(command, stdout=None if args.debug_exception else subprocess.PIPE,
                              stderr=subprocess.STDOUT, text=True, start_new_session=True,
                              env=environment) as process:
            try:
                output, _ = process.communicate(timeout=60)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                output, _ = process.communicate()
                print(output or "", end="")
                raise
        print(output or "", end="")
        if args.debug_exception:
            return
        assert process.returncode == 0, f"fixture exited {process.returncode}"
        assert "CARD_PASS 9" in output, "fixture never completed all nine cycles"
        assert "CARD_WEBVIEWS_RELEASED 9" in output
        assert "CARD_NORMAL_WINDOW_OK" in output
        assert output.count("CARD_DESTROYED ") == 9
        for action in ("reject", "accept", "timeout"):
            assert sum(line.endswith(" " + action) for line in output.splitlines()) == 3


if __name__ == "__main__":
    main()
