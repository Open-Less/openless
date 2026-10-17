"""Sign with local developer material without storing passwords in the project."""
from pathlib import Path
import subprocess
import sys

toolchain, signing, output = map(Path, sys.argv[1:])
password = (signing / 'keystore-password').read_text().strip()
jar = toolchain / 'sdk/default/openharmony/toolchains/lib/hap-sign-tool.jar'
unsigned = output / 'entry-default-unsigned.hap'
signed = output / 'entry-default-signed.hap'
command = ['java', '-jar', str(jar), 'sign-app', '-mode', 'localSign',
           '-keyAlias', 'harmony-debug', '-appCertFile', str(signing / 'debug.cer'),
           '-profileFile', str(signing / 'debug.p7b'), '-inFile', str(unsigned),
           '-signAlg', 'SHA256withECDSA', '-keystoreFile', str(signing / 'debug.p12'),
           '-keystorePwd', password, '-keyPwd', password, '-outFile', str(signed),
           '-compatibleVersion', '12', '-signCode', '1']
result = subprocess.run(command, capture_output=True, text=True)
print((result.stdout + result.stderr).replace(password, '[redacted]'))
if result.returncode:
    sys.exit(result.returncode)
print('Signed HAP:', signed)
