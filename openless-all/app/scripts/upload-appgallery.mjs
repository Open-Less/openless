#!/usr/bin/env node
// Upload a HarmonyOS HAP to HUAWEI AppGallery Connect (Publishing API v2/v3).
//
//   node scripts/upload-appgallery.mjs --file path/to/hap [--channel release|test]
//        [--release-type N] [--release-phase N] [--dry-run] [--skip-if-unconfigured]
//
// Environment (GitHub Secrets in CI, never the repository):
//   APPGALLERY_CLIENT_ID      AGC API client id
//   APPGALLERY_CLIENT_SECRET  AGC API client secret
//   APPGALLERY_APP_ID         AGC app id of the HarmonyOS app
//
// Flow (matches the documented Publishing API sequence):
//   1. POST /api/oauth2/v1/token                      -> access_token
//   2. GET  /api/publish/v2/upload-url/for-obs        -> objectId + upload url/headers
//   3. PUT  <upload url>                              -> file bytes
//   4. PUT  /api/publish/v3/app-package-info          -> bind the object to the app
//
// The app is *not* submitted for review: step 4 only replaces the candidate
// package, so a release still needs the human submission that AGC requires.
import { createHash } from 'node:crypto';
import { readFileSync, statSync } from 'node:fs';
import { basename } from 'node:path';

const ORIGIN = 'https://connect-api.cloud.huawei.com';
const args = process.argv.slice(2);
const value = (flag, fallback = '') => {
  const index = args.indexOf(flag);
  return index >= 0 && args[index + 1] ? args[index + 1] : fallback;
};
const file = value('--file');
const dryRun = args.includes('--dry-run');
const skipIfUnconfigured = args.includes('--skip-if-unconfigured');
// The GitHub Release ships the unsigned package, so only this store upload ever
// sees a signed HAP. `--channel test` is a label for the log plus the default
// release type used by the AppGallery test channel.
const channel = value('--channel', 'release');
const releaseType = Number(value('--release-type', channel === 'test' ? '1' : '1'));
const releasePhase = Number(value('--release-phase', '0'));

const clientId = process.env.APPGALLERY_CLIENT_ID ?? '';
const clientSecret = process.env.APPGALLERY_CLIENT_SECRET ?? '';
const appId = process.env.APPGALLERY_APP_ID ?? '';

const missing = [];
if (!clientId) missing.push('APPGALLERY_CLIENT_ID');
if (!clientSecret) missing.push('APPGALLERY_CLIENT_SECRET');
if (!appId) missing.push('APPGALLERY_APP_ID');
if (!file) missing.push('--file');

if (missing.length) {
  const message = `AppGallery upload needs ${missing.join(', ')}`;
  if (skipIfUnconfigured) {
    console.log(`[appgallery] skipped: ${message}`);
    process.exit(0);
  }
  console.error(`[appgallery] ${message}`);
  process.exit(2);
}

const size = statSync(file).size;
const fileName = basename(file);
const digest = createHash('sha256').update(readFileSync(file)).digest('hex');
console.log(`[appgallery] ${fileName} ${size} bytes sha256=${digest} channel=${channel}`);

if (dryRun) {
  console.log(`[appgallery] dry run: would upload to app ${appId} (releaseType=${releaseType}, releasePhase=${releasePhase})`);
  process.exit(0);
}

const request = async (path, init = {}) => {
  const response = await fetch(`${ORIGIN}${path}`, init);
  const text = await response.text();
  let body = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = null;
  }
  return { status: response.status, body, text };
};

const token = await request('/api/oauth2/v1/token', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json', 'cache-control': 'no-cache' },
  body: JSON.stringify({ grant_type: 'client_credentials', client_id: clientId, client_secret: clientSecret }),
});
const accessToken = token.body?.access_token;
if (!accessToken) {
  console.error(`[appgallery] token request failed: HTTP ${token.status} ${token.text.slice(0, 400)}`);
  process.exit(1);
}

const authHeaders = { Authorization: `Bearer ${accessToken}`, client_id: clientId };

const uploadUrl = await request(
  `/api/publish/v2/upload-url/for-obs?appId=${encodeURIComponent(appId)}&fileName=${encodeURIComponent(fileName)}&contentLength=${size}`,
  { headers: { ...authHeaders, 'Content-Type': 'application/json' } },
);
const urlInfo = uploadUrl.body?.urlInfo;
if (uploadUrl.body?.ret?.code !== 0 || !urlInfo?.url || !urlInfo?.objectId) {
  console.error(`[appgallery] upload-url failed: HTTP ${uploadUrl.status} ${uploadUrl.text.slice(0, 400)}`);
  process.exit(1);
}

const put = await fetch(urlInfo.url, {
  method: 'PUT',
  headers: { ...(urlInfo.headers ?? {}), 'Content-Length': String(size) },
  body: readFileSync(file),
});
if (!put.ok) {
  console.error(`[appgallery] object upload failed: HTTP ${put.status} ${(await put.text()).slice(0, 400)}`);
  process.exit(1);
}

const apply = await request(
  `/api/publish/v3/app-package-info?appId=${encodeURIComponent(appId)}&releaseType=${releaseType}&releasePhase=${releasePhase}`,
  {
    method: 'PUT',
    headers: { ...authHeaders, 'Content-Type': 'application/json' },
    body: JSON.stringify({ fileName, objectId: urlInfo.objectId }),
  },
);
if (apply.body?.ret?.code !== 0) {
  console.error(`[appgallery] app-package-info failed: HTTP ${apply.status} ${apply.text.slice(0, 400)}`);
  process.exit(1);
}

console.log(
  `[appgallery] uploaded ${fileName} to app ${appId} (${channel}); submit it for review in AppGallery Connect to publish.`,
);
