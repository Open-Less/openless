import { en } from '../i18n/en';
import { ja } from '../i18n/ja';
import { ko } from '../i18n/ko';
import { zhCN } from '../i18n/zh-CN';
import { zhTW } from '../i18n/zh-TW';
import { isAskAnythingAvailable, isStableChannelSwitch } from './appVersion';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

assert(
  isStableChannelSwitch('1.3.18-Beta.7', '1.3.18'),
  'Beta → Stable should be a channel switch',
);
assert(
  !isStableChannelSwitch('1.3.18-Beta.7', '1.3.18-Beta.8'),
  'Beta → Beta should be a regular update',
);
assert(!isStableChannelSwitch('1.3.18', '1.3.19'), 'Stable → Stable should be a regular update');

for (const version of ['2.0.3-dev.noask.1', '2.0.3-Beta.2+build.noask.20261123', '2.0.3', '2.0.3-Dev.1', '2.0.3-dev.1', '2.0.3-Beta.2+build.20261123']) {
  assert(isAskAnythingAvailable(version), `${version} must expose the reopened Ask feature`);
}

for (const resources of [zhCN, zhTW, en, ja, ko]) {
  const description = resources.settings.about.updateDialog.stableChannelSwitch.desc;
  assert(description.includes('{{currentVersion}}'), 'switch copy should show the current version');
  assert(description.includes('{{version}}'), 'switch copy should show the target version');
}

console.log('appVersion tests passed');
