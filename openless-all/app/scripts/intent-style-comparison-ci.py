"""Opt-in CI runner. Review text leaves the runner only in authenticated CMS."""
import base64
import copy
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from urllib.parse import quote

TARGET = "intent_style_tests::intent_live_configured_channel_comparison"
MODEL = "deepseek-flash"
ENDPOINT = "https://api.deepseek.com/v1"
MAX_REPORT = 2_000_000
MAX_REVIEW = 1_000_000
MAX_TEXT = 32_768
REPOSITORY = "Open-Less/openless"
AUTHORIZED_ACTOR = "appergb"
AUTHORIZED_ACTOR_ID = "192432815"
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
        "transport": {"state": "not_started", "attemptedSends": 0, "fallbackSends": 0},
        "semanticReview": "pending human review; outputs are untrusted text",
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


def validate_dispatch():
    expected = os.environ.get("INTENT_EXPECTED_SOURCE_SHA", "")
    ref = os.environ.get("GITHUB_REF", "")
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    if (
        not re.fullmatch("[0-9a-f]{40}", expected)
        or os.environ.get("GITHUB_EVENT_NAME") != "workflow_dispatch"
        or os.environ.get("GITHUB_REPOSITORY") != REPOSITORY
        or ref not in ("refs/heads/main", "refs/heads/beta")
        or os.environ.get("GITHUB_SHA") != expected
        or os.environ.get("INTENT_WORKFLOW_SHA") != expected
        or os.environ.get("GITHUB_WORKFLOW_REF") != f"{REPOSITORY}/.github/workflows/ci.yml@{ref}"
        or os.environ.get("GITHUB_ACTOR") != AUTHORIZED_ACTOR
        or os.environ.get("GITHUB_ACTOR_ID") != AUTHORIZED_ACTOR_ID
        or os.environ.get("INTENT_TRIGGERING_ACTOR") != AUTHORIZED_ACTOR
        or os.environ.get("GITHUB_RUN_ATTEMPT") != "1"
        or not re.fullmatch("[1-9][0-9]{0,19}", run_id)
    ):
        raise ValueError("unverified dispatch")
    checkout = subprocess.run(
        ["git", "rev-parse", "HEAD"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        timeout=10, check=False,
        env={name: os.environ[name] for name in ("PATH", "HOME", "TMPDIR", "LANG")
             if name in os.environ},
    )
    if checkout.returncode != 0 or checkout.stdout.decode("ascii").strip() != expected:
        raise ValueError("unverified checkout")
    return {
        "sourceSha": expected, "workflowSha": expected,
        "workflowRef": os.environ["GITHUB_WORKFLOW_REF"],
        "runId": run_id, "runAttempt": 1,
        "actor": AUTHORIZED_ACTOR, "actorId": AUTHORIZED_ACTOR_ID,
    }


def intent_preflight(directory):
    verified = validate_dispatch()
    expected = os.environ.get("INTENT_RECIPIENT_SHA256", "")
    if not re.fullmatch("[0-9a-f]{64}", expected):
        raise ValueError("unconfirmed recipient")
    certificate, actual = validate_recipient(
        os.environ.get("INTENT_RECIPIENT_CERTIFICATE", ""), directory
    )
    if not hmac.compare_digest(expected, actual):
        raise ValueError("recipient mismatch")
    verified["recipientSha256"] = actual
    return certificate, verified


def apply_transport(report, raw, returncode):
    report["transport"] = {"state": "unknown"}
    counts = raw.get("transport", {})
    sends, fallbacks = counts.get("attemptedSends"), counts.get("fallbackSends")
    if (
        returncode < 0 or counts.get("state") != "final"
        or type(sends) is not int or not 0 <= sends <= 36
        or type(fallbacks) is not int or not 0 <= fallbacks <= min(4, sends)
    ):
        raise ValueError("unknown transport count")
    report["transport"] = {
        "state": "final", "attemptedSends": sends, "fallbackSends": fallbacks,
    }

def openssl(arguments, data=None):
    result = subprocess.run(
        ["openssl", *arguments], input=data, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, timeout=30, check=False,
        env={name: os.environ[name] for name in ("PATH", "HOME", "TMPDIR", "LANG")
             if name in os.environ},
    )
    if result.returncode != 0:
        raise ValueError("cryptographic operation failed")
    return result.stdout


def private_write(path, data):
    with open(path, "xb", opener=lambda name, flags: os.open(name, flags, 0o600)) as stream:
        stream.write(data)


def cms_encrypt(plaintext, ciphertext, certificate):
    openssl([
        "cms", "-encrypt", "-binary", "-aes-256-gcm",
        "-in", str(plaintext), "-outform", "DER", "-out", str(ciphertext),
        "-recip", str(certificate), "-keyopt", "rsa_padding_mode:oaep",
        "-keyopt", "rsa_oaep_md:sha256", "-keyopt", "rsa_mgf1_md:sha256",
    ])
    validate_ciphertext(ciphertext)


def validate_ciphertext(ciphertext):
    # Both sides reject unauthenticated CBC, even for an otherwise valid CMS file.
    if not 128 <= ciphertext.stat().st_size <= MAX_REPORT:
        raise ValueError("unexpected ciphertext size")
    structure = openssl(["cms", "-cmsout", "-inform", "DER", "-in", str(ciphertext), "-print"])
    for field in (
        rb"^  contentType: id-smime-ct-authEnvelopedData \(",
        rb"^\s+algorithm: aes-256-gcm \(",
        rb"^\s+algorithm: rsaesOaep \(",
    ):
        if not re.search(field, structure, re.M):
            raise ValueError("authenticated CMS required")


def validate_recipient(certificate, directory):
    if (
        not isinstance(certificate, str) or len(certificate) > 8192
        or not re.fullmatch(
            r"\s*-----BEGIN CERTIFICATE-----\r?\n[A-Za-z0-9+/=\r\n]+"
            r"-----END CERTIFICATE-----\s*", certificate
        )
    ):
        raise ValueError("one public PEM certificate required")
    path = directory / "recipient.pem"
    private_write(path, certificate.encode("ascii"))
    der = openssl(["x509", "-in", str(path), "-outform", "DER"])
    openssl(["x509", "-in", str(path), "-checkend", "3600", "-noout"])
    public = openssl(["x509", "-in", str(path), "-pubkey", "-noout"])
    details = openssl(["rsa", "-pubin", "-text", "-noout"], public)
    bits = re.search(rb"Public-Key: \((\d+) bit\)", details)
    if not bits or not 3072 <= int(bits[1]) <= 8192:
        raise ValueError("RSA 3072..8192 recipient required")
    # Prove that this installed OpenSSL supports the exact authenticated format
    # before the caller reads provider credentials or starts a paid request.
    probe = directory / "probe.json"
    encrypted = directory / "probe.cms"
    private_write(probe, b'{"preflight":"non-sensitive fixture"}')
    cms_encrypt(probe, encrypted, path)
    probe.unlink()
    encrypted.unlink()
    return path, hashlib.sha256(der).hexdigest()


def review_text(text, key, allowed_urls):
    if not isinstance(text, str) or len(text.encode("utf-8")) > MAX_TEXT:
        raise ValueError("invalid review text")
    if any(ord(char) < 32 and char not in "\r\n\t" for char in text):
        raise ValueError("unexpected control character")
    variants = {
        key, quote(key, safe=""), key.encode().hex(),
        base64.b64encode(key.encode()).decode(),
        base64.urlsafe_b64encode(key.encode()).decode(),
        json.dumps(key)[1:-1],
    }
    if any(value in text for value in variants):
        raise ValueError("credential echo rejected")
    if re.search(
        r"-----BEGIN (?:[A-Z ]*PRIVATE KEY|CERTIFICATE)-----"
        r"|authorization\s*:\s*bearer|(?:api[_ -]?key|token)\s*[:=]"
        r"|\bsk-[A-Za-z0-9_-]{16,}", text, re.I
    ):
        raise ValueError("credential-like content rejected")
    if set(re.findall(r'https?://[^\s<>"`()，。；）]+', text)) - allowed_urls:
        raise ValueError("unexpected URL")
    return text


def review_payload(report, raw, key):
    payload = copy.deepcopy(report)
    for target, canonical, case in zip(payload["cases"], report["cases"], raw["cases"]):
        allowed_urls = set(re.findall(r'https?://[^\s<>"`()，。；）]+', canonical["input"]))
        for side in ("old", "new"):
            value = case.get(side)
            if value is None:
                target[side] = None
            elif "errorCode" in value:
                target[side] = {"errorCode": "request_failed"}
            else:
                selected = {"text": review_text(value["text"], key, allowed_urls)}
                source = value.get("sourceText")
                if source is not None:
                    selected["sourceText"] = review_text(source, key, allowed_urls)
                elapsed = canonical[side].get("elapsedMs")
                if elapsed is not None:
                    selected["elapsedMs"] = elapsed
                target[side] = selected
    encoded = (json.dumps(payload, ensure_ascii=False, indent=2) + "\n").encode()
    if len(encoded) > MAX_REVIEW:
        raise ValueError("oversize review")
    return encoded


def decrypt_review(ciphertext, certificate, private_key, output, expected_sha, run_id, fingerprint):
    """Local explicit command only. Never writes unauthenticated plaintext."""
    ciphertext = Path(ciphertext)
    if (
        not re.fullmatch("[0-9a-f]{40}", expected_sha)
        or not re.fullmatch("[1-9][0-9]{0,19}", run_id)
        or not re.fullmatch("[0-9a-f]{64}", fingerprint)
    ):
        raise ValueError("receipt binding required")
    actual = hashlib.sha256(openssl(["x509", "-in", certificate, "-outform", "DER"])).hexdigest()
    if not hmac.compare_digest(fingerprint, actual):
        raise ValueError("wrong local recipient")
    validate_ciphertext(ciphertext)
    plaintext = openssl([
        "cms", "-decrypt", "-binary", "-inform", "DER", "-in", str(ciphertext),
        "-recip", certificate, "-inkey", private_key,
    ])
    if len(plaintext) > MAX_REVIEW:
        raise ValueError("oversize review")
    payload = json.loads(plaintext)
    if (
        payload.get("sourceSha") != expected_sha or payload.get("workflowSha") != expected_sha
        or payload.get("runId") != run_id or type(payload.get("runAttempt")) is not int
        or payload["runAttempt"] != 1 or payload.get("recipientSha256") != fingerprint
        or payload.get("actor") != AUTHORIZED_ACTOR
        or payload.get("actorId") != AUTHORIZED_ACTOR_ID
        or payload.get("workflowRef") not in {
            f"{REPOSITORY}/.github/workflows/ci.yml@refs/heads/main",
            f"{REPOSITORY}/.github/workflows/ci.yml@refs/heads/beta",
        }
    ):
        raise ValueError("wrong evidence origin")
    canonical = safe_report(json.loads(FIXTURE.read_text()))
    apply_status(canonical, payload)
    apply_transport(canonical, payload, 0)
    # Keep the plaintext out of stdout; the original renderer reads this private file.
    private_write(Path(output), plaintext)


def check_recipient(certificate, private_key):
    """Coordinator-only local receipt check, with fixed non-sensitive bytes."""
    with tempfile.TemporaryDirectory(prefix="openless-local-recipient-") as scratch:
        directory = Path(scratch)
        recipient, fingerprint = validate_recipient(Path(certificate).read_text(), directory)
        plaintext = directory / "local-check.txt"
        ciphertext = directory / "local-check.cms"
        expected = b"OpenLess one-time recipient possession check"
        private_write(plaintext, expected)
        cms_encrypt(plaintext, ciphertext, recipient)
        actual = openssl([
            "cms", "-decrypt", "-binary", "-inform", "DER", "-in", str(ciphertext),
            "-recip", str(recipient), "-inkey", private_key,
        ])
        if actual != expected:
            raise ValueError("recipient possession check failed")
        return fingerprint


def run(binary, private_directory, evidence_directory):
    report = safe_report(json.loads(FIXTURE.read_text()))
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
    ciphertext = evidence_directory / "comparison.cms"
    try:
        try:
            recipient, verified = intent_preflight(private_directory)
            report.update(verified)
        except Exception:
            report["executionStatus"] = "dispatch_or_recipient_unverified"
            return 1
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
        report["transport"] = {"state": "unknown"}
        completed = subprocess.run(
            [binary, TARGET, "--ignored", "--exact", "--test-threads=1"],
            env=environment,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=900,
            check=False,
        )
        if not raw_output.is_file() or raw_output.stat().st_size > MAX_REPORT:
            report["executionStatus"] = "missing_or_invalid_report"
            return result
        raw = json.loads(raw_output.read_text())
        apply_status(report, raw)
        apply_transport(report, raw, completed.returncode)
        all_completed = all(
            case[side]["status"] == "completed"
            for case in report["cases"]
            for side in ("old", "new")
        )
        result = 0 if completed.returncode == 0 and all_completed else 1
        report["executionStatus"] = "completed" if result == 0 else "test_failed"
        try:
            plaintext = private_directory / "review.json"
            private_write(plaintext, review_payload(report, raw, key))
            encrypted = private_directory / "comparison.cms"
            cms_encrypt(plaintext, encrypted, recipient)
            encrypted.replace(ciphertext)
            report["encryptedReview"] = "available"
        except Exception:
            report["executionStatus"] = "review_rejected_or_encryption_failed"
            return 1
        return result
    except subprocess.TimeoutExpired:
        report["executionStatus"] = "timed_out"
        return 1
    except Exception:
        # No repr/traceback: even parser and filesystem errors can contain secrets.
        report["executionStatus"] = "execution_or_report_failed"
        return 1
    finally:
        for name in (
            "channel.json", "results.json", "review.json", "comparison.cms",
            "recipient.pem", "probe.json", "probe.cms",
        ):
            (private_directory / name).unlink(missing_ok=True)
        output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    try:
        if len(sys.argv) == 3 and sys.argv[1] == "select-binary":
            select_binary(sys.argv[2])
        elif len(sys.argv) == 2 and sys.argv[1] == "preflight":
            with tempfile.TemporaryDirectory(prefix="openless-intent-recipient-") as scratch:
                intent_preflight(Path(scratch))
        elif len(sys.argv) == 9 and sys.argv[1] == "decrypt":
            decrypt_review(*sys.argv[2:])
        elif len(sys.argv) == 4 and sys.argv[1] == "check-recipient":
            print("recipientSha256=" + check_recipient(*sys.argv[2:]))
        elif len(sys.argv) == 5 and sys.argv[1] == "run":
            sys.exit(run(*sys.argv[2:]))
        else:
            sys.exit(2)
    except Exception:
        # Inputs in this wrapper are never printed, including exception messages.
        print("Intent comparison wrapper failed; no private diagnostic was published.")
        sys.exit(1)
