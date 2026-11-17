"""Check temporary source harnesses honor the CI target without running native code."""
import os
from pathlib import Path
import runpy
import subprocess
import tempfile
import unittest
from unittest import mock

SCRIPTS = Path(__file__).resolve().parent


class HarnessCacheTests(unittest.TestCase):
    def run_harness(self, script, args=(), target=None):
        calls = []

        def cargo(command, **kwargs):
            env = kwargs.get("env", os.environ)
            calls.append((command, env.get("CARGO_TARGET_DIR")))
            if "owner_must_not_move" in command:
                return subprocess.CompletedProcess(command, 1, "", "E0277")
            if "borrow_must_not_escape" in command:
                return subprocess.CompletedProcess(command, 1, "", "lifetime may not live long enough")
            return subprocess.CompletedProcess(command, 0, "", "")

        with mock.patch.dict(os.environ), mock.patch("sys.argv", [script, *args]):
            if target is None:
                os.environ.pop("CARGO_TARGET_DIR", None)
            else:
                os.environ["CARGO_TARGET_DIR"] = str(target)
            with mock.patch("subprocess.run", side_effect=cargo):
                namespace = runpy.run_path(str(SCRIPTS / script), run_name="__main__")
        return calls, namespace

    def test_owner_harness_honors_shared_target_for_tests_and_compile_fail_checks(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory).resolve() / "host-target"
            calls, namespace = self.run_harness("test-windows-runtime-target.py", target=target)
            self.assertEqual(namespace["target"], target)
            self.assertEqual(len(calls), 3)
            self.assertTrue(all(value == str(target) for _, value in calls))
            self.assertIn("test", calls[0][0])
            self.assertTrue(all("check" in command for command, _ in calls[1:]))

    def test_owner_local_default_is_still_isolated(self):
        calls, namespace = self.run_harness("test-windows-runtime-target.py")
        expected = (SCRIPTS.parent / "src-tauri/target/ownership-harness").resolve()
        self.assertEqual(namespace["target"], expected)
        self.assertTrue(all(value == str(expected) for _, value in calls))

    def test_diagnostics_reuses_target_without_dropping_off_on_or_facade_coverage(self):
        with tempfile.TemporaryDirectory() as directory:
            calls, _ = self.run_harness("test-windows-hang-diagnostics.py", target=directory)
        self.assertEqual(len(calls), 4)
        self.assertTrue(all(value == directory for _, value in calls))
        self.assertEqual(sum("--features" in command for command, _ in calls), 2)
        self.assertEqual(sum("test" in command for command, _ in calls), 2)
        self.assertEqual(sum("run" in command for command, _ in calls), 2)

    def test_recorder_preserves_target_and_test_or_cross_check_mode(self):
        for args in [(), ("--check", "--target", "x86_64-pc-windows-msvc")]:
            with self.subTest(args=args), tempfile.TemporaryDirectory() as directory:
                calls, _ = self.run_harness("test-recorder-module.py", args, directory)
                self.assertEqual(len(calls), 1)
                command, target = calls[0]
                self.assertEqual(target, directory)
                self.assertEqual(command[1], "check" if args else "test")
                if args:
                    self.assertIn("--tests", command)
                    self.assertIn("x86_64-pc-windows-msvc", command)


if __name__ == "__main__":
    unittest.main()
