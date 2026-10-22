#!/usr/bin/env python3
"""Prepare an isolated Core source tree for the iOS host without editing shared sources."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


IOS = Path(__file__).resolve().parents[1]
APP = IOS.parent / "app"
CORE = APP / "crates/openless-core"
PATCH = IOS / "CoreHost/Patches/ios-input-contracts.patch"
BUILD = IOS / "CoreHost/.build"
OUTPUT = BUILD / "core-source"


def checksum(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def generated_files(root):
    return {str(path.relative_to(root)): checksum(path)
            for path in sorted(root.rglob("*")) if path.is_file()}


def main():
    repository = Path(subprocess.check_output(
        ["git", "rev-parse", "--show-toplevel"], cwd=IOS, text=True).strip())
    names = subprocess.check_output(
        ["git", "ls-files", "-z", "--", str(CORE.relative_to(repository))],
        cwd=repository).decode().split("\0")
    sources = [repository / name for name in names if name]
    if not sources or not (CORE / "Cargo.toml").is_file():
        raise RuntimeError("The checked-in shared Core source is required")
    before = {str(path.relative_to(APP)): checksum(path) for path in sources}
    patch = PATCH.read_text()
    paths = re.findall(r"^(?:---|\+\+\+) [ab]/(.+)$", patch, re.MULTILINE)
    paths += [path for pair in re.findall(r"^diff --git a/(\S+) b/(\S+)$", patch, re.MULTILINE) for path in pair]
    if not paths:
        raise RuntimeError("The iOS Core patch contains no source paths")
    for path in paths:
        if not path.startswith("crates/openless-core/") or ".." in Path(path).parts:
            raise RuntimeError(f"Patch escapes the iOS Core copy: {path}")
    fingerprint = hashlib.sha256(json.dumps({
        "sources": before, "patch": checksum(PATCH), "script": checksum(Path(__file__)),
    }, sort_keys=True).encode()).hexdigest()
    BUILD.mkdir(exist_ok=True)
    with (BUILD / "core-source.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        stamp = OUTPUT / "source.json"
        if stamp.is_file():
            saved = json.loads(stamp.read_text())
            if saved.get("fingerprint") == fingerprint and saved.get("generated") == generated_files(OUTPUT / "crates/openless-core"):
                print(f"iOS Core source is current: {OUTPUT}")
                return
        staging = Path(tempfile.mkdtemp(prefix="core-source-", dir=BUILD))
        try:
            for source in sources:
                destination = staging / source.relative_to(APP)
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, destination)
            # Unmodified sibling crates and include_str resources remain shared and read-only.
            for sibling in (APP / "crates").iterdir():
                if sibling.name != "openless-core" and sibling.is_dir():
                    (staging / "crates" / sibling.name).symlink_to(sibling, target_is_directory=True)
            for name in ["assets", "contract", "src"]:
                (staging / name).symlink_to(APP / name, target_is_directory=True)
            environment = os.environ.copy()
            for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"]:
                environment.pop(name, None)
            environment["GIT_CEILING_DIRECTORIES"] = str(BUILD)
            subprocess.run(["git", "apply", "--check", str(PATCH)], cwd=staging, env=environment, check=True)
            subprocess.run(["git", "apply", str(PATCH)], cwd=staging, env=environment, check=True)
            after = {str(path.relative_to(APP)): checksum(path) for path in sources}
            if after != before:
                raise RuntimeError("Shared Core changed while preparing the iOS copy")
            (staging / "source.json").write_text(json.dumps({
                "fingerprint": fingerprint,
                "generated": generated_files(staging / "crates/openless-core"),
            }, indent=2) + "\n")
            if OUTPUT.exists():
                shutil.rmtree(OUTPUT)
            staging.rename(OUTPUT)
        finally:
            if staging.exists():
                shutil.rmtree(staging)
    print(f"Prepared isolated iOS Core source: {OUTPUT}")


if __name__ == "__main__":
    main()
