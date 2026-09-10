import { mockSettings } from './mock-data';

// 系统代理开关默认开启，与后端 serde 默认值保持一致（issue #869）。
if (mockSettings.useSystemProxy !== true) {
  throw new Error(
    `mockSettings.useSystemProxy must default to true, got ${mockSettings.useSystemProxy}`,
  );
}

console.log('mock-data.test.ts passed');
