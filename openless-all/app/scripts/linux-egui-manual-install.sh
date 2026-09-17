#!/usr/bin/env bash
# OpenLess Linux egui —— 手动安装（给装不了 deb/rpm 的发行版用）。
#
# 这个脚本随发布 zip 一起分发，和解压出来的散装文件同目录：
#   install.sh   本脚本
#   usr/...      安装树（与 deb 内的文件逐项一致）
#   SHA256SUMS   对上面散装文件重新计算的校验和
#
# 安装 = 把 usr/ 按正确权限铺到 /，再让 fcitx5 重新加载插件。
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
payload="$here/usr"

if [ ! -d "$payload" ]; then
    echo "找不到 $payload —— 请先解压整个 zip，再从解压目录里运行 ./install.sh" >&2
    exit 1
fi

if [ "$(id -u)" -ne 0 ]; then
    echo "需要 root 权限：请运行 sudo ./install.sh" >&2
    exit 1
fi

# 散装文件比 deb 更容易在下载/解压中途损坏，所以自带校验和就先验一遍。
if [ -s "$here/SHA256SUMS" ] && command -v sha256sum >/dev/null 2>&1; then
    if ! ( cd "$here" && sha256sum -c SHA256SUMS >/dev/null ); then
        echo "SHA256SUMS 校验失败：payload 已损坏，请重新下载 zip" >&2
        exit 1
    fi
fi

# 权限按用途给：可执行文件 0755，配置/桌面项/图标 0644。
while IFS= read -r -d '' file; do
    rel="${file#"$here"/}"
    case "$rel" in
        usr/bin/* | usr/lib/*/fcitx5/*.so) mode=755 ;;
        *) mode=644 ;;
    esac
    install -D -m "$mode" "$file" "/$rel"
done < <(find "$payload" -type f -print0)

echo "已安装 openless 与 fcitx5 插件到 /usr。"

# fcitx5 必须重载才能加载新插件，且要用用户会话里的 D-Bus controller Restart
# （立即返回）。**不要**用 `fcitx5 -r`：它会替换 daemon 并一直前台运行，脚本会卡死，
# 与 deb postinst 保持一致。
for bus in /run/user/[0-9]*/bus; do
    [ -S "$bus" ] || continue
    runtime_dir="${bus%/bus}"
    uid="${runtime_dir##*/}"
    [ "$uid" != 0 ] || continue
    user="$(getent passwd "$uid" | cut -d: -f1)"
    [ -n "$user" ] && timeout 5s runuser -u "$user" -- env \
        XDG_RUNTIME_DIR="$runtime_dir" \
        DBUS_SESSION_BUS_ADDRESS="unix:path=$bus" \
        dbus-send --session --dest=org.fcitx.Fcitx5 --type=method_call \
        /controller org.fcitx.Fcitx.Controller1.Restart >/dev/null 2>&1 || true
done

cat <<'NOTE'
完成。请退出 OpenLess 后重新启动；全局快捷键会在 fcitx5 重载后生效。
卸载：退出 OpenLess，删除下面这些文件，再重载 fcitx5：
  /usr/bin/openless
  /usr/lib/*/fcitx5/libopenless.so
  /usr/share/fcitx5/addon/openless.conf
  /usr/share/applications/openless.desktop
  /usr/share/metainfo/top.openless.OpenLess.metainfo.xml
  /usr/share/icons/hicolor/256x256/apps/openless.png
NOTE
