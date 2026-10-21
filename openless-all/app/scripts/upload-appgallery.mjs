#!/usr/bin/env node
// Upload a HarmonyOS HAP to HUAWEI AppGallery Connect (Publishing API v2/v3).
//
//   node scripts/upload-appgallery.mjs --file path/to/hap [--channel release|test]
//        [--release-type N] [--release-phase N] [--dry-run] [--skip-if-unconfigured]
//
// Environment (GitHub Secrets in CI, never the repository). Two credential
// styles are accepted:
//
//   A. API client (OAuth2 client credentials)
//        APPGALLERY_CLIENT_ID      AGC API client id
//        APPGALLERY_CLIENT_SECRET  AGC API client secret
//   B. Service account (the `*private.json` AGC hands out when you create one)
//        APPGALLERY_SERVICE_ACCOUNT_JSON   path to that file, or
//        APPGALLERY_SERVICE_ACCOUNT_BASE64 base64 of it
//      B wins when both are present: it is what AGC shows for new projects.
//
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
import { createHash, createSign } from 'node:crypto';
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

const serviceAccountPath = process.env.APPGALLERY_SERVICE_ACCOUNT_JSON ?? '';
const serviceAccountBase64 = process.env.APPGALLERY_SERVICE_ACCOUNT_BASE64 ?? '';
const hasServiceAccount = Boolean(serviceAccountPath || serviceAccountBase64);

const missing = [];
if (!hasServiceAccount && !clientId) missing.push('APPGALLERY_CLIENT_ID');
if (!hasServiceAccount && !clientSecret) missing.push('APPGALLERY_CLIENT_SECRET');
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
  const url = init.urlOverride ?? `${ORIGIN}${path}`;
  const response = await fetch(url, init);
  const text = await response.text();
  let body = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = null;
  }
  return { status: response.status, body, text };
};

// `headerFromServiceAccount` signs a JWT assertion with the RSA key AGC bundled
// into `*private.json`; `getAccessToken` then exchanges it at `token_uri`.
const loadServiceAccount = () => {
  const text = serviceAccountBase64
    ? Buffer.from(serviceAccountBase64, 'base64').toString('utf8')
    : readFileSync(serviceAccountPath, 'utf8');
  const account = JSON.parse(text);
  for (const field of ['key_id', 'sub_account', 'private_key', 'token_uri']) {
    if (!account[field]) {
      console.error(`[appgallery] service account file is missing ${field}`);
      process.exit(1);
    }
  }
  return account;
};

const base64url = (value) => Buffer.from(value).toString('base64url');

const serviceAccountAssertion = (account, audience) => {
  const now = Math.floor(Date.now() / 1000);
  const header = base64url(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid: account.key_id }));
  const payload = base64url(
    JSON.stringify({ iss: account.sub_account, aud: audience ?? account.token_uri, iat: now, exp: now + 3600 }),
  );
  const signer = createSign('RSA-SHA256');
  signer.update(`${header}.${payload}`);
  signer.end();
  const signature = signer.sign(account.private_key).toString('base64url');
  return `${header}.${payload}.${signature}`;
};

let accessToken;
let clientHeader;
if (hasServiceAccount) {
  const account = loadServiceAccount();
  clientHeader = account.sub_account;
  const assertion = serviceAccountAssertion(account);
  const token = await request('', {
    method: 'POST',
    urlOverride: account.token_uri,
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      grant_type: 'urn:ietf:params:oauth:grant-type:jwt-bearer',
      assertion,
    }).toString(),
  });
  accessToken = token.body?.access_token;
  if (!accessToken) {
    console.error(`[appgallery] service account token failed: HTTP ${token.status} ${token.text.slice(0, 400)}`);
    process.exit(1);
  }
  console.log(`[appgallery] authenticated with the service account for project ${account.project_id}`);
} else {
  clientHeader = clientId;
  const token = await request('/api/oauth2/v1/token', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'cache-control': 'no-cache' },
    body: JSON.stringify({ grant_type: 'client_credentials', client_id: clientId, client_secret: clientSecret }),
  });
  accessToken = token.body?.access_token;
  if (!accessToken) {
    console.error(`[appgallery] token request failed: HTTP ${token.status} ${token.text.slice(0, 400)}`);
    process.exit(1);
  }
}

const authHeaders = { Authorization: `Bearer ${accessToken}`, client_id: clientHeader };

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
