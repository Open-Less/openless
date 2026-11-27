"""Opt-in CI runner. Never publish untrusted provider text, errors, or config."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys

TARGET = "intent_style_tests::intent_live_configured_channel_comparison"
MODEL = "deepseek-flash"
ENDPOINT = "https://api.deepseek.com/v1"
FIXTURE = Path(__file__).resolve().parent.parent / (
    "crates/openless-core/tests/fixtures/intent-reconstruction-evaluation.json"
)


def select_binary(manifest):
    binaries = set()
    for line in Path(manifest).read_text().splitlines():
        row = json.loads(line)
        if (
            row.get("reason") == "compiler-artifact"
            and row.get("target", {}).get("name") == "openless_core"
            and row.get("profile", {}).get("test") is True
            and row.get("executable")
        ):
            binaries.add(row["executable"])
    if len(binaries) != 1:
        raise ValueError("expected one compiled Core test binary")
    binary = next(iter(binaries))
    if "\n" in binary or "\r" in binary or not Path(binary).is_file():
        raise ValueError("invalid compiled test binary")
    print("binary=" + binary)


def safe_report(evaluation):
    # Every string comes from a trusted repository fixture or a constant here.
    # Arbitrary response text cannot be guaranteed secret-free, so omit it.
    return {
        "schemaVersion": 1,
        "baselineVersion": "1.3.0",
        "candidateVersion": "1.4.0",
        "requestedModel": MODEL,
        "thinkingEnabled": False,
        "primaryRequestLimit": 32,
        "totalRequestLimit": 36,
        "executionStatus": "not_started",
        "semanticReview": "pending; provider text intentionally not published",
        "cases": [
            {
                **{
                    key: case[key]
                    for key in (
                        "id", "operation", "input", "mustPreserve", "mustRemove", "structure"
                    )
                },
                **{
                    key: case[key]
                    for key in ("instruction", "targetLanguage")
                    if key in case
                },
                "old": {"status": "pending"},
                "new": {"status": "pending"},
            }
            for case in evaluation["cases"]
        ],
    }


def apply_status(report, raw):
    if (
        raw.get("schemaVersion") != 1
        or raw.get("baselineVersion") != "1.3.0"
        or raw.get("candidateVersion") != "1.4.0"
        or raw.get("requestedModel") != MODEL
        or raw.get("thinkingEnabled") is not False
        or not isinstance(raw.get("cases"), list)
        or [case.get("id") for case in raw["cases"]]
        != [case["id"] for case in report["cases"]]
    ):
        raise ValueError("unexpected comparison report")
    for target, case in zip(report["cases"], raw["cases"]):
        for side in ("old", "new"):
            value = case.get(side)
            if value is None:
                continue
            if not isinstance(value, dict):
                raise ValueError("unexpected result")
            if "errorCode" in value:
                target[side] = {"status": "request_failed"}
            elif isinstance(value.get("text"), str):
                target[side] = {"status": "completed"}
                elapsed = value.get("elapsedMs")
                if type(elapsed) is int and 0 <= elapsed <= 900_000:
                    target[side]["elapsedMs"] = elapsed
            else:
                raise ValueError("unexpected result")


def run(binary, private_directory, evidence_directory):
    report = safe_report(json.loads(FIXTURE.read_text()))
    source_sha = os.environ.get("GITHUB_SHA", "")
    if re.fullmatch("[0-9a-f]{40}", source_sha):
        report["sourceSha"] = source_sha
    operations = [case["operation"] for case in report["cases"]]
    if (
        len(operations) != 16
        or operations.count("dictation") != 12
        or operations.count("translation") != 2
        or operations.count("selection") != 2
    ):
        raise ValueError("expected the original bounded 16 fixtures")
    evidence_directory = Path(evidence_directory)
    evidence_directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    output = evidence_directory / "status.json"
    private_directory = Path(private_directory)
    config = private_directory / "channel.json"
    raw_output = private_directory / "results.json"
    result = 1
    try:
        key = os.environ.pop("DEEPSEEK_API_KEY", "")
        if not key or key != key.strip() or any(char.isspace() for char in key):
            report["executionStatus"] = "credentials_missing_or_invalid"
            return result
        # This matches the existing official-provider suite, not local user settings.
        settings = {
            "channelId": "deepseek",
            "providerType": "deepseek",
            "model": MODEL,
            "thinkingEnabled": False,
            "baseline": "1.3.0",
            "credentials": {
                "ark.endpoint": ENDPOINT,
                "ark.api_key": key,
            },
        }
        with open(config, "x", opener=lambda path, flags: os.open(path, flags, 0o600)) as stream:
            json.dump(settings, stream)
        # No workflow credentials or logging environment are inherited by the test.
        environment = {
            name: os.environ[name]
            for name in ("PATH", "HOME", "TMPDIR", "LANG")
            if name in os.environ
        }
        environment.update({
            "OPENLESS_INTENT_COMPARISON_APPROVED": "yes",
            "OPENLESS_INTENT_COMPARISON_CONFIG": str(config),
            "OPENLESS_INTENT_COMPARISON_OUTPUT": str(raw_output),
        })
        completed = subprocess.run(
            [binary, TARGET, "--ignored", "--exact", "--test-threads=1"],
            env=environment,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=900,
            check=False,
        )
        if not raw_output.is_file() or raw_output.stat().st_size > 2_000_000:
            report["executionStatus"] = "missing_or_invalid_report"
            return result
        apply_status(report, json.loads(raw_output.read_text()))
        all_completed = all(
            case[side]["status"] == "completed"
            for case in report["cases"]
            for side in ("old", "new")
        )
        result = 0 if completed.returncode == 0 and all_completed else 1
        report["executionStatus"] = "completed" if result == 0 else "test_failed"
        return result
    except subprocess.TimeoutExpired:
        report["executionStatus"] = "timed_out"
        return 1
    except Exception:
        # No repr/traceback: even parser and filesystem errors can contain secrets.
        report["executionStatus"] = "execution_or_report_failed"
        return 1
    finally:
        config.unlink(missing_ok=True)
        raw_output.unlink(missing_ok=True)
        output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    try:
        if len(sys.argv) == 3 and sys.argv[1] == "select-binary":
            select_binary(sys.argv[2])
        elif len(sys.argv) == 5 and sys.argv[1] == "run":
            sys.exit(run(*sys.argv[2:]))
        else:
            sys.exit(2)
    except Exception:
        # Inputs in this wrapper are never printed, including exception messages.
        print("Intent comparison wrapper failed; no private diagnostic was published.")
        sys.exit(1)
