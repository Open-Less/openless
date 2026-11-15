import packageJson from '../../package.json';

export const APP_VERSION = packageJson.version;
export const APP_VERSION_LABEL = `v${APP_VERSION}`;
// Development builds carry a `-dev.N` prerelease segment. They are installed manually
// and must not display the Beta badge or be treated as a Beta release.
export const IS_DEV_BUILD = /-dev(?:\.|$)/i.test(APP_VERSION);
// Is this build a Beta? Convention: a semver prerelease segment (contains `-`) = Beta,
// e.g. `1.2.24-1` / `1.2.24-2`; stable builds have no `-`, e.g. `1.2.23` / `1.2.24`.
// Used for conditional UI rendering — the Beta badge appears only on Beta builds.
export const IS_BETA_BUILD = APP_VERSION.includes('-') && !IS_DEV_BUILD;

export const isStableChannelSwitch = (currentVersion: string, targetVersion: string) =>
  currentVersion.includes('-') && !targetVersion.includes('-');
