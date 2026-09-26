#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

// 把 ios/ 下的 Swift/资源模板复制进 gen/apple，并对 tauri ios init 生成的
// project.yml 做必要 patch，最后重跑 xcodegen。流程对齐 copy-android-scaffolding.mjs。

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const genAppleRoot = join(appRoot, 'src-tauri/gen/apple');
const projectYmlPath = join(genAppleRoot, 'project.yml');
const swiftRoot = join(appRoot, 'ios/swift');
const appSwiftDestRoot = join(genAppleRoot, 'Sources');
const keyboardSwiftDestRoot = join(genAppleRoot, 'KeyboardExtension');

// Xcode 27 的 iOS SDK 只接受 15.0+ 部署目标（swift-rs 同样把 Swift 编译目标钳到 15）。
const DEPLOYMENT_TARGET = '15.0';
const APP_GROUP = 'group.com.openless.app';

// 签名策略：未提供 Team ID 时禁用签名（模拟器构建不需要）；提供后走自动签名。
// 真机调试键盘扩展必须提供 OPENLESS_IOS_DEVELOPMENT_TEAM（付费开发者账号）。
const developmentTeam = process.env.OPENLESS_IOS_DEVELOPMENT_TEAM;

function signingSettingsLines(indent) {
  if (developmentTeam) {
    return [
      `${indent}DEVELOPMENT_TEAM: ${developmentTeam}`,
      `${indent}CODE_SIGN_STYLE: Automatic`,
    ];
  }
  return [
    `${indent}CODE_SIGNING_ALLOWED: NO`,
    `${indent}CODE_SIGNING_REQUIRES_TEAM: NO`,
  ];
}

const INFO_PROPERTIES_SNIPPET = `        LSRequiresIPhoneOS: true
        NSMicrophoneUsageDescription: OpenLess 需要使用麦克风进行语音输入转写。
        # iOS 26+ SDK 强制 Scene 生命周期：manifest 必须存在且 supportsMultipleScenes
        # 为 true，tao 的 AppDelegate 才会注册 configurationForConnectingSceneSession
        # 并把 TaoSceneDelegate 挂到 scene 上（tao view.rs multiple_scenes_enabled 门控）。
        UIApplicationSceneManifest:
          UIApplicationSupportsMultipleScenes: true
        UILaunchStoryboardName: LaunchScreen`;

