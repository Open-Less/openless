// 开发/自动化验收专用：OpenLess 正式产品默认最小化到托盘，
// 但端到端测试需要一个可被系统自动化识别的主窗口。
import { spawn } from 'node:child_process';

const command = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const child = spawn(command, ['run', 'tauri', '--', 'dev'], {
  stdio: 'inherit',
  shell: process.platform === 'win32',
  env: { ...process.env, OPENLESS_SHOW_MAIN_ON_START: '1' },
});

child.on('exit', code => process.exit(code ?? 1));
