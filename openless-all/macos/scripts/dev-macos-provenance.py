"""CI-only configuration and formal-data sentinels for an isolated Dev build."""
import hashlib
import json
import os
from pathlib import Path
import re
import sys

assert os.environ.get("CI") == "true", "CI only"
app = Path(__file__).resolve().parents[2] / "app"
home = Path.home()
evidence = Path(os.environ["RUNNER_TEMP"]) / "openless-dev-evidence"
evidence.mkdir(exist_ok=True)
roots = [home / "Library/Application Support/OpenLess", home / "Library/Logs/OpenLess", home / ".openless"]


def snapshot():
    return {
        str(path.relative_to(home)): [hashlib.sha256(path.read_bytes()).hexdigest(), path.stat().st_mtime_ns]
        for root in roots for path in root.rglob("*") if path.is_file()
    }


if sys.argv[1] == "prepare":
    sha = os.environ["OPENLESS_BUILD_ID"]
    assert re.fullmatch("[0-9a-f]{40}", sha)
    # Hosted runner only; never overwrite a pre-existing formal data tree.
    assert all(not root.exists() for root in roots), "formal roots already exist"
    for root in roots:
        root.mkdir(parents=True)
        (root / "dev-isolation-sentinel.txt").write_text("owned CI sentinel\n")
    (home / ".openless/credentials.json").write_text('{"version":1,"providers":{}}\n')
    (evidence / "formal-before.json").write_text(json.dumps(snapshot(), sort_keys=True))
    version = "2.0.3-Dev.1+" + sha[:12]
    overlay_path = app / "src-tauri/tauri.dev.conf.json"
    overlay = json.loads(overlay_path.read_text())
    overlay["version"] = version
    # A windows array replaces the base array, so preserve all original window options.
    base = json.loads((app / "src-tauri/tauri.conf.json").read_text())
    windows = base["app"]["windows"]
    for window in windows:
        window["title"] = window["title"].replace("OpenLess", "OpenLess Dev", 1)
    overlay["app"] = {"windows": windows}
    overlay_path.write_text(json.dumps(overlay, indent=2) + "\n")
    # About imports package.json; npm ci has already consumed the unchanged lockfile.
    package_path = app / "package.json"
    package = json.loads(package_path.read_text())
    package["version"] = version
    package_path.write_text(json.dumps(package, indent=2) + "\n")
    with open(os.environ["GITHUB_ENV"], "a") as output:
        output.write(f"OPENLESS_DEV_VERSION={version}\n")
elif sys.argv[1] == "verify":
    assert snapshot() == json.loads((evidence / "formal-before.json").read_text()), "formal data changed"
    assert (home / "Library/Application Support/OpenLess Dev").is_dir(), "Dev data absent"
    assert (home / "Library/Logs/OpenLess Dev/openless.log").is_file(), "Dev log absent"
    dmg = Path(sys.argv[2])
    result = (evidence / "result.txt").read_text()
    assert "status=PASS\n" in result
    ui = json.loads((evidence / "ui-results.json").read_text())["stats"]
    assert ui["expected"] == 3 and ui["unexpected"] == 0 and ui["skipped"] == 0, "three UI cases must pass"
    report = {
        "source_sha": os.environ["OPENLESS_BUILD_ID"],
        "run_url": f'{os.environ["GITHUB_SERVER_URL"]}/{os.environ["GITHUB_REPOSITORY"]}/actions/runs/{os.environ["GITHUB_RUN_ID"]}',
        "version": os.environ["OPENLESS_DEV_VERSION"],
        "identifier": "com.openless.app.dev",
        "dmg": dmg.name,
        "sha256": hashlib.file_digest(dmg.open("rb"), "sha256").hexdigest(),
        "formal_data_sentinels": "unchanged",
        "formal_keychain_sentinels": "unchanged (verified by build script)",
        "smoke": result.splitlines(),
        "chromium_ui": ui,
        "validation_scope": "bounded synthetic tests; see targeted-tests.log; no full suite or live models",
    }
    (evidence / "manifest.json").write_text(json.dumps(report, indent=2) + "\n")
else:
    raise ValueError("expected prepare or verify")
