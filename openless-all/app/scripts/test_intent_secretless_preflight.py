"""Offline contracts only. No Cargo or real Rust binary is invoked here."""
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
SCRIPT = Path(__file__).with_name("intent-style-comparison-ci.py")
SPEC = importlib.util.spec_from_file_location("intent_secretless_ci", SCRIPT)
ci = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci)
GATES = Path(__file__).with_name("intent-style-gates.sh")


class SecretlessPreflightTests(unittest.TestCase):
    def test_source_only_gate_binds_sha_without_provider_or_recipient(self):
        environment = {
            "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REPOSITORY": ci.REPOSITORY,
            "GITHUB_REF": "refs/heads/beta", "GITHUB_SHA": "a" * 40,
            "INTENT_EXPECTED_SOURCE_SHA": "a" * 40, "INTENT_WORKFLOW_SHA": "a" * 40,
            "GITHUB_WORKFLOW_REF": ci.REPOSITORY + "/.github/workflows/ci.yml@refs/heads/beta",
        }
        git = subprocess.CompletedProcess([], 0, stdout=("a" * 40 + "\n").encode())
        with patch.dict(os.environ, environment, clear=True):
            with patch.object(ci.subprocess, "run", return_value=git):
                self.assertEqual(ci.validate_source()["sourceSha"], "a" * 40)
                # The paid validator still requires its original actor/attempt gates.
                with self.assertRaises(ValueError):
                    ci.validate_dispatch()
                for field in ("GITHUB_SHA", "INTENT_WORKFLOW_SHA", "INTENT_EXPECTED_SOURCE_SHA"):
                    with patch.dict(os.environ, {field: "b" * 40}):
                        with self.assertRaises(ValueError):
                            ci.validate_source()
            with patch.object(ci.subprocess, "run", return_value=subprocess.CompletedProcess(
                [], 0, stdout=("b" * 40 + "\n").encode()
            )):
                with self.assertRaises(ValueError):
                    ci.validate_source()

    def test_workflow_is_nondefault_mutually_exclusive_and_secretless(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        secretless = workflow.split("\n  intent-secretless-preflight:\n", 1)[1].split(
            "\n  live-acceptance:\n", 1
        )[0]
        paid = workflow.split("\n  live-acceptance:\n", 1)[1]
        self.assertIn("default: 'off'", workflow)
        self.assertIn("inputs.live_acceptance == 'intent-style-preflight'", secretless)
        self.assertIn("inputs.live_acceptance != 'intent-style-preflight'", paid)
        self.assertIn("github.event_name != 'workflow_dispatch' || inputs.live_acceptance == 'off'", workflow)
        for text in (secretless, paid):
            self.assertIn("github.repository == 'Open-Less/openless'", text)
            self.assertIn('["refs/heads/main","refs/heads/beta"]', text)
            self.assertIn("contents: read", text)
            self.assertIn("persist-credentials: false", text)
        for forbidden in ("secrets.", "environment:", "DEEPSEEK", "ci.py run", "gh workflow"):
            self.assertNotIn(forbidden, secretless)
        self.assertIn("ci.py source-preflight", secretless)
        self.assertIn('export CARGO_TARGET_DIR="$scratch/target"', secretless)
        self.assertIn('export TMPDIR="$scratch/tmp"', secretless)
        self.assertIn("trap 'rm -rf", secretless)
        self.assertLess(secretless.index("gates.sh python"), secretless.index("gates.sh build"))
        self.assertLess(secretless.index("gates.sh build"), secretless.index("gates.sh verify"))
        helper = GATES.read_text()
        self.assertEqual(helper.count("cargo test --locked -p openless-core --lib -j 1 --no-run --message-format=json"), 1)
        self.assertNotIn("cargo ", helper.split("  verify)", 1)[1])

    def test_shared_gate_lists_live_only_and_rejects_zero_tests(self):
        # Stubs are intentional: this validates shell control flow without compiling.
        with tempfile.TemporaryDirectory(prefix="openless-secretless-contract-") as scratch:
            directory = Path(scratch)
            binary = directory / "fixture-binary"
            binary.write_text("""#!/bin/sh
printf '%s\\n' "$*" >> "$CALLS_FILE"
if [ "$1" = intent_style_tests::intent_comparison_ ]; then
  if [ "$ZERO_TESTS" = 1 ]; then
    printf 'test result: ok. 0 passed; 0 failed; 0 ignored;\\n'
  else
    printf 'test result: ok. 5 passed; 0 failed; 0 ignored;\\n'
  fi
else
  case " $* " in *" --exact --list "*) ;; *) exit 90 ;; esac
  case "$1" in
    *intent_live_configured_channel_comparison)
      case " $* " in *" --ignored --exact --list "*) ;; *) exit 91 ;; esac ;;
  esac
  printf '%s: test\\n' "$1"
fi
""")
            binary.chmod(0o700)
            temporary = directory / "tmp"
            temporary.mkdir()
            calls = directory / "calls"
            environment = {
                "PATH": os.environ["PATH"], "TMPDIR": str(temporary),
                "CALLS_FILE": str(calls), "ZERO_TESTS": "0",
            }
            result = subprocess.run(
                ["bash", str(GATES), "verify", str(binary)], env=environment,
                text=True, capture_output=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("mock_tests=5", result.stdout)
            recorded = calls.read_text().splitlines()
            self.assertEqual(len(recorded), 7)
            self.assertEqual(recorded[-1], ci.TARGET + " --ignored --exact --list")
            self.assertEqual(list(temporary.iterdir()), [])
            environment["ZERO_TESTS"] = "1"
            result = subprocess.run(
                ["bash", str(GATES), "verify", str(binary)], env=environment,
                text=True, capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("FAIL intent_gate=mock_count", result.stderr)
            self.assertEqual(list(temporary.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
