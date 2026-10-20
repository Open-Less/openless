import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export function compareVersions(left, right) {
  const parse = (value) =>
    /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?$/.exec(value ?? '');
  const a = parse(left),
    b = parse(right);
  if (!a || !b) return null;
  for (let i = 1; i <= 3; i++) {
    if (BigInt(a[i]) !== BigInt(b[i])) return BigInt(a[i]) > BigInt(b[i]) ? 1 : -1;
  }
  if (!a[4] && b[4]) return 1;
  if (a[4] && !b[4]) return -1;
  const ap = a[4]?.split('.') ?? [],
    bp = b[4]?.split('.') ?? [];
  for (let i = 0; i < Math.max(ap.length, bp.length); i++) {
    if (ap[i] === bp[i]) continue;
    if (ap[i] === undefined) return -1;
    if (bp[i] === undefined) return 1;
    const an = /^\d+$/.test(ap[i]),
      bn = /^\d+$/.test(bp[i]);
    if (an && bn) {
      if (BigInt(ap[i]) === BigInt(bp[i])) continue;
      return BigInt(ap[i]) > BigInt(bp[i]) ? 1 : -1;
    }
    if (an !== bn) return an ? -1 : 1;
    return ap[i] > bp[i] ? 1 : -1;
  }
  // A dated repack may keep SemVer precedence; only a strictly newer date is an update.
  const ad = /^build\.(\d{8})(?:[.-]|$)/.exec(a[5] ?? '')?.[1];
  const bd = /^build\.(\d{8})(?:[.-]|$)/.exec(b[5] ?? '')?.[1];
  return ad && bd && ad !== bd ? (ad > bd ? 1 : -1) : 0;
}

export function checkHomebrewOpenless({ notify = false, home = homedir() } = {}) {
  const command = (exe, args) =>
    spawnSync(exe, args, {
      encoding: 'utf8',
      timeout: 30000,
      env: { ...process.env, HOMEBREW_NO_AUTO_UPDATE: '1', HOMEBREW_NO_ENV_HINTS: '1' },
    });
  const info = command('brew', ['info', '--cask', '--json=v2', 'open-less/openless/openless']);
  if (info.status !== 0) throw new Error(info.stderr?.trim() || 'Homebrew cask lookup failed');
  const cask = JSON.parse(info.stdout).casks?.find((item) => item.token === 'openless');
  if (!cask?.version) throw new Error('OpenLess cask version is unavailable');
  const app = ['/Applications/OpenLess.app', join(home, 'Applications/OpenLess.app')].find(
    existsSync,
  );
  const plist = app
    ? command('/usr/libexec/PlistBuddy', [
        '-c',
        'Print :CFBundleShortVersionString',
        join(app, 'Contents/Info.plist'),
      ])
    : null;
  const current = plist?.status === 0 ? plist.stdout.trim() : cask.installed;
  const comparison = current ? compareVersions(cask.version, current) : 1;
  const update = comparison !== null && comparison > 0;
  const message = update
    ? `OpenLess 可更新：${current ?? '未安装'} → ${cask.version}；运行 brew upgrade --cask open-less/openless/openless（未纳入 Homebrew 管理时先安装）。`
    : comparison === null
      ? `OpenLess 版本无法自动比较：本机 ${current}，Homebrew ${cask.version}。`
      : comparison < 0
        ? `OpenLess 本机 ${current} 新于 Homebrew 公开版 ${cask.version}，保留当前安装。`
        : `OpenLess 已是最新版本：${current}。`;
  console.log(message);
  if (notify && update && process.platform === 'darwin') {
    const statePath = join(home, '.cache/openless-homebrew/notification.json');
    let previous;
    try {
      previous = JSON.parse(readFileSync(statePath, 'utf8')).version;
    } catch {
      /* first check */
    }
    if (previous !== cask.version) {
      const result = command('/usr/bin/osascript', [
        '-e',
        'on run argv\n display notification (item 1 of argv) with title "OpenLess 更新"\nend run',
        message,
      ]);
      if (result.status === 0) {
        mkdirSync(dirname(statePath), { recursive: true });
        writeFileSync(statePath, JSON.stringify({ version: cask.version }) + '\n');
      }
    }
  }
  return { current, available: cask.version, update };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    checkHomebrewOpenless({ notify: process.argv.includes('--notify') });
  } catch (error) {
    console.error(`OpenLess 更新检查失败：${error.message}`);
    process.exitCode = 1;
  }
}
