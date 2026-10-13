#!/usr/bin/env python3
"""Exercise the actual helper from a foreground .app fixture in a macOS GUI session."""
import argparse
import collections
import json
from pathlib import Path
import plistlib
import re
import select
import shutil
import subprocess
import sys
import tempfile
import threading
import time


def lsappinfo(*args):
    return subprocess.check_output(
        ["/usr/bin/lsappinfo", *args], text=True, timeout=3
    )


def frame(sequence, style, phase="recording"):
    return {
        "type": "frame",
        "version": 1,
        "frame": {
            "session_id": "native-macos-regression",
            "sequence": sequence,
            "state": {
                "phase": phase,
                "text": "",
                "audio_level": 0.4,
                "translation_active": False,
                "style": style,
            },
            "transcript": "中文测试 / native capsule",
            "transcript_font_size": 15.0,
            "labels": {
                "thinking": "Processing",
                "cancelled": "Cancelled",
                "error": "Failed",
                "translating": "Translating",
            },
            "bounds": [500, 500, 460, 158],
            "interactive": style != "siri",
        },
    }


def run_cycle(helper, style, ending):
    with tempfile.TemporaryFile(mode="w+") as errors:
        child = subprocess.Popen(
            [str(helper)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=errors, text=True,
        )
        stop = threading.Event()
        types = collections.Counter()
        foreground = []
        sampler_errors = []

        def sample():
            try:
                while not stop.is_set():
                    info = lsappinfo("info", f"#{child.pid}")
                    types.update(re.findall(r'type="([^"]+)"', info))
                    asn = re.search(r"ASN:0x[0-9a-f]+-0x[0-9a-f]+:", info)
                    if asn and asn.group(0) == lsappinfo("front").strip():
                        foreground.append(asn.group(0))
                    stop.wait(0.01)
            except Exception as error:
                sampler_errors.append(str(error))

        sampler = threading.Thread(target=sample)
        sampler.start()
        try:
            readable, _, _ = select.select([child.stdout], [], [], 10)
            assert readable, "helper did not send Ready within 10 seconds"
            ready = child.stdout.readline()
            if not ready:
                errors.seek(0)
                raise AssertionError(f"helper exited before Ready ({child.poll()}): {errors.read()}")
            assert json.loads(ready) == {"type": "ready", "version": 1}, ready
            for sequence, phase in enumerate(["recording", "thinking", "done"], 1):
                child.stdin.write(json.dumps(frame(sequence, style, phase)) + "\n")
                child.stdin.flush()
                time.sleep(0.15)
            if ending == "shutdown":
                child.stdin.write('{"type":"shutdown","version":1}\n')
                child.stdin.flush()
            elif ending == "idle":
                child.stdin.write(json.dumps(frame(4, style, "idle")) + "\n")
                child.stdin.flush()
            # Keep the pipe open for explicit messages so EOF cannot mask a
            # broken Shutdown/Idle handler.
            if ending == "eof":
                child.stdin.close()
            started = time.monotonic()
            code = child.wait(timeout=5)
            elapsed = time.monotonic() - started
            errors.seek(0)
            diagnostics = errors.read()
            assert code == 0, f"helper exited {code}: {diagnostics}"
            assert not sampler_errors, sampler_errors
            assert types["UIElement"] > 0, f"no LaunchServices registration sampled: {types}"
            assert set(types) == {"UIElement"}, f"unexpected application type: {types}"
            assert not foreground, "helper became the foreground application"
            return {"style": style, "ending": ending, "exit": code,
                    "types": dict(types), "exit_ms": round(elapsed * 1000)}
        finally:
            stop.set()
            sampler.join(timeout=5)
            if child.poll() is None:
                child.kill()
                child.wait()
            child.stdout.close()
            if not child.stdin.closed:
                child.stdin.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--helper", type=Path, required=True)
    parser.add_argument("--cycles", type=int, default=9)
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("requires macOS and a logged-in GUI session")
    if not args.helper.is_file() or args.cycles < 1:
        parser.error("provide an existing helper and a positive cycle count")
    with tempfile.TemporaryDirectory(prefix="openless-capsule-macos-", dir="/tmp") as directory:
        contents = Path(directory) / "OpenLess.app" / "Contents"
        (contents / "MacOS").mkdir(parents=True)
        # Deliberately challenge the helper with its enclosing host's foreground
        # identity. The embedded plist must supply the independent agent identity.
        with (contents / "Info.plist").open("wb") as output:
            plistlib.dump({"CFBundleIdentifier": "com.openless.capsule-test-host",
                          "CFBundleExecutable": "openless",
                          "CFBundleName": "OpenLess Test Host",
                          "CFBundlePackageType": "APPL", "LSUIElement": False}, output)
        shutil.copyfile("/usr/bin/true", contents / "MacOS" / "openless")
        (contents / "MacOS" / "openless").chmod(0o755)
        helper = contents / "MacOS" / "openless-native-capsule"
        shutil.copyfile(args.helper, helper)
        helper.chmod(0o755)
        # A valid executable signature alone is insufficient after placing it
        # in a foreground bundle. Sign the entire disposable fixture before
        # LaunchServices inspects it, avoiding damaged-app system alerts.
        bundle = contents.parent
        subprocess.run(["/usr/bin/codesign", "--force", "--deep", "--sign", "-", str(bundle)],
                       check=True, capture_output=True)
        subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(bundle)],
                       check=True, capture_output=True)
        results = []
        for index in range(args.cycles):
            result = run_cycle(helper, ["classic", "typeless", "siri"][index % 3],
                               ["shutdown", "eof", "idle"][(index // 3) % 3])
            results.append(result)
            print(json.dumps(result), flush=True)
        print(json.dumps({"passed": len(results), "samples": sum(
            sum(result["types"].values()) for result in results)}))


if __name__ == "__main__":
    main()
