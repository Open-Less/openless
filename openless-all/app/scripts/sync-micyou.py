#!/usr/bin/env python3
"""Reproducible MicYou imports. Updates are staged and patched before any writes."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import tempfile
import urllib.request

APP_ROOT = Path(__file__).resolve().parents[1]
MANIFEST = "scripts/micyou-upstream.json"
PATCH = "scripts/micyou-dsp.patch"
REPOSITORY = "MicYou-Dev/MicYou"
PATCHABLE = {
    "crates/micyou-dsp/src/lib.rs",
    "crates/micyou-infer/src/lib.rs",
    "crates/micyou-infer/src/ops.rs",
}
# Deliberately fixed destinations: never let upstream metadata choose files to
# overwrite, run downloaded scripts, or introduce workflow/config changes.
IMPORTS = {
    "crates/micyou-dsp/src/lib.rs": "tauri-app/crates/micyou-audio/src/dsp.rs",
    "crates/micyou-infer/src/lib.rs": "tauri-app/crates/micyou-infer/src/lib.rs",
    "crates/micyou-infer/src/ops.rs": "tauri-app/crates/micyou-infer/src/ops.rs",
    "crates/micyou-dsp/src/assets/purevox6.mcy": "tauri-app/crates/micyou-audio/src/assets/purevox6.mcy",
    "crates/micyou-dsp/src/assets/aec7.mcy": "tauri-app/crates/micyou-audio/src/assets/aec7.mcy",
    "crates/micyou-dsp/LICENSE": "LICENSE",
    "crates/micyou-infer/LICENSE": "LICENSE",
    "crates/micyou-dsp/LICENSE-PureVox.txt": "tauri-app/src-tauri/resources/LICENSE-PureVox.txt",
    "crates/micyou-dsp/LICENSE-AEC7.txt": "tauri-app/src-tauri/resources/LICENSE-AEC7.txt",
}


def canonical(data, path):
    return data if path.endswith(".mcy") else data.replace(b"\r\n", b"\n")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def revision(value):
    if not re.fullmatch(r"[0-9a-f]{40}", value):
        raise ValueError("an immutable, lowercase 40-character commit SHA is required")
    return value


def download(url):
    request = urllib.request.Request(url, headers={"User-Agent": "OpenLess-MicYou-Sync"})
    with urllib.request.urlopen(request, timeout=60) as response:
        data = response.read(16 * 1024 * 1024 + 1)
    if len(data) > 16 * 1024 * 1024:
        raise ValueError("upstream file exceeds import size limit")
    return data


def fetch_sources(sha):
    cache = {}
    result = {}
    for local, remote in IMPORTS.items():
        if remote not in cache:
            cache[remote] = download(f"https://raw.githubusercontent.com/{REPOSITORY}/{sha}/{remote}")
        result[local] = canonical(cache[remote], local)
    return result


def load_manifest(root):
    manifest = json.loads((root / MANIFEST).read_text())
    revision(manifest["revision"])
    if manifest.get("repository") != REPOSITORY or set(manifest["files"]) != set(IMPORTS):
        raise ValueError("unexpected import repository or file list")
    return manifest


def verify_local(root, manifest):
    if digest((root / PATCH).read_bytes()) != manifest["patchSha256"]:
        raise ValueError("local adapter patch changed; run --refresh-patch after reviewing it")
    for local, hashes in manifest["files"].items():
        if digest(canonical((root / local).read_bytes(), local)) != hashes["localSha256"]:
            raise ValueError(f"local imported file changed: {local}; run --refresh-patch to record reviewed adaptations")


def apply_patch(sources, patch):
    # Reject file additions, deletions, renames and arbitrary destinations even
    # if somebody accidentally edits the adapter patch incorrectly.
    text = patch.decode("utf-8")
    old_paths = re.findall(r"^--- a/(.+)$", text, re.MULTILINE)
    new_paths = re.findall(r"^\+\+\+ b/(.+)$", text, re.MULTILINE)
    if not old_paths or old_paths != new_paths or any(p not in PATCHABLE for p in new_paths):
        raise ValueError("adapter patch must modify only the three imported Rust source files")
    if any(line.startswith(("diff --git", "rename ", "new file mode", "deleted file mode")) for line in text.splitlines()):
        raise ValueError("adapter patch must be a plain unified diff")
    with tempfile.TemporaryDirectory(prefix="openless-micyou-stage-") as directory:
        stage = Path(directory)
        for local, data in sources.items():
            path = stage / PurePosixPath(local)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        patch_file = stage / "adapter.patch"
        patch_file.write_bytes(patch)
        for args in [["--check"], []]:
            result = subprocess.run(["git", "-c", "core.autocrlf=false", "-c", "core.eol=lf", "apply", *args, "--whitespace=error-all", str(patch_file)], cwd=stage, capture_output=True, text=True)
            if result.returncode:
                raise ValueError("MicYou adapter no longer applies cleanly; review upstream changes and refresh the patch:\n" + result.stderr)
        return {local: (stage / local).read_bytes() for local in IMPORTS}


def write_import(root, sources, manifest):
    previous = {local: (root / local).read_bytes() for local in sources}
    manifest_path = root / MANIFEST
    old_manifest = manifest_path.read_bytes() if manifest_path.exists() else None
    try:
        for local, data in sources.items():
            (root / local).write_bytes(data)
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    except BaseException:
        for local, data in previous.items():
            (root / local).write_bytes(data)
        if old_manifest is not None:
            manifest_path.write_bytes(old_manifest)
        raise


def refresh_patch(root, manifest):
    sources = fetch_sources(manifest["revision"])
    patch = ""
    current = {}
    for local, source in sources.items():
        local_data = canonical((root / local).read_bytes(), local)
        current[local] = local_data
        if local in PATCHABLE:
            patch += "".join(difflib.unified_diff(source.decode().splitlines(keepends=True), local_data.decode().splitlines(keepends=True), fromfile="a/" + local, tofile="b/" + local))
        elif source != local_data:
            raise ValueError(f"do not silently fork an upstream model or license: {local}")
    patch_bytes = patch.encode()
    if apply_patch(sources, patch_bytes) != current:
        raise ValueError("refreshed patch did not reproduce the current source")
    manifest["patchSha256"] = digest(patch_bytes)
    manifest["files"] = {local: {"upstreamSha256": digest(sources[local]), "localSha256": digest(data)} for local, data in current.items()}
    (root / PATCH).write_bytes(patch_bytes)
    (root / MANIFEST).write_text(json.dumps(manifest, indent=2) + "\n")


def sync(root, manifest, sha, write):
    verify_local(root, manifest)
    upstream = fetch_sources(sha)
    if sha == manifest["revision"]:
        for local, data in upstream.items():
            if digest(data) != manifest["files"][local]["upstreamSha256"]:
                raise ValueError(f"pinned upstream integrity mismatch: {local}")
    patched = apply_patch(upstream, (root / PATCH).read_bytes())
    changed = any(digest(data) != manifest["files"][local]["upstreamSha256"] for local, data in upstream.items())
    if not changed:
        if any(digest(data) != manifest["files"][local]["localSha256"] for local, data in patched.items()):
            raise ValueError("adapter patch does not reproduce the recorded local source")
        return False
    if write:
        updated = dict(manifest, revision=sha)
        updated["files"] = {local: {"upstreamSha256": digest(upstream[local]), "localSha256": digest(data)} for local, data in patched.items()}
        write_import(root, patched, updated)
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--verify", action="store_true", help="offline checksum verification")
    mode.add_argument("--check", action="store_true", help="check and stage upstream changes without modifying files")
    mode.add_argument("--sync", action="store_true", help="import upstream and reapply the reviewed adapter patch")
    mode.add_argument("--refresh-patch", action="store_true", help="record reviewed local source changes against the pinned revision")
    parser.add_argument("--revision", type=revision, help="immutable upstream SHA; defaults to current master for check/sync")
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args()
    manifest = load_manifest(APP_ROOT)
    sha = manifest["revision"]
    changed = False
    if args.verify:
        verify_local(APP_ROOT, manifest)
    elif args.refresh_patch:
        refresh_patch(APP_ROOT, manifest)
    else:
        sha = args.revision or revision(json.loads(download(f"https://api.github.com/repos/{REPOSITORY}/commits/master"))["sha"])
        changed = sync(APP_ROOT, manifest, sha, args.sync)
    print(json.dumps({"revision": sha, "changed": changed, "mode": "verify" if args.verify else "refresh" if args.refresh_patch else "sync" if args.sync else "check"}))
    if args.github_output:
        with args.github_output.open("a") as output:
            output.write(f"changed={str(changed).lower()}\nrevision={sha}\n")


if __name__ == "__main__":
    main()
