"""Download existing Beta.2 artifacts only; never build or modify the app payload."""
import hashlib
import json
import pathlib
import subprocess
import sys

platform, destination = sys.argv[1:]
root = pathlib.Path(destination)
root.mkdir(parents=True, exist_ok=True)
repo = "Open-Less/openless"
run_id = 45062141853
source_sha = "670a44a03fc1415629553ccc3de9a02bf8205919"
version = "2.0.3-Beta.2+build.noask.20261123"
artifact_name, filename = {
    "macos": ("openless-macos-aarch64-updater", "OpenLess_aarch64.app.tar.gz"),
    "windows": ("openless-windows-x64", f"OpenLess_{version}_x64-setup.exe"),
}[platform]


def api(path):
    return json.loads(subprocess.check_output(["gh", "api", f"repos/{repo}/{path}"]))


run = api(f"actions/runs/{run_id}")
assert run["head_sha"] == source_sha and run["conclusion"] == "success"
artifact = next(item for item in api(f"actions/runs/{run_id}/artifacts")["artifacts"]
                if item["name"] == artifact_name)
artifact_id = artifact["id"]
assert artifact["name"] == artifact_name and not artifact["expired"]
assert artifact["workflow_run"]["id"] == run_id
assert artifact["workflow_run"]["head_sha"] == source_sha
release = api("releases/454700643")
assert release["tag_name"] == "v2.0.3-Beta.2-tauri"
assert release["draft"] and release["prerelease"]
asset = next(asset for asset in release["assets"] if asset["name"] == filename)
payload = root / "payload"
subprocess.run(["gh", "run", "download", str(run_id), "--repo", repo,
                "--name", artifact_name, "--dir", str(payload)], check=True)
files = list(payload.rglob(filename))
assert len(files) == 1, files
archive = files[0]
assert archive.stat().st_size == asset["size"]
# This single payload digest is needed to prove equivalence to the final attachment.
digest = "sha256:" + hashlib.file_digest(archive.open("rb"), "sha256").hexdigest()
assert digest == asset["digest"], (digest, asset["digest"])
provenance = {
    "release_run": run_id, "release_source_sha": source_sha,
    "version_expected": version, "artifact_id": artifact_id,
    "artifact_name": artifact_name, "artifact_zip_bytes": artifact["size_in_bytes"],
    "release_id": release["id"], "release_draft": release["draft"],
    "release_prerelease": release["prerelease"], "asset_id": asset["id"],
    "asset_name": filename, "asset_bytes": asset["size"], "asset_digest": digest,
    "same_final_attachment": True,
    "source_note": "SHA from immutable release run metadata, not a runtime binary SHA probe",
}
(root / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
(root / "payload-path.txt").write_text(str(archive.resolve()))
print(json.dumps(provenance, indent=2))
