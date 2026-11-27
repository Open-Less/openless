#!/usr/bin/env python3
"""Run real released clients and the deployed v1 service, without production writes.

Implementation files are extracted byte-for-byte from Git objects to temporary
test projects. Only a test module is appended; no old codec/registry is patched.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "openless-all/app"
CORE = APP / "crates/openless-core"
SERVICE_REV = "09627c2b495ae918d850f459756cb92e986725f1"
RELEASES = {"v2.0.0-tauri": "b5e60db6ac010dfc5fdd8aae4db77e1e3a72ac7f",
            "v2.0.2-tauri": "962705d56984b4c267f450d016aa16b3a881e4b4",
            "v2.0.3-Beta.1-tauri": "872c3293a8c8b10760017459a280526f3ebdf625"}


def git(repo, *args):
    return subprocess.check_output(["git", "-C", str(repo), *args])


def extract(repo, rev, paths, target):
    names = git(repo, "ls-tree", "-r", "--name-only", rev, "--", *paths).decode().splitlines()
    if not names:
        raise RuntimeError(f"Missing source at {repo}:{rev}")
    # cat-file batches avoid one process per source file, and return raw bytes.
    objects = git(repo, "ls-tree", "-r", rev, "--", *paths).decode().splitlines()
    with subprocess.Popen(["git", "-C", str(repo), "cat-file", "--batch"], stdin=subprocess.PIPE, stdout=subprocess.PIPE) as proc:
        for entry in objects:
            header, name = entry.split("\t", 1)
            oid = header.split()[2]
            proc.stdin.write((oid + "\n").encode())
            proc.stdin.flush()
            size = int(proc.stdout.readline().split()[2])
            data = proc.stdout.read(size)
            assert proc.stdout.read(1) == b"\n"
            destination = target / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        proc.stdin.close()


def run(cwd, args, env):
    print("RUN", cwd.name, " ".join(args), flush=True)
    subprocess.run(args, cwd=cwd, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--service-repo", type=Path, required=True)
    parser.add_argument("--release", choices=RELEASES, action="append")
    args = parser.parse_args()
    releases = args.release or list(RELEASES)
    with tempfile.TemporaryDirectory(prefix="openless-v1-compat-") as temporary:
        temp = Path(temporary)
        server = temp / "service"
        extract(args.service_repo, SERVICE_REV, ["Cargo.toml", "Cargo.lock", "src", "migrations", "tests"], server)
        (server / "tests/v1_compat.rs").write_bytes((CORE / "tests/fixtures/v1-deployed-server.rs").read_bytes())
        env = dict(os.environ)
        # Build products are reusable test caches, never delivered snapshots.
        env.setdefault("CARGO_TARGET_DIR", str(APP / "target/v1-released"))
        for release in releases:
            assert git(ROOT, "rev-list", "-n", "1", release).decode().strip() == RELEASES[release]
            old = temp / release
            # Historical release tar trees retain their original app-local platform layout.
            extract(ROOT, RELEASES[release], ["openless-all/app/crates", "openless-all/app/linux-egui", "openless-all/app/native-capsule", "openless-all/app/harmony/rust", "openless-all/app/assets", "openless-all/app/contract", "openless-all/app/src/lib/ipc", "openless-all/app/Cargo.toml", "openless-all/app/Cargo.lock"], old)
            old_app = old / "openless-all/app"
            tests = old_app / "crates/openless-core/src/cloud_sync_e2ee_store/tests.rs"
            harness = CORE / "tests/fixtures/v1-released-client.rs"
            with tests.open("a") as stream:
                stream.write('\n#[path = ' + '"' + str(harness) + '"' + ']\nmod released_v1_contract;\n')
            expected_rejection = release == "v2.0.0-tauri"
            cases = [(False, False)] if expected_rejection else [(False, False), (True, False), (False, True), (True, True)]
            for alias, edited in cases:
                data = temp / f"{release}-alias{int(alias)}-edit{int(edited)}"
                data.mkdir()
                case_env = dict(env, OPENLESS_V1_COMPAT_DIR=str(data), OPENLESS_V1_COMPAT_ALIAS=str(int(alias)), OPENLESS_V1_COMPAT_EDIT=str(int(edited)), OPENLESS_V1_COMPAT_REJECT=str(int(expected_rejection)))
                current_env = dict(case_env, CARGO_TARGET_DIR=str(APP / "target"), OPENLESS_V1_COMPAT_PHASE="create")
                run(APP, ["cargo", "test", "--locked", "-p", "openless-core", "--lib", "v1_release_exchange_current", "--", "--nocapture"], current_env)
                run(server, ["cargo", "test", "--locked", "--test", "v1_compat", "--", "--nocapture"], case_env)
                run(old_app, ["cargo", "test", "--locked", "-p", "openless-core", "--lib", "v1_release_exchange_old", "--", "--nocapture"], case_env)
                if expected_rejection:
                    print("PASS expected incompatibility:", release, "cursorContextEnabled was Excluded", flush=True)
                    continue
                run(server, ["cargo", "test", "--locked", "--test", "v1_compat", "--", "--nocapture"], case_env)
                run(APP, ["cargo", "test", "--locked", "-p", "openless-core", "--lib", "v1_release_exchange_current", "--", "--nocapture"], dict(current_env, OPENLESS_V1_COMPAT_PHASE="receive"))
                print("PASS", release, "alias=", alias, "edited=", edited, flush=True)


if __name__ == "__main__":
    main()
