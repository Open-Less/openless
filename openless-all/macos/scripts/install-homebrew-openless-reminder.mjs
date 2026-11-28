import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export function installReminder(
  home = homedir(),
  source = dirname(fileURLToPath(import.meta.url)),
) {
  const directory = join(home, '.local/share/openless');
  mkdirSync(directory, { recursive: true });
  for (const name of ['homebrew-openless-notify.mjs', 'homebrew-openless-hook.zsh'])
    copyFileSync(join(source, name), join(directory, name));
  const startup = join(home, '.zshrc');
  const content = existsSync(startup) ? readFileSync(startup, 'utf8') : '';
  const marker = '# >>> OpenLess Homebrew update reminders >>>';
  if (!content.includes(marker)) {
    if (existsSync(startup)) {
      const backup = `${startup}.openless-before-${Date.now()}`;
      copyFileSync(startup, backup);
    }
    writeFileSync(
      startup,
      content +
        `\n${marker}\n[[ -f "$HOME/.local/share/openless/homebrew-openless-hook.zsh" ]] && source "$HOME/.local/share/openless/homebrew-openless-hook.zsh"\n# <<< OpenLess Homebrew update reminders <<<\n`,
    );
  }
  return directory;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  console.log(`OpenLess Homebrew 提醒已安装：${installReminder()}（新终端生效，无定时任务）`);
}
