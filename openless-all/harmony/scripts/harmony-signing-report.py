#!/usr/bin/env python3
"""Report whether a Huawei certificate chain belongs to a keystore key.

    harmony-signing-report.py <chain.cer> <keystore.pem> <profile.p7b>

`release.cer` / `debug.cer` downloaded from AppGallery Connect are PEM chains
(Huawei root CA + leaf), so the certificate that matters is the one whose public
key equals the keystore's - not the first certificate in the file, which is the
issuing CA. The profile (`.p7b`) embeds the certificate it was issued for; that
is compared verbatim (whitespace-insensitive) against the leaf.

Prints `ok`/`warn`/`FAIL` lines for the caller and exits 1 when a check failed.
Nothing secret is printed.
"""
import json
import re
import subprocess
import sys


def run(args, stdin=None):
    return subprocess.run(args, input=stdin, capture_output=True)


def pem_blocks(raw):
    blocks = re.findall(rb'-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----', raw, re.S)
    if blocks:
        return blocks
    # A single DER certificate.
    converted = run(['openssl', 'x509', '-inform', 'DER', '-outform', 'PEM'], raw).stdout
    return [converted] if converted else []


def describe(pem):
    out = run(['openssl', 'x509', '-noout', '-subject'], pem).stdout.decode().strip()
    return out.removeprefix('subject=').strip()


def pubkey(pem):
    return run(['openssl', 'x509', '-noout', '-pubkey'], pem).stdout


def normalise(text):
    return re.sub(r'\s+', '', text)


def main():
    chain_path, keystore_path, profile_path = sys.argv[1:4]
    blanks = []
    failures = 0

    def ok(message):
        print(f'  ok    {message}')

    def bad(message):
        nonlocal failures
        failures += 1
        print(f'  FAIL  {message}', file=sys.stderr)

    with open(chain_path, 'rb') as handle:
        blocks = pem_blocks(handle.read())
    if not blocks:
        bad(f'cannot read any certificate from {chain_path}')
        return 1

    with open(keystore_path, 'rb') as handle:
        keystore_pem = handle.read()
    keystore_key = pubkey(keystore_pem)

    leaf = next((pem for pem in blocks if pubkey(pem) == keystore_key), None)
    if not leaf:
        subjects = '; '.join(describe(pem) for pem in blocks) or 'unreadable'
        bad(f'no certificate in the chain belongs to the keystore key (chain: {subjects})')
        return failures

    ok(f'certificate matches the keystore key ({describe(leaf)})')
    if len(blocks) > 1:
        ok(f'chain carries {len(blocks) - 1} issuing certificate(s)')
    if blanks:
        ok(blanks[0])

    end = run(['openssl', 'x509', '-noout', '-enddate'], leaf).stdout.decode().strip()
    end = end.removeprefix('notAfter=').removeprefix('enddate=').strip()
    if run(['openssl', 'x509', '-noout', '-checkend', '0'], leaf).returncode == 0:
        ok(f'certificate valid until {end}')
    else:
        bad(f'certificate expired on {end}')

    # The profile from `hap-sign-tool`/`openssl cms` is JSON; a profile shipped
    # next to the material is already unwrapped.
    issued = ''
    try:
        if profile_path.endswith('.json'):
            profile = json.load(open(profile_path, encoding='utf-8'))
        else:
            unwrapped = run(
                ['openssl', 'cms', '-verify', '-noverify', '-inform', 'DER', '-in', profile_path]
            ).stdout
            profile = json.loads(unwrapped)
        bundle_info = profile.get('bundle-info', {})
        issued = (
            bundle_info.get('distribution-certificate')
            or bundle_info.get('development-certificate')
            or ''
        )
    except Exception as error:  # noqa: BLE001 - report, never raise
        print(f'  warn  could not read the profile for comparison ({error})')

    if not issued:
        print('  warn  profile embeds no certificate; cannot compare')
    elif normalise(issued) == normalise(leaf.decode()):
        ok('profile was issued for this certificate')
    else:
        bad('profile was issued for a different certificate; re-apply the profile')

    return failures


if __name__ == '__main__':
    sys.exit(1 if main() else 0)
