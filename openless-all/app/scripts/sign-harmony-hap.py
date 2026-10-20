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
args = []
rest = sys.argv[1:]
while rest:
    argument = rest.pop(0)
    if argument == '--prefix':
        if not rest:
            sys.exit('--prefix needs a value')
        prefix_override = rest.pop(0)
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
key_password = os.environ.get('HARMONY_KEY_PASSWORD', '').strip() or password

required = [signing / f'{prefix}.cer', signing / f'{prefix}.p7b', signing / f'{prefix}.p12']
missing = [str(path) for path in required if not path.is_file()]
if missing:
    sys.exit(f'missing HarmonyOS signing material: {", ".join(missing)}')

jar = toolchain / 'sdk/default/openharmony/toolchains/lib/hap-sign-tool.jar'
if not jar.is_file():
    sys.exit(f'missing hap-sign-tool.jar under {toolchain}')

candidates = sorted(output.glob('*-unsigned.hap'))
if len(candidates) != 1:
    sys.exit(f'expected exactly one unsigned HAP in {output}, found {len(candidates)}')
unsigned = candidates[0]
# Keep the name the build scripts already reference; `--release` only swaps the
# signing material, not the artifact name.
signed = output / 'entry-default-signed.hap'
if signed.exists():
    signed.unlink()

command = ['java', '-jar', str(jar), 'sign-app', '-mode', 'localSign',
           '-keyAlias', key_alias,
           '-appCertFile', str(signing / f'{prefix}.cer'),
           '-profileFile', str(signing / f'{prefix}.p7b'),
           '-inFile', str(unsigned),
           '-signAlg', 'SHA256withECDSA',
           '-keystoreFile', str(signing / f'{prefix}.p12'),
           '-keystorePwd', password, '-keyPwd', key_password,
           '-outFile', str(signed),
           '-compatibleVersion', '12', '-signCode', '1']
result = subprocess.run(command, capture_output=True, text=True)
# Never echo the passwords back, even on failure.
print((result.stdout + result.stderr).replace(password, '[redacted]').replace(key_password, '[redacted]'))
if result.returncode:
    sys.exit(result.returncode)
print('Signed HAP:', signed)
