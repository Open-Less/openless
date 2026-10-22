#!/usr/bin/env python3
"""Bundle attribution from resolved non-dev crates; never include machine paths."""
import hashlib
import json
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[1]
subprocess.run(["python3", str(root / "scripts/prepare-core.py")], check=True)
metadata = json.loads(subprocess.check_output([
    "rustup", "run", "1.99.0", "cargo", "metadata", "--format-version", "1",
    "--manifest-path", str(root / "CoreHost/Cargo.toml"), "--locked",
    "--filter-platform", "aarch64-apple-ios"
]))
nodes = {item["id"]: item for item in metadata["resolve"]["nodes"]}
reachable = set()
def visit(identifier):
    if identifier in reachable:
        return
    reachable.add(identifier)
    for dependency in nodes[identifier]["deps"]:
        if any(kind["kind"] != "dev" for kind in dependency["dep_kinds"]):
            visit(dependency["pkg"])
visit(metadata["resolve"]["root"])
output = root / "Resources/Licenses/Rust"
output.mkdir(parents=True, exist_ok=True)
manifest = []
for package in sorted(metadata["packages"], key=lambda item: (item["name"], item["version"])):
    if package["id"] not in reachable:
        continue
    directory = Path(package["manifest_path"]).parent
    files = []
    for source in sorted(directory.iterdir()):
        if not source.is_file() or source.stat().st_size > 256 * 1024:
            continue
        if not source.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")):
            continue
        name = package["name"] + "-" + package["version"] + "-" + source.name + ".txt"
        data = source.read_bytes()
        (output / name).write_bytes(data)
        files.append({"file": name, "sha256": hashlib.sha256(data).hexdigest()})
    if package["name"] in ("openless-core", "openless-ios-host"):
        files = [{"file": "../OpenLess-LICENSE.txt", "sha256": hashlib.sha256((output.parent / "OpenLess-LICENSE.txt").read_bytes()).hexdigest()}]
    if not files:
        standard = output.parent / ("SPDX-" + str(package["license"]) + ".txt")
        if standard.is_file():
            # Some upstream crates declare a license but omit its standalone file.
            # Preserve their metadata alongside the corresponding standard text.
            vcs = directory / ".cargo_vcs_info.json"
            revision = json.loads(vcs.read_text())["git"]["sha1"] if vcs.is_file() else None
            name = f"{package['name']}-{package['version']}-UPSTREAM-NOTICE.txt"
            notice = (f"Package: {package['name']} {package['version']}\n"
                      f"Declared license: {package['license']}\n"
                      f"Authors: {', '.join(package['authors'])}\n"
                      f"Repository: {package['repository'] or 'Not supplied by upstream'}\n"
                      f"Source revision: {revision or 'Not supplied by upstream'}\n\n"
                      "The upstream crate declares this license without bundling a standalone license file.\n"
                      "Standard license text: SPDX license-list-data v3.27.0.\n\n"
                      + standard.read_text()).encode()
            (output / name).write_bytes(notice)
            files = [{"file": name, "sha256": hashlib.sha256(notice).hexdigest(),
                      "source": "upstream package metadata and SPDX standard text"}]
    manifest.append({"name": package["name"], "version": package["version"],
                     "license": package["license"], "repository": package["repository"],
                     "files": files})
(output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
retained = {entry["file"] for package in manifest for entry in package["files"]}
for path in output.glob("*.txt"):
    if path.name not in retained:
        path.unlink()
print("Bundled attribution for", len(manifest), "resolved non-dev Rust packages.")
missing = [item["name"] for item in manifest if not item["files"]]
if missing:
    print("Packages with SPDX metadata only (review before public redistribution):", ", ".join(missing))
