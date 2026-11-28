#!/usr/bin/env python3
"""Report whether a HarmonyOS profile matches the application being packaged.

    harmony-profile-report.py <profile.json> <app.json5> <channel>

`openssl cms -verify -noverify` (or `hap-sign-tool verify-profile`) gives the
profile as JSON. What matters before a release:

  * `bundle-info.bundle-name` equals the `bundleName` in AppScope/app.json5
  * `type` matches the channel - a `release` dispatch needs a release profile
  * `validity` covers today
  * a debug profile lists the device UDIDs it may be installed on

Exits 1 when a check failed.
"""
import json
import re
import sys
import time


def main():
    profile_path, app_json5_path, channel = sys.argv[1:4]
    failures = 0

    def ok(message):
        print(f'  ok    {message}')

    def bad(message):
        nonlocal failures
        failures += 1
        print(f'  FAIL  {message}', file=sys.stderr)

    def warn(message):
        print(f'  warn  {message}')

    profile = json.load(open(profile_path, encoding='utf-8'))
    bundle_info = profile.get('bundle-info', {})
    bundle = bundle_info.get('bundle-name', '')

    source = open(app_json5_path, encoding='utf-8').read()
    expected = re.search(r'"bundleName"\s*:\s*"([^"]+)"', source)
    expected = expected.group(1) if expected else ''

    if expected and bundle == expected:
        ok(f'profile bundleName matches app.json5 ({bundle})')
    elif expected:
        bad(f'profile is for "{bundle}", app.json5 says "{expected}"')
    else:
        warn('app.json5 bundleName not found; profile not compared')

    profile_type = profile.get('type', '')
    if channel == 'release' and profile_type != 'release':
        bad(f'profile type is "{profile_type}", a release dispatch needs a release profile')
    elif channel == 'debug' and profile_type != 'debug':
        warn(f'profile type is "{profile_type}", debug signing usually uses a debug profile')
    else:
        ok(f'profile type is "{profile_type}"')

    validity = profile.get('validity', {})
    start = int(validity.get('not-before', 0) or 0)
    end = int(validity.get('not-after', 0) or 0)
    now = int(time.time())
    stamp = lambda value: time.strftime('%Y-%m-%d', time.gmtime(value))  # noqa: E731
    if start and end:
        if start <= now <= end:
            ok(f'profile valid {stamp(start)} → {stamp(end)}')
        else:
            bad(f'profile outside its window: {stamp(start)} → {stamp(end)}')
    else:
        warn('profile has no validity window')

    devices = profile.get('debug-info', {}).get('device-ids') or []
    if profile_type == 'debug':
        if devices:
            ok(f'{len(devices)} device UDID(s) authorised for this debug profile')
        else:
            bad('debug profile lists no device UDID; installation will be refused')

    version_name = profile.get('version-name')
    print(f'  note  profile versionName is {version_name!r}; the packager stamps the HAP'
          ' from package.json, so keep them compatible when AppGallery rejects a build')

    return failures


if __name__ == '__main__':
    sys.exit(1 if main() else 0)