function printHelp() {
  console.log(`Usage: node scripts/copy-ios-scaffolding.mjs [options]

Copy Swift scaffolding into gen/apple and patch project.yml after \`tauri ios init\`.

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

function ensureDir(path, dryRun) {
  if (dryRun || existsSync(path)) {
    return;
  }
  mkdirSync(path, { recursive: true });
}

function copyDirectoryContents(srcRoot, destRoot, dryRun) {
  if (!existsSync(srcRoot)) {
    throw new Error(`Missing iOS scaffolding directory: ${srcRoot}`);
  }
  ensureDir(destRoot, dryRun);
  for (const entry of readdirSync(srcRoot)) {
    const src = join(srcRoot, entry);
    const dest = join(destRoot, entry);
    if (statSync(src).isDirectory()) {
      copyDirectoryContents(src, dest, dryRun);
      continue;
    }
    if (dryRun) {
      console.log(`[dry-run] Would copy ${src} -> ${dest}`);
      continue;
    }
    ensureDir(dirname(dest), dryRun);
    copyFileSync(src, dest);
    console.log(`Copied ${dest}`);
  }
}

function patchProjectYml(dryRun) {
  const original = readFileSync(projectYmlPath, 'utf8');
  let content = original;

  // 1. 部署目标：init 生成 14.0，Xcode 27 SDK 要求 >= 15.0。
  const deploymentPattern = /(deploymentTarget:\s*\n\s*iOS:\s*)14\.0/;
  if (deploymentPattern.test(content)) {
    content = content.replace(deploymentPattern, `$1${DEPLOYMENT_TARGET}`);
    console.log(`Patched deploymentTarget iOS -> ${DEPLOYMENT_TARGET}`);
  } else if (content.includes(`iOS: ${DEPLOYMENT_TARGET}`)) {
    console.log('deploymentTarget already patched; skipping.');
  } else {
    throw new Error('project.yml: 未找到 deploymentTarget.iOS 锚点，请检查 tauri ios init 模板变更');
  }

  // 2. Info.plist 权限声明：麦克风用途描述（幂等：已存在则跳过）。
  if (content.includes('NSMicrophoneUsageDescription')) {
    console.log('Info.plist mic usage already present; skipping.');
  } else if (content.includes('        LSRequiresIPhoneOS: true\n')) {
    content = content.replace(
      '        LSRequiresIPhoneOS: true\n',
      `${INFO_PROPERTIES_SNIPPET}\n`,
    );
    console.log('Added NSMicrophoneUsageDescription to app Info.plist properties.');
  } else {
    throw new Error('project.yml: 未找到 LSRequiresIPhoneOS 锚点，无法插入 Info.plist 权限声明');
  }

  // 3. 签名设置（幂等）：以 EXCLUDED_ARCHS 行为锚点追加到 app target settings.base。
  const excludedArchsAnchor = '        EXCLUDED_ARCHS[sdk=iphoneos*]: x86_64';
  if (content.includes('CODE_SIGNING_ALLOWED:') || content.includes('DEVELOPMENT_TEAM:')) {
    console.log('Signing settings already present; skipping.');
  } else if (content.includes(excludedArchsAnchor)) {
    content = content.replace(
      excludedArchsAnchor,
      [excludedArchsAnchor, ...signingSettingsLines('        ')].join('\n'),
    );
    console.log(
      developmentTeam
        ? `Configured automatic signing with team ${developmentTeam}.`
        : 'Disabled code signing for simulator builds (set OPENLESS_IOS_DEVELOPMENT_TEAM for device builds).',
    );
  } else {
    throw new Error('project.yml: 未找到 EXCLUDED_ARCHS 锚点，无法插入签名设置');
  }

  // 4. Swift 兼容库：libapp.a 内嵌的 Swift 目标文件（swift-rs / tauri-plugin-dialog）
  //    携带 __swift_FORCE_LOAD_$_swiftCompatibility56 强制加载符号，而最终链接由
  //    clang 驱动（app target 无 Swift 源码，Swift driver 不传兼容库），需显式补上。
  //    搜索路径不用 TOOLCHAIN_DIR：Xcode 27 下构建期它可能被解析到 Metal 工具链
  //    cryptex 瞬态挂载点，DEVELOPER_DIR 恒定指向 Xcode.app。
  const embedSwiftAnchor = '        ALWAYS_EMBED_SWIFT_STANDARD_LIBRARIES: true';
  if (content.includes('swiftCompatibility56')) {
    console.log('Swift compatibility libs already linked; skipping.');
  } else if (content.includes(embedSwiftAnchor)) {
    content = content.replace(
      embedSwiftAnchor,
      `${embedSwiftAnchor}\n        OTHER_LDFLAGS: $(inherited) -L$(DEVELOPER_DIR)/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/$(PLATFORM_NAME) -lswiftCompatibility56`,
    );
    console.log('Added swiftCompatibility56 link flags to OTHER_LDFLAGS.');
  } else {
    throw new Error('project.yml: 未找到 ALWAYS_EMBED_SWIFT_STANDARD_LIBRARIES 锚点');
  }

  // 5. 主 App entitlements：App Group（键盘扩展共享数据）。
  const appEntitlementsAnchor = '    entitlements:\n      path: openless_iOS/openless_iOS.entitlements';
  if (content.includes('com.apple.security.application-groups')) {
    console.log('App entitlements already patched; skipping.');
  } else if (content.includes(appEntitlementsAnchor)) {
    content = content.replace(
      appEntitlementsAnchor,
      `${appEntitlementsAnchor}\n      properties:\n        com.apple.security.application-groups:\n          - ${APP_GROUP}`,
    );
    console.log('Added App Group to app entitlements.');
  } else {
    throw new Error('project.yml: 未找到主 App entitlements 锚点');
  }

  // 6. 主 App dependencies：嵌入键盘扩展（PlugIns/*.appex）。
  const appDepsAnchor = '    dependencies:\n      - framework: libapp.a';
  if (content.includes('- target: OpenLessKeyboard')) {
    console.log('App dependencies already patched; skipping.');
  } else if (content.includes(appDepsAnchor)) {
    content = content.replace(
      appDepsAnchor,
      '    dependencies:\n      - target: OpenLessKeyboard\n        embed: true\n      - framework: libapp.a',
    );
    console.log('Added keyboard extension embed to app dependencies.');
  } else {
    throw new Error('project.yml: 未找到主 App dependencies 锚点');
  }

  // 7. 键盘扩展 target：追加到 openless_iOS target 之后（其最后一个键是
  //    preBuildScripts 的 arm64 outputFiles）。
  const appTargetEndAnchor =
    '          - $(SRCROOT)/Externals/arm64/${CONFIGURATION}/libapp.a';
  const keyboardTargetBlock = `          - \$(SRCROOT)/Externals/arm64/\${CONFIGURATION}/libapp.a
  OpenLessKeyboard:
    type: app-extension
    platform: iOS
    sources:
      - path: KeyboardExtension
    info:
      path: KeyboardExtension/Info.plist
      properties:
        CFBundleDisplayName: OpenLess 键盘
        NSExtension:
          NSExtensionPointIdentifier: com.apple.keyboard-service
          NSExtensionPrincipalClass: \$(PRODUCT_MODULE_NAME).KeyboardViewController
        RequestsOpenAccess: true
        NSMicrophoneUsageDescription: OpenLess 键盘需要使用麦克风进行语音输入转写。
    entitlements:
      path: KeyboardExtension/OpenLessKeyboard.entitlements
      properties:
        com.apple.security.application-groups:
          - ${APP_GROUP}
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: com.openless.app.OpenLessKeyboard
        ENABLE_BITCODE: false
        ALWAYS_EMBED_SWIFT_STANDARD_LIBRARIES: true
${signingSettingsLines('        ').join('\n')}`;
  if (content.includes('  OpenLessKeyboard:')) {
    console.log('Keyboard extension target already present; skipping.');
  } else if (content.includes(appTargetEndAnchor)) {
    content = content.replace(appTargetEndAnchor, keyboardTargetBlock);
    console.log('Added OpenLessKeyboard extension target.');
  } else {
    throw new Error('project.yml: 未找到 Build Rust Code 锚点，无法追加扩展 target');
  }

  if (content === original) {
    return;
  }
  if (dryRun) {
    console.log('[dry-run] Would write patched project.yml');
    return;
  }
  writeFileSync(projectYmlPath, content, 'utf8');
  console.log(`Wrote patched ${projectYmlPath}`);
}

function regenerateXcodeProject(dryRun) {
  if (dryRun) {
    console.log('[dry-run] Would run: xcodegen generate');
    return;
  }
  const result = spawnSync('xcodegen', ['generate'], {
    cwd: genAppleRoot,
    stdio: 'inherit',
  });
  if (result.error || result.status !== 0) {
    throw new Error('xcodegen generate 失败 — 请确认已安装 xcodegen (brew install xcodegen)');
  }
  console.log('Regenerated openless.xcodeproj from patched project.yml');
}

function main() {
  const { dryRun } = parseArgs(process.argv.slice(2));

  if (!existsSync(genAppleRoot)) {
    throw new Error(
      `Generated iOS project not found under src-tauri/gen/apple.\nRun "npm run tauri -- ios init" first.`,
    );
  }
  if (!existsSync(projectYmlPath)) {
    throw new Error(`project.yml not found at ${projectYmlPath}`);
  }

  patchProjectYml(dryRun);

  // ios/swift/KeyboardExtension → gen/apple/KeyboardExtension（扩展 target 源）；
  // 其余（ios/swift/App 等）→ gen/apple/Sources（主 App target 源）。
  if (existsSync(swiftRoot)) {
    for (const entry of readdirSync(swiftRoot)) {
      const src = join(swiftRoot, entry);
      if (!statSync(src).isDirectory()) {
        continue;
      }
      if (entry === 'KeyboardExtension') {
        copyDirectoryContents(src, keyboardSwiftDestRoot, dryRun);
      } else {
        copyDirectoryContents(src, join(appSwiftDestRoot, entry), dryRun);
      }
    }
  }

  regenerateXcodeProject(dryRun);
}

try {
  const isDirectRun = Boolean(
    process.argv[1]?.replace(/\\/g, '/').endsWith('copy-ios-scaffolding.mjs'),
  );
  if (isDirectRun) {
    main();
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
