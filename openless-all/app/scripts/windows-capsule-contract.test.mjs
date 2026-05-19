import { readFile } from 'node:fs/promises';

function assertMatch(source, pattern, name) {
  if (!pattern.test(source)) {
    throw new Error(`${name}: pattern ${pattern} not found`);
  }
}

function assertNotMatch(source, pattern, name) {
  if (pattern.test(source)) {
    throw new Error(`${name}: forbidden pattern ${pattern} found`);
  }
}

const libRs = await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf-8');
const coordinatorRs = await readFile(new URL('../src-tauri/src/coordinator.rs', import.meta.url), 'utf-8');
const dictationRs = await readFile(new URL('../src-tauri/src/coordinator/dictation.rs', import.meta.url), 'utf-8');
const capsuleTsx = await readFile(new URL('../src/components/Capsule.tsx', import.meta.url), 'utf-8');
const capsuleLayoutTs = await readFile(new URL('../src/lib/capsuleLayout.ts', import.meta.url), 'utf-8');
const hitlRunner = await readFile(new URL('./windows-capsule-hitl.ps1', import.meta.url), 'utf-8');

assertNotMatch(
  libRs,
  /apply_acrylic\(&capsule|DwmEnableBlurBehindWindow|DWMWA_SYSTEMBACKDROP_TYPE/,
  'windows capsule native host must not use Acrylic/Mica/DWM material',
);

assertNotMatch(
  libRs,
  /WS_EX_TRANSPARENT|SetWindowLongPtrW|EnumChildWindows/,
  'windows capsule must not rely on dynamic WebView child HWND click-through styles',
);

assertMatch(
  libRs,
  /fn configure_windows_capsule_container_region[\s\S]*?GetWindowRect[\s\S]*?windows_capsule_create_region[\s\S]*?SetWindowRgn/,
  'windows capsule should apply a native container/input envelope',
);

assertMatch(
  libRs,
  /fn windows_capsule_region_rects[\s\S]*?windows_capsule_translation_badge_region_rect[\s\S]*?windows_capsule_pill_region_rect/,
  'windows capsule native envelope should be derived from rendered capsule surfaces',
);

assertMatch(
  libRs,
  /CreateRoundRectRgn[\s\S]*?CombineRgn[\s\S]*?RGN_OR/,
  'windows capsule should use rounded native regions for pill and translation badge envelopes',
);

assertMatch(
  libRs,
  /fn windows_capsule_pill_region_rect[\s\S]*?const PILL_WIDTH: f64 = 196\.0;[\s\S]*?const PILL_HEIGHT: f64 = 52\.0;[\s\S]*?const BOTTOM_INSET: f64 = 12\.0;/,
  'windows native envelope should cover the full rendered capsule, not a strict inner CSS box',
);

assertMatch(
  coordinatorRs,
  /show_capsule_window_for_recording\(&app_for_main,\s*&window\);[\s\S]*?refresh_windows_capsule_container_region\(&window,\s*translation\)/,
  'windows capsule should refresh the native envelope after no-activate show',
);

assertMatch(
  coordinatorRs,
  /fn capsule_hitl_translation_fixture_enabled\(\)[\s\S]*?OPENLESS_CAPSULE_HITL_TRANSLATION/,
  'translation badge HITL fixture should be gated behind an explicit debug environment variable',
);

assertMatch(
  dictationRs,
  /if hotkey_injection_dry_run_enabled\(\)[\s\S]*?capsule_hitl_translation_fixture_enabled\(\)[\s\S]*?translation_modifier_seen[\s\S]*?store\(true,[\s\S]*?emit_capsule\(inner,\s*CapsuleState::Recording/,
  'translation badge HITL fixture should only affect hotkey-injection dry-run sessions',
);

assertMatch(
  capsuleLayoutTs,
  /export interface CapsuleInputEnvelopeMetrics[\s\S]*?export function getCapsuleInputEnvelopeMetrics/,
  'frontend layout contract should name native input envelope metrics separately from host and pill metrics',
);

assertMatch(
  capsuleLayoutTs,
  /getCapsuleInputEnvelopeMetrics\(os: OS\)[\s\S]*?width: 196,[\s\S]*?height: 52,[\s\S]*?radius: 26,[\s\S]*?bottomInset: 12,[\s\S]*?badgeWidth: 132,[\s\S]*?badgeHeight: 24,[\s\S]*?badgeGap: 8/,
  'frontend Windows input envelope metrics should match the native region contract',
);

assertMatch(
  capsuleTsx,
  /const useBackdrop = os !== 'win';[\s\S]*?background: os === 'win' \? 'rgba\(255, 255, 255, 0\.96\)' : 'rgba\(255, 255, 255, 0\.85\)'/,
  'windows capsule DOM pill should own its surface without relying on desktop backdrop blur',
);

assertMatch(
  capsuleTsx,
  /return\s*\(\s*<div\s*style=\{\{[\s\S]*?width:\s*'100%',[\s\S]*?height:\s*'100%',[\s\S]*?paddingLeft:\s*hostMetrics\.horizontalInset,[\s\S]*?paddingRight:\s*hostMetrics\.horizontalInset,[\s\S]*?background:\s*'transparent'/,
  'capsule host should remain transparent outside the visible pill',
);

assertMatch(
  hitlRunner,
  /\[switch\]\$TranslationActive[\s\S]*?OPENLESS_CAPSULE_HITL_TRANSLATION[\s\S]*?badgeCenterHitsCapsule[\s\S]*?screenBadgeCaptured/,
  'windows capsule HITL runner should verify the translation badge native region when requested',
);
