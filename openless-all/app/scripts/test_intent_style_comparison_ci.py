"""Offline tests only: the process boundary is mocked; no credentials or network."""
import importlib.util
import contextlib
import io
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("intent-style-comparison-ci.py")
SPEC = importlib.util.spec_from_file_location("intent_comparison_ci", SCRIPT)
ci = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci)
SENTINEL = "synthetic-ci-fixture-secret"


class IntentComparisonCiTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="openless-intent-ci-test-")
        self.addCleanup(self.scratch.cleanup)
        self.private = Path(self.scratch.name) / "private"
        self.private.mkdir(mode=0o700)
        self.evidence = Path(self.scratch.name) / "evidence"
        self.evaluation = json.loads(ci.FIXTURE.read_text())

    def raw(self):
        return {
            **ci.safe_report(self.evaluation),
            "credentials": {"key": SENTINEL},
            "cases": [
                {
                    "id": case["id"],
                    "input": SENTINEL,
                    "old": {"text": SENTINEL, "elapsedMs": 12},
                    "new": {"text": "https://private.invalid/" + SENTINEL, "elapsedMs": 15},
                }
                for case in self.evaluation["cases"]
            ],
        }

    def execute(self, raw=None, returncode=0, exception=None, key=SENTINEL):
        def fake_process(argv, **kwargs):
            self.assertEqual(argv, [
                "/unused/test-binary", ci.TARGET, "--ignored", "--exact", "--test-threads=1"
            ])
            self.assertEqual(kwargs["stdout"], subprocess.DEVNULL)
            self.assertEqual(kwargs["stderr"], subprocess.DEVNULL)
            self.assertNotIn("DEEPSEEK_API_KEY", kwargs["env"])
            self.assertNotIn("GITHUB_TOKEN", kwargs["env"])
            config = self.private / "channel.json"
            self.assertEqual(stat.S_IMODE(config.stat().st_mode), 0o600)
            settings = json.loads(config.read_text())
            self.assertEqual(settings["credentials"]["ark.api_key"], SENTINEL)
            self.assertEqual(settings["baseline"], "1.3.0")
            self.assertEqual(settings["model"], ci.MODEL)
            self.assertIs(settings["thinkingEnabled"], False)
            if exception:
                raise exception
            if raw is not None:
                (self.private / "results.json").write_text(json.dumps(raw))
            return subprocess.CompletedProcess(argv, returncode)

        with patch.dict(os.environ, {
            "DEEPSEEK_API_KEY": key, "GITHUB_TOKEN": SENTINEL, "GITHUB_SHA": "a" * 40
        }):
            with patch.object(ci.subprocess, "run", side_effect=fake_process) as process:
                code = ci.run("/unused/test-binary", self.private, self.evidence)
        published = (self.evidence / "status.json").read_text()
        self.assertNotIn(SENTINEL, published)
        self.assertNotIn("private.invalid", published)
        self.assertEqual(list(self.private.iterdir()), [])
        return code, json.loads(published), process.call_count

    def test_success_uploads_only_canonical_fixtures_and_status(self):
        code, report, calls = self.execute(self.raw())
        self.assertEqual((code, calls), (0, 1))
        self.assertEqual(report["executionStatus"], "completed")
        self.assertEqual(report["cases"][0]["old"], {"status": "completed", "elapsedMs": 12})
        self.assertEqual(report["cases"][0]["input"], self.evaluation["cases"][0]["input"])
        self.assertNotIn("credentials", report)
        self.assertNotIn("text", report["cases"][0]["new"])
        self.assertEqual(report["sourceSha"], "a" * 40)

    def test_provider_failure_drops_error_and_retains_pending(self):
        raw = self.raw()
        raw["cases"][0]["new"] = {"errorCode": SENTINEL}
        for case in raw["cases"][1:]:
            case["old"] = case["new"] = None
        code, report, _ = self.execute(raw, returncode=101)
        self.assertEqual(code, 1)
        self.assertEqual(report["cases"][0]["new"], {"status": "request_failed"})
        self.assertEqual(report["cases"][1]["old"], {"status": "pending"})

    def test_missing_or_invalid_credentials_fail_before_process(self):
        code, report, calls = self.execute(key="")
        self.assertEqual((code, calls), (1, 0))
        self.assertEqual(report["executionStatus"], "credentials_missing_or_invalid")

    def test_timeout_is_safe_and_cleans_config(self):
        code, report, _ = self.execute(exception=subprocess.TimeoutExpired(SENTINEL, 900))
        self.assertEqual(code, 1)
        self.assertEqual(report["executionStatus"], "timed_out")

    def test_invalid_report_never_publishes_raw_diagnostic(self):
        raw = self.raw()
        raw["cases"][0]["id"] = SENTINEL
        code, report, _ = self.execute(raw)
        self.assertEqual(code, 1)
        self.assertEqual(report["executionStatus"], "execution_or_report_failed")

    def test_zero_tests_is_failure(self):
        code, report, _ = self.execute()
        self.assertEqual(code, 1)
        self.assertEqual(report["executionStatus"], "missing_or_invalid_report")

    def test_selects_one_precompiled_binary_and_rejects_ambiguity(self):
        binary = self.private / "test-binary"
        binary.touch()
        manifest = self.private / "build.jsonl"
        artifact = {
            "reason": "compiler-artifact", "target": {"name": "openless_core"},
            "profile": {"test": True}, "executable": str(binary)
        }
        manifest.write_text(json.dumps(artifact) + "\n")
        with contextlib.redirect_stdout(io.StringIO()) as output:
            ci.select_binary(manifest)
        self.assertEqual(output.getvalue(), "binary=" + str(binary) + "\n")
        artifact["executable"] = str(self.private / "another-binary")
        with manifest.open("a") as stream:
            stream.write(json.dumps(artifact) + "\n")
        with self.assertRaises(ValueError):
            ci.select_binary(manifest)

    def test_workflow_uses_only_trusted_manual_path_and_safe_artifact(self):
        workflow = SCRIPT.parents[3] / ".github/workflows/ci.yml"
        text = workflow.read_text()
        live = text.split("\n  live-acceptance:\n", 1)[1]
        self.assertIn("default: 'off'", text)
        self.assertIn('["refs/heads/main","refs/heads/beta"]', live)
        self.assertIn("github.repository == 'Open-Less/openless'", live)
        self.assertIn("github.event_name == 'workflow_dispatch'", live)
        self.assertIn("persist-credentials: false", live)
        self.assertIn("chmod 700", live)
        self.assertIn("trap 'rm -rf", live)
        self.assertIn("inputs.live_acceptance != 'intent-style-comparison'", live)
        self.assertIn("openless-intent-evidence/status.json", live)
        self.assertIn("CARGO_TARGET_DIR: ${{ runner.temp }}/openless-live-target", live)
        self.assertIn('rm -rf "$RUNNER_TEMP/openless-live-target"', live)
        suite = SCRIPT.parent.parent / "crates/openless-core/src/deepseek_suite.rs"
        for name, value in (("MODEL", ci.MODEL), ("ENDPOINT", ci.ENDPOINT)):
            found = re.search(rf'pub const {name}: &str = "([^"]+)";', suite.read_text())
            self.assertEqual(found.group(1), value)
        self.assertEqual(
            sum(c["operation"] == "translation" for c in self.evaluation["cases"]), 2
        )


if __name__ == "__main__":
    unittest.main()
