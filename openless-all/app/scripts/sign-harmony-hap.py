"""Sign a HarmonyOS HAP with material that never enters the repository.

Usage:
    sign-harmony-hap.py TOOLCHAIN SIGNING_DIR OUTPUT_DIR [--release] [--prefix NAME]

Signing material layout inside SIGNING_DIR:

    debug.cer / debug.p7b / debug.p12 / keystore-password      # local debug builds
    release.cer / release.p7b / release.p12 / keystore-password # AppGallery production
    test.cer / test.p7b / test.p12 / keystore-password          # AppGallery test channel

`--prefix` selects the material explicitly, which is how a test-channel build is
signed with the same script.

The passwords may also come from the environment (`HARMONY_KEYSTORE_PASSWORD`,
`HARMONY_KEY_PASSWORD`), which is what CI uses: the files are written from
base64 secrets at job time and never committed or cached.
"""
from pathlib import Path
import os
import subprocess
import sys

release = '--release' in sys.argv[1:]
prefix_override = ''
skip_hap = False
args = []
rest = sys.argv[1:]
while rest:
    argument = rest.pop(0)
    if argument == '--prefix':
        if not rest:
            sys.exit('--prefix needs a value')
        prefix_override = rest.pop(0)
    elif argument == '--skip-hap':
        # The second pass only signs the App Pack in its own output directory.
        skip_hap = True
    elif argument.startswith('--'):
        continue
    else:
        args.append(argument)
if len(args) != 3:
    sys.exit(__doc__)
toolchain, signing, output = map(Path, args)

prefix = prefix_override or ('release' if release else 'debug')
key_alias = os.environ.get('HARMONY_KEY_ALIAS') or f'harmony-{prefix}'

password = os.environ.get('HARMONY_KEYSTORE_PASSWORD', '').strip()
if not password:
    password_file = signing / 'keystore-password'
    if not password_file.is_file():
        sys.exit(f'missing signing password: set HARMONY_KEYSTORE_PASSWORD or {password_file}')
    password = password_file.read_text().strip()
key_password = os.environ.get('HARMONY_KEY_PASSWORD', '').strip()
if not key_password:
    key_password_file = signing / 'key-password'
    if key_password_file.is_file():
        key_password = key_password_file.read_text().strip()
key_password = key_password or password

required = [signing / f'{prefix}.cer', signing / f'{prefix}.p7b', signing / f'{prefix}.p12']
missing = [str(path) for path in required if not path.is_file()]
if missing:
    sys.exit(f'missing HarmonyOS signing material: {", ".join(missing)}')

jar = toolchain / 'sdk/default/openharmony/toolchains/lib/hap-sign-tool.jar'
if not jar.is_file():
    sys.exit(f'missing hap-sign-tool.jar under {toolchain}')

# `hap-sign-tool sign-app` also signs an App Pack (`.app`), which is what
# AppGallery accepts: a bare HAP is rejected with 204144641 "The suffix name of
# the software package is incorrect". Both inputs are signed the same way.
hap_candidates = [] if skip_hap else sorted(output.glob('*-unsigned.hap'))
app_candidates = sorted(output.glob('*-unsigned.app'))
if not skip_hap and len(hap_candidates) != 1:
    sys.exit(f'expected exactly one unsigned HAP in {output}, found {len(hap_candidates)}')
if skip_hap and not app_candidates:
    sys.exit(f'expected an unsigned App Pack in {output}')
def sign_package(source, target):
    """`hap-sign-tool sign-app` handles both a `.hap` and an `.app`."""
    if target.exists():
        target.unlink()
    argv = ['java', '-jar', str(jar), 'sign-app', '-mode', 'localSign',
            '-keyAlias', key_alias,
            '-appCertFile', str(signing / f'{prefix}.cer'),
            '-profileFile', str(signing / f'{prefix}.p7b'),
            '-inFile', str(source),
            '-signAlg', 'SHA256withECDSA',
            '-keystoreFile', str(signing / f'{prefix}.p12'),
            '-keystorePwd', password, '-keyPwd', key_password,
            '-outFile', str(target),
            '-compatibleVersion', '12', '-signCode', '1']
    result = subprocess.run(argv, capture_output=True, text=True)
    # Never echo the passwords back, even on failure.
    print((result.stdout + result.stderr).replace(password, '[redacted]').replace(key_password, '[redacted]'))
    if result.returncode:
        sys.exit(result.returncode)
    print('Signed:', target)


# Keep the name the build scripts already reference; `--release` only swaps the
# signing material, not the artifact name.
if hap_candidates:
    signed = output / 'entry-default-signed.hap'
    sign_package(hap_candidates[0], signed)

for candidate in app_candidates:
    app_signed = candidate.with_name(candidate.name.replace('-unsigned', '-signed'))
    sign_package(candidate, app_signed)
