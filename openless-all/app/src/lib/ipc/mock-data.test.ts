import { mockSettings } from './mock-data';

// The system proxy toggle defaults to on, matching the backend serde default (issue #869).
if (mockSettings.useSystemProxy !== true) {
  throw new Error(
    `mockSettings.useSystemProxy must default to true, got ${mockSettings.useSystemProxy}`,
  );
}

console.log('mock-data.test.ts passed');
