#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import process from 'node:process';

const gradlePath = fileURLToPath(
  new URL('../src-tauri/gen/android/app/build.gradle.kts', import.meta.url),
);

export const SHIZUKU_API = 'implementation("dev.rikka.shizuku:api:13.1.5")';
export const SHIZUKU_PROVIDER = 'implementation("dev.rikka.shizuku:provider:13.1.5")';
export const AIDL_FEATURE = 'aidl = true';

function printHelp() {
  console.log(`Usage: node scripts/patch-android-shizuku-deps.mjs [options]

Inject Shizuku SDK dependencies and enable AIDL in generated app/build.gradle.kts.

Options:
  --dry-run   Print planned changes without writing
  --help      Show this help text
`);
}

function parseArgs(argv) {
  let dryRun = false;
  for (const arg of argv) {
    if (arg === '--help' || arg === '-h') {
      printHelp();
      process.exit(0);
    }
    if (arg === '--dry-run') {
      dryRun = true;
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }
  return { dryRun };
}

export function patchShizukuDependencies(gradleContent) {
  if (gradleContent.includes(SHIZUKU_API) && gradleContent.includes(SHIZUKU_PROVIDER)) {
    return { content: gradleContent, changed: false };
  }

  const dependenciesMatch = /^(\s*)dependencies\s*\{\s*$/m.exec(gradleContent);
  if (!dependenciesMatch) {
    throw new Error('dependencies block not found in Gradle file');
  }

  const indent = dependenciesMatch[1];
  const lines = [];
  if (!gradleContent.includes(SHIZUKU_API)) {
    lines.push(`${indent}    ${SHIZUKU_API}`);
  }
  if (!gradleContent.includes(SHIZUKU_PROVIDER)) {
    lines.push(`${indent}    ${SHIZUKU_PROVIDER}`);
  }
  const injection = `${dependenciesMatch[0]}\n${lines.join('\n')}`;
  const updated = gradleContent.replace(dependenciesMatch[0], injection);
  return { content: updated, changed: true };
}

export function patchAidlBuildFeature(gradleContent) {
  if (gradleContent.includes(AIDL_FEATURE)) {
    return { content: gradleContent, changed: false };
  }

  const buildFeaturesBlock = /(\s*)buildFeatures\s*\{([^}]*)\}/m.exec(gradleContent);
  if (buildFeaturesBlock) {
    const indent = buildFeaturesBlock[1];
    const inner = buildFeaturesBlock[2];
    const aidlAssignment = /\baidl\s*=\s*(true|false)/;
    const updatedInner = aidlAssignment.test(inner)
      ? inner.replace(aidlAssignment, AIDL_FEATURE)
      : `${inner}${inner.trim().length > 0 ? '\n' : ''}${indent}    ${AIDL_FEATURE}\n`;
    const replacement = `${indent}buildFeatures {${updatedInner}${indent}}`;
    const updated = gradleContent.replace(buildFeaturesBlock[0], replacement);
    return { content: updated, changed: updated !== gradleContent };
  }

  const androidBlock = /(\s*)android\s*\{/;
  const androidMatch = androidBlock.exec(gradleContent);
  if (!androidMatch) {
    throw new Error('android block not found in Gradle file');
  }
  const indent = androidMatch[1];
  const insertion =
    `${androidMatch[0]}\n${indent}    buildFeatures {\n${indent}        ${AIDL_FEATURE}\n${indent}    }`;
  const updated = gradleContent.replace(androidMatch[0], insertion);
  return { content: updated, changed: true };
}

export function patchShizukuGradle(gradleContent) {
  let content = gradleContent;
  let changed = false;

  const deps = patchShizukuDependencies(content);
  content = deps.content;
  changed = changed || deps.changed;

  const aidl = patchAidlBuildFeature(content);
  content = aidl.content;
  changed = changed || aidl.changed;

  return { content, changed };
}

function main() {
  const { dryRun } = parseArgs(process.argv.slice(2));

  if (!existsSync(gradlePath)) {
    throw new Error(
      `Generated Android Gradle file not found: ${gradlePath}\nRun "npm run tauri -- android init --ci" first.`,
    );
  }

  const existing = readFileSync(gradlePath, 'utf8');
  const result = patchShizukuGradle(existing);

  if (!result.changed) {
    console.log(`Shizuku dependencies and AIDL already present in ${gradlePath}; skipping patch.`);
    return;
  }

  if (dryRun) {
    console.log(`[dry-run] Would patch Shizuku Gradle config in ${gradlePath}`);
    return;
  }

  writeFileSync(gradlePath, result.content, 'utf8');
  console.log(`Patched Shizuku dependencies and AIDL build feature in ${gradlePath}`);
}

const isDirectRun = Boolean(
  process.argv[1]?.replace(/\\/g, '/').endsWith('patch-android-shizuku-deps.mjs'),
);
if (isDirectRun) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
}
