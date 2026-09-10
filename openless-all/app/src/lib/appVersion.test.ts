import { en } from '../i18n/en';
import { ja } from '../i18n/ja';
import { ko } from '../i18n/ko';
import { zhCN } from '../i18n/zh-CN';
import { zhTW } from '../i18n/zh-TW';
import { isStableChannelSwitch } from './appVersion';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

assert(isStableChannelSwitch('1.3.18-Beta.7', '1.3.18'), 'Beta → Stable should be a channel switch');
assert(!isStableChannelSwitch('1.3.18-Beta.7', '1.3.18-Beta.8'), 'Beta → Beta should be a regular update');
assert(!isStableChannelSwitch('1.3.18', '1.3.19'), 'Stable → Stable should be a regular update');

for (const resources of [zhCN, zhTW, en, ja, ko]) {
  const description = resources.settings.about.updateDialog.stableChannelSwitch.desc;
  assert(description.includes('{{currentVersion}}'), 'switch copy should show the current version');
  assert(description.includes('{{version}}'), 'switch copy should show the target version');
}

console.log('appVersion tests passed');
