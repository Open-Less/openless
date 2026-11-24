#!/bin/sh
# Only started by an explicit module installation, in an owned process group.
# Keep the inherited installation-lock fd open until uv and its children exit.
set -u
parent="$1"
shift
stop_owned_group() {
  trap '' TERM INT HUP
  kill -KILL -- "-$$"
}
trap stop_owned_group TERM INT HUP
"$@" &
child="$!"
while kill -0 "$child" 2>/dev/null; do
  actual_parent=$(/bin/ps -o ppid= -p "$$" | /usr/bin/tr -d ' ')
  if [ "$actual_parent" != "$parent" ]; then
    stop_owned_group
  fi
  /bin/sleep 1
done
status=0
wait "$child" || status="$?"
exit "$status"
