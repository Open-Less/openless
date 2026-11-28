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
REAL_RUN = subprocess.run


class IntentComparisonCiTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.recipient_scratch = tempfile.TemporaryDirectory(prefix="openless-dummy-recipient-")
        cls.addClassCleanup(cls.recipient_scratch.cleanup)
        cls.cert = Path(cls.recipient_scratch.name) / "dummy.crt"
        cls.key = Path(cls.recipient_scratch.name) / "dummy.key"
        result = REAL_RUN([
            "openssl", "req", "-x509", "-newkey", "rsa:3072", "-sha256", "-nodes",
            "-keyout", str(cls.key), "-out", str(cls.cert), "-days", "2",
            "-subj", "/CN=OpenLess-offline-dummy",
        ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
        if result.returncode != 0:
            raise RuntimeError("existing OpenSSL required for offline dummy roundtrip")
        cls.certificate = cls.cert.read_text()
        cls.fingerprint = ci.hashlib.sha256(
            ci.openssl(["x509", "-in", str(cls.cert), "-outform", "DER"])
        ).hexdigest()

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
            "transport": {"state": "final", "attemptedSends": 32, "fallbackSends": 0},
            "credentials": {"key": SENTINEL},
            "cases": [
                {
                    "id": case["id"],
                    "input": SENTINEL,
                    "old": {"text": "合成旧版正文。", "elapsedMs": 12},
                    "new": {"text": "合成新版正文。", "sourceText": "合成源文。", "elapsedMs": 15},
                }
                for case in self.evaluation["cases"]
            ],
        }

    def execute(self, raw=None, returncode=0, exception=None, key=SENTINEL, certificate=None,
                env_changes=None, checkout_sha="a" * 40):
        model_calls = []
        def fake_process(argv, **kwargs):
            if argv == ["git", "rev-parse", "HEAD"]:
                return subprocess.CompletedProcess(argv, 0, stdout=(checkout_sha + "\n").encode())
            if argv[0] != "/unused/test-binary":
                return REAL_RUN(argv, **kwargs)
            model_calls.append(argv)
            self.assertEqual(argv, [
                "/unused/test-binary", ci.TARGET, "--ignored", "--exact", "--test-threads=1"
            ])
            self.assertEqual(kwargs["stdout"], subprocess.DEVNULL)
            self.assertEqual(kwargs["stderr"], subprocess.DEVNULL)
            self.assertNotIn("DEEPSEEK_API_KEY", kwargs["env"])
            self.assertNotIn("GITHUB_TOKEN", kwargs["env"])
            self.assertNotIn("INTENT_RECIPIENT_CERTIFICATE", kwargs["env"])
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

        environment = {
            "DEEPSEEK_API_KEY": key, "GITHUB_TOKEN": SENTINEL, "GITHUB_SHA": "a" * 40,
            "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REPOSITORY": ci.REPOSITORY,
            "GITHUB_REF": "refs/heads/beta", "GITHUB_RUN_ID": "123456789", "GITHUB_RUN_ATTEMPT": "1",
            "GITHUB_ACTOR": "appergb", "GITHUB_ACTOR_ID": ci.AUTHORIZED_ACTOR_ID,
            "INTENT_TRIGGERING_ACTOR": "appergb", "INTENT_WORKFLOW_SHA": "a" * 40,
            "GITHUB_WORKFLOW_REF": ci.REPOSITORY + "/.github/workflows/ci.yml@refs/heads/beta",
            "INTENT_EXPECTED_SOURCE_SHA": "a" * 40, "INTENT_RECIPIENT_SHA256": self.fingerprint,
            "INTENT_RECIPIENT_CERTIFICATE": (
                self.certificate if certificate is None else certificate
            ),
        }
        environment.update(env_changes or {})
        with patch.dict(os.environ, environment):
            with patch.object(ci.subprocess, "run", side_effect=fake_process):
                code = ci.run("/unused/test-binary", self.private, self.evidence)
        published = (self.evidence / "status.json").read_text()
        self.assertNotIn(SENTINEL, published)
        self.assertNotIn("private.invalid", published)
        self.assertNotIn("合成旧版正文", published)
        self.assertNotIn("合成新版正文", published)
        self.assertEqual(list(self.private.iterdir()), [])
        if code == 0:
            self.assertTrue((self.evidence / "comparison.cms").is_file())
        else:
            self.assertFalse((self.evidence / "review.json").exists())
        self.assertEqual(
            {path.name for path in self.evidence.iterdir()} - {"status.json", "comparison.cms"},
            set(),
        )
        return code, json.loads(published), len(model_calls)

    def decrypt(self, ciphertext, output, sha="a" * 40, run_id="123456789", fingerprint=None):
        return ci.decrypt_review(
            ciphertext, str(self.cert), str(self.key), str(output), sha, run_id,
            self.fingerprint if fingerprint is None else fingerprint,
        )

    def test_success_uploads_only_canonical_fixtures_and_status(self):
        code, report, calls = self.execute(self.raw())
        self.assertEqual((code, calls), (0, 1))
        self.assertEqual(report["executionStatus"], "completed")
        self.assertEqual(report["cases"][0]["old"], {"status": "completed", "elapsedMs": 12})
        self.assertEqual(report["cases"][0]["input"], self.evaluation["cases"][0]["input"])
        self.assertNotIn("credentials", report)
        self.assertNotIn("text", report["cases"][0]["new"])
        self.assertEqual(report["sourceSha"], "a" * 40)
        self.assertEqual(report["encryptedReview"], "available")
        output = Path(self.scratch.name) / "reviewed.json"
        self.decrypt(self.evidence / "comparison.cms", output)
        review = json.loads(output.read_text())
        self.assertEqual(review["cases"][0]["old"]["text"], "合成旧版正文。")
        self.assertEqual(review["cases"][0]["new"]["sourceText"], "合成源文。")
        self.assertNotIn(SENTINEL, output.read_text())
        self.assertEqual(stat.S_IMODE(output.stat().st_mode), 0o600)
        self.assertEqual(report["transport"], {
            "state": "final", "attemptedSends": 32, "fallbackSends": 0,
        })
        self.assertEqual((report["runId"], report["runAttempt"]), ("123456789", 1))

    def test_current_candidate_and_original_request_budget(self):
        report = ci.safe_report(self.evaluation)
        self.assertEqual(report["baselineVersion"], "1.3.0")
        self.assertEqual(report["candidateVersion"], "1.5.0")
        self.assertEqual(len(report["cases"]), 16)
        self.assertEqual(report["primaryRequestLimit"], 32)
        self.assertEqual(report["totalRequestLimit"], 36)
        ci.apply_status(report, self.raw())
        for stale in ("1.4.0", "1.0.0"):
            raw = self.raw()
            raw["candidateVersion"] = stale
            with self.assertRaises(ValueError):
                ci.apply_status(report, raw)

    def test_missing_recipient_fails_before_any_model_process(self):
        code, report, calls = self.execute(certificate="")
        self.assertEqual((code, calls), (1, 0))
        self.assertEqual(report["executionStatus"], "dispatch_or_recipient_unverified")
        self.assertFalse((self.evidence / "comparison.cms").exists())

    def test_dispatch_sha_actor_attempt_and_recipient_fail_before_model(self):
        changes = [
            {"INTENT_EXPECTED_SOURCE_SHA": ""},
            {"INTENT_EXPECTED_SOURCE_SHA": "a" * 39},
            {"INTENT_EXPECTED_SOURCE_SHA": "A" * 40},
            {"INTENT_EXPECTED_SOURCE_SHA": "a" * 40 + "\n"},
            {"INTENT_EXPECTED_SOURCE_SHA": "b" * 40},
            {"GITHUB_SHA": "b" * 40},
            {"INTENT_WORKFLOW_SHA": "b" * 40},
            {"GITHUB_REF": "refs/tags/beta"},
            {"GITHUB_REF": "refs/heads/beta/child"},
            {"GITHUB_REPOSITORY": "fork/openless"},
            {"GITHUB_EVENT_NAME": "pull_request"},
            {"GITHUB_WORKFLOW_REF": ci.REPOSITORY + "/.github/workflows/other.yml@refs/heads/beta"},
            {"GITHUB_WORKFLOW_REF": ci.REPOSITORY + "/.github/workflows/ci.yml@refs/heads/main"},
            {"GITHUB_ACTOR": "another-writer"},
            {"GITHUB_ACTOR_ID": "0"},
            {"INTENT_TRIGGERING_ACTOR": "another-writer"},
            {"GITHUB_RUN_ATTEMPT": "2"},
            {"GITHUB_RUN_ATTEMPT": "01"},
            {"GITHUB_RUN_ID": ""},
            {"INTENT_RECIPIENT_SHA256": ""},
            {"INTENT_RECIPIENT_SHA256": "b" * 64},
        ]
        for index, change in enumerate(changes):
            self.evidence = Path(self.scratch.name) / f"blocked-{index}"
            code, report, calls = self.execute(self.raw(), env_changes=change)
            self.assertEqual((code, calls), (1, 0), next(iter(change)))
            self.assertEqual(report["executionStatus"], "dispatch_or_recipient_unverified")
            self.assertEqual(report["transport"]["attemptedSends"], 0)
            self.assertFalse((self.evidence / "comparison.cms").exists())

    def test_checkout_drift_is_rejected_before_model(self):
        code, report, calls = self.execute(self.raw(), checkout_sha="b" * 40)
        self.assertEqual((code, calls), (1, 0))
        self.assertEqual(report["executionStatus"], "dispatch_or_recipient_unverified")

    def test_receipt_decrypt_is_bound_to_exact_run_sha_and_fingerprint(self):
        self.execute(self.raw())
        for index, binding in enumerate((
            {"sha": "b" * 40}, {"run_id": "123456790"}, {"fingerprint": "b" * 64},
        )):
            output = Path(self.scratch.name) / f"wrong-receipt-{index}.json"
            with self.assertRaises(ValueError):
                self.decrypt(self.evidence / "comparison.cms", output, **binding)
            self.assertFalse(output.exists())

    def test_only_final_bounded_transport_counts_are_certified(self):
        raw = self.raw()
        raw["transport"] = {"state": "final", "attemptedSends": 36, "fallbackSends": 4}
        code, report, _ = self.execute(raw)
        self.assertEqual(code, 0)
        self.assertEqual(report["transport"], raw["transport"])
        for index, (counts, returncode) in enumerate((
            ({"state": "running", "attemptedSends": 3, "fallbackSends": 0}, 0),
            ({"state": "final", "attemptedSends": 37, "fallbackSends": 4}, 0),
            ({"state": "final", "attemptedSends": 36, "fallbackSends": 5}, 0),
            ({"state": "final", "attemptedSends": 36, "fallbackSends": 4}, -9),
        )):
            self.evidence = Path(self.scratch.name) / f"unknown-{index}"
            raw["transport"] = counts
            code, report, _ = self.execute(raw, returncode=returncode)
            self.assertEqual(code, 1)
            self.assertEqual(report["transport"], {"state": "unknown"})

    def test_private_key_or_bad_certificate_is_rejected_before_model(self):
        code, report, calls = self.execute(certificate=self.key.read_text())
        self.assertEqual((code, calls), (1, 0))
        self.assertEqual(report["executionStatus"], "dispatch_or_recipient_unverified")

    def test_malformed_public_certificate_is_rejected_before_model(self):
        code, _, calls = self.execute(
            certificate="-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n"
        )
        self.assertEqual((code, calls), (1, 0))

    def test_sensitive_echo_is_rejected_and_no_plaintext_is_published(self):
        raw = self.raw()
        raw["cases"][0]["new"]["text"] = SENTINEL
        code, report, calls = self.execute(raw)
        self.assertEqual((code, calls), (1, 1))
        self.assertEqual(report["executionStatus"], "review_rejected_or_encryption_failed")
        self.assertFalse((self.evidence / "comparison.cms").exists())

    def test_encoded_echo_and_abnormal_output_are_rejected(self):
        for index, text in enumerate((
            ci.base64.b64encode(SENTINEL.encode()).decode(),
            "Authorization: Bearer hidden", "https://private.invalid/path",
            "\x00", "x" * (ci.MAX_TEXT + 1),
        )):
            self.evidence = Path(self.scratch.name) / f"rejected-{index}"
            raw = self.raw()
            raw["cases"][0]["new"]["sourceText"] = text
            code, report, _ = self.execute(raw)
            self.assertEqual(code, 1)
            self.assertEqual(report["executionStatus"], "review_rejected_or_encryption_failed")
            self.assertFalse((self.evidence / "comparison.cms").exists())

    def test_tampered_ciphertext_never_writes_decrypted_output(self):
        self.execute(self.raw())
        ciphertext = self.evidence / "comparison.cms"
        data = bytearray(ciphertext.read_bytes())
        data[-1] ^= 1
        ciphertext.write_bytes(data)
        output = Path(self.scratch.name) / "must-not-exist.json"
        with self.assertRaises(ValueError):
            self.decrypt(ciphertext, output)
        self.assertFalse(output.exists())

    def test_cms_uses_fresh_randomness(self):
        plain = self.private / "dummy.json"
        plain.write_bytes(b'{"dummy":true}')
        first = self.private / "first.cms"
        second = self.private / "second.cms"
        ci.cms_encrypt(plain, first, self.cert)
        ci.cms_encrypt(plain, second, self.cert)
        self.assertNotEqual(first.read_bytes(), second.read_bytes())

    def test_local_decrypt_rejects_unauthenticated_cms_before_private_key_use(self):
        plain = self.private / "dummy.txt"
        plain.write_bytes(b"non-sensitive fixture")
        cipher = self.private / "cbc.cms"
        ci.openssl([
            "cms", "-encrypt", "-binary", "-aes-256-cbc", "-in", str(plain),
            "-outform", "DER", "-out", str(cipher), str(self.cert)
        ])
        output = self.private / "must-not-exist.json"
        with patch.object(ci, "openssl", wraps=ci.openssl) as operations:
            with self.assertRaises(ValueError):
                self.decrypt(cipher, output)
        self.assertFalse(any("-decrypt" in call.args[0] for call in operations.call_args_list))
        self.assertFalse(output.exists())

    def test_encrypt_failure_cleans_plaintext_and_publishes_no_ciphertext(self):
        real_encrypt = ci.cms_encrypt
        calls = []
        def fail_second(plain, cipher, certificate):
            calls.append(plain)
            if len(calls) == 1:
                return real_encrypt(plain, cipher, certificate)
            cipher.write_bytes(b"partial cipher")
            raise ValueError(SENTINEL)
        with patch.object(ci, "cms_encrypt", side_effect=fail_second):
            code, report, _ = self.execute(self.raw())
        self.assertEqual(code, 1)
        self.assertEqual(report["executionStatus"], "review_rejected_or_encryption_failed")
        self.assertFalse((self.evidence / "comparison.cms").exists())

    def test_coordinator_can_check_dummy_recipient_without_paid_requests(self):
        fingerprint = ci.check_recipient(str(self.cert), str(self.key))
        self.assertRegex(fingerprint, "^[0-9a-f]{64}$")

    def test_provider_failure_drops_error_and_retains_pending(self):
        raw = self.raw()
        raw["cases"][0]["new"] = {"errorCode": SENTINEL}
        raw["transport"]["attemptedSends"] = 2
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
        self.assertEqual(report["transport"], {"state": "unknown"})

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
        self.assertIn("openless-intent-evidence/comparison.cms", live)
        self.assertNotIn("openless-intent-evidence/*.json", live)
        self.assertLess(live.index("preflight"), live.index("Build acceptance without credentials"))
        self.assertIn("INTENT_WORKFLOW_SHA: ${{ github.workflow_sha }}", live)
        self.assertIn("INTENT_TRIGGERING_ACTOR: ${{ github.triggering_actor }}", live)
        self.assertIn("intent_expected_source_sha", text)
        self.assertIn("intent_recipient_sha256", text)
        self.assertNotIn("intent_allowed_actor", text)
        self.assertLess(
            live.index("Verify intent mock transport gates"),
            live.index("Compare intent 1.3.0 and 1.5.0"),
        )
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
