#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cleanup=$(sed -n '/^          cleanup() {$/,/^          }$/p' ../../.github/workflows/ci.yml)
test -n "$cleanup"
work=$(mktemp -d)
trap 'rm -f "$work/adb" "$work/ready" "$work/emulator.log"; rmdir "$work"' EXIT
cat > "$work/adb" << 'ADB'
#!/usr/bin/env bash
if [[ "${STUCK_ADB:-}" == 1 ]]; then
  trap '' TERM
  exec sleep 10
fi
exit 0
ADB
chmod +x "$work/adb"

for scenario in normal stuck stuck-adb failure no-process; do
  expected=0
  if [[ "$scenario" == failure || "$scenario" == no-process ]]; then expected=7; fi
  rm -f "$work/ready"
  actual=0
  # The outer timeout also kills the fake emulator if the old cleanup hangs.
  timeout --kill-after=1s 3s bash -c '
    set -euo pipefail
    export PATH="$3:$PATH"
    if [[ "$4" == stuck-adb ]]; then export STUCK_ADB=1; fi
    timeout() {
      local args=("${@/10s/0.1s}")
      command timeout "${args[@]/5s/0.1s}"
    }
    log="$3/emulator.log"
    echo emulator-failure > "$log"
    emulator_pid=""
    if [[ "$4" != no-process ]]; then
      (
        if [[ "$4" != normal ]]; then trap "" TERM; fi
        echo ready > "$3/ready"
        while :; do command sleep 0.1; done
      ) &
      emulator_pid=$!
      until [[ -s "$3/ready" ]]; do command sleep 0.01; done
    fi
    # Exercise the real loop without waiting ten wall-clock seconds per case.
    sleep() { command sleep 0.01; }
    eval "$1"
    trap cleanup EXIT
    (exit "$2")
  ' bash "$cleanup" "$expected" "$work" "$scenario" || actual=$?
  if [[ "$actual" != "$expected" ]]; then
    echo "FAIL: $scenario cleanup returned $actual, expected $expected" >&2
    exit 1
  fi
  echo "ok: $scenario cleanup preserves exit status $expected"
done
