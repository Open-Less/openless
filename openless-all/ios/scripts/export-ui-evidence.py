#!/usr/bin/env python3
"""Export named XCUITest screenshots with traceable, human-readable filenames."""
import argparse
import json
import re
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("bundle", type=Path)
parser.add_argument("--tag", required=True, help="e.g. light, dark, large-type")
parser.add_argument("--output", type=Path, default=root / "Artifacts/Review")
args = parser.parse_args()
bundle = args.bundle.resolve()
if not bundle.is_dir() or bundle.suffix != ".xcresult" or not (bundle / "Info.plist").is_file():
    parser.error("Supply a finalized .xcresult bundle; interrupted runs cannot export attachments")
tag = re.sub(r"[^\w.-]+", "-", args.tag).strip(".-")
if not tag:
    parser.error("Tag must contain a filename-safe character")
attachments = root / "Artifacts" / (bundle.stem + "-attachments")
manifest = attachments / "manifest.json"
if not manifest.exists():
    subprocess.run([
        "xcrun", "xcresulttool", "export", "attachments", "--path", str(bundle),
        "--output-path", str(attachments),
    ], check=True)
args.output.mkdir(parents=True, exist_ok=True)
index_path = args.output / "index.json"
index = json.loads(index_path.read_text()) if index_path.exists() else []
by_name = {item["file"]: item for item in index}
written = set()
for test in json.loads(manifest.read_text()):
    for attachment in test["attachments"]:
        filename = attachment["exportedFileName"]
        if Path(filename).name != filename or not filename.endswith(".png"):
            continue
        suggested = attachment["suggestedHumanReadableName"].split("_0_")[0]
        name = re.sub(r"[^\w.-]+", "-", suggested).strip(".-")[:100]
        if not name:
            continue
        destination = name + "-" + tag + ".png"
        if destination in written:
            continue
        written.add(destination)
        (args.output / destination).write_bytes((attachments / filename).read_bytes())
        by_name[destination] = {
            "file": destination, "bundle": str(bundle), "test": test["testIdentifier"],
            "tag": tag, "source": filename,
            "associatedWithFailure": attachment.get("isAssociatedWithFailure", False),
        }
        print(args.output / destination)
index_path.write_text(json.dumps(list(by_name.values()), ensure_ascii=False, indent=2) + "\n")
