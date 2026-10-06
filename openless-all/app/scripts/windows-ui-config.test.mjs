import { readFile } from 'node:fs/promises';

function assertEqual(actual, expected, name) {
  if (actual !== expected) {
    throw new Error(`${name}: expected ${expected}, got ${actual}`);
  }
}

function assertMatch(source, pattern, name) {
  if (!pattern.test(source)) {
    throw new Error(`${name}: pattern ${pattern} not found`);
  }
}

const raw = await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf-8');
const config = JSON.parse(raw);
const capsuleWindow = config.app.windows.find((window) => window.label === 'capsule');
const capsuleRailWindow = config.app.windows.find((window) => window.label === 'capsule-rail');
const mainWindow = config.app.windows.find((window) => window.label === 'main');
const libRs = await readFile(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf-8');
const dictationRs = await readFile(new URL('../src-tauri/src/commands/dictation.rs', import.meta.url), 'utf-8');
const sharedTypesRs = await readFile(
  new URL('../crates/openless-core/src/shared_types.rs', import.meta.url),
  'utf-8',
);
// Contract check over the union of the capsule subsystem compiled into the binary; native
// window manipulation belongs to the explicit Tauri Host.
const coordinatorRs =
  (await readFile(new URL('../src-tauri/src/coordinator.rs', import.meta.url), 'utf-8')) +
  '\n' +
  (await readFile(
    new URL('../src-tauri/src/coordinator/capsule_focus.rs', import.meta.url),
    'utf-8',
  )) +
  '\n' +
  (await readFile(new URL('../src-tauri/src/tauri_coordinator_host.rs', import.meta.url), 'utf-8'));
const tauriEventsRs = await readFile(new URL('../src-tauri/src/tauri_events.rs', import.meta.url), 'utf-8');
const capsuleTsx = await readFile(
  new URL('../src/components/Capsule.tsx', import.meta.url),
  'utf-8',
);
const capsuleLayoutTs = await readFile(
  new URL('../src/lib/capsuleLayout.ts', import.meta.url),
  'utf-8',
);
const capsuleStylesCss = await readFile(
  new URL('../src/components/CapsuleStyles.css', import.meta.url),
  'utf-8',
);
const typelessTsx = await readFile(
  new URL('../src/components/TypelessCapsule.tsx', import.meta.url),
  'utf-8',
);
const liveTranscriptTsx = await readFile(
  new URL('../src/components/LiveTranscriptPill.tsx', import.meta.url),
  'utf-8',
);
const backendEventTs = await readFile(
  new URL('../src/lib/backendEvent.ts', import.meta.url),
  'utf-8',
);
const appTsx = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf-8');
const mainTsx = await readFile(new URL('../src/main.tsx', import.meta.url), 'utf-8');
const windowChromeTsx = await readFile(
  new URL('../src/components/WindowChrome.tsx', import.meta.url),
  'utf-8',
);
const floatingShellTsx = await readFile(
  new URL('../src/components/FloatingShell.tsx', import.meta.url),
  'utf-8',
);
const themeModeTs = await readFile(new URL('../src/lib/themeMode.ts', import.meta.url), 'utf-8');
const platformTs = await readFile(new URL('../src/lib/platform.ts', import.meta.url), 'utf-8');

if (!capsuleWindow) {
  throw new Error('capsule window config missing');
}
if (!mainWindow) {
  throw new Error('main window config missing');
}
if (!capsuleRailWindow) {
  throw new Error('capsule rail window config missing');
}
assertEqual(capsuleWindow.width, 460, 'windows capsule config keeps the shared bootstrap width');
assertEqual(capsuleWindow.height, 180, 'windows capsule config keeps the shared bootstrap height');
assertEqual(capsuleWindow.transparent, true, 'capsule window should keep transparent visuals');
assertEqual(
  capsuleWindow.alwaysOnTop,
  true,
  'capsule window should stay above the focused app while recording',
);
assertEqual(capsuleRailWindow.transparent, true, 'capsule rail should be transparent');
assertEqual(capsuleRailWindow.visible, false, 'capsule rail should start hidden');
assertEqual(capsuleRailWindow.focus, false, 'capsule rail should never take focus');
assertEqual(capsuleRailWindow.skipTaskbar, true, 'capsule rail should stay out of the taskbar');
assertEqual(capsuleRailWindow.url, 'index.html?window=capsule-rail&os=win', 'capsule rail route');
assertEqual(mainWindow.decorations, true, 'windows main window should keep native decorations');
assertEqual(
  mainWindow.visible,
  false,
  'windows main window should stay hidden until the intended first show point',
);

assertMatch(
  libRs,
  /fn apply_windows_caption_theme[\s\S]*?DWMWA_USE_IMMERSIVE_DARK_MODE[\s\S]*?DWMWA_CAPTION_COLOR[\s\S]*?DWMWA_TEXT_COLOR[\s\S]*?DWMWA_BORDER_COLOR/,
  'windows runtime should sync immersive dark mode and caption/text/border colors',
);

assertMatch(
  libRs,
  /#\[tauri::command\][\s\S]*?fn set_windows_caption_theme/,
  'windows caption theme should be exposed as a Tauri command',
);

assertMatch(
  themeModeTs,
  /export function applyThemeMode[\s\S]*?syncWindowsCaptionTheme/,
  'applyThemeMode should sync Windows native caption theme',
);

assertMatch(
  platformTs,
  /export async function syncWindowsCaptionTheme[\s\S]*?set_windows_caption_theme/,
  'platform IPC wrapper should invoke set_windows_caption_theme',
);

assertMatch(
  coordinatorRs,
  /#\[cfg\(target_os = "macos"\)\][\s\S]*?orderFrontRegardless/,
  'macOS capsule should show without taking the key window',
);

const tokensCss = await readFile(new URL('../src/styles/tokens.css', import.meta.url), 'utf-8');

if (!/os === 'win' \|\| os === 'android' \? 0 : 14/.test(windowChromeTsx)) {
  throw new Error(
    'windows main shell should rely on native decorations instead of a frameless chrome shell',
  );
}

assertMatch(
  windowChromeTsx,
  /\/\/ Windows: with decorations:true the shell draws no radius/,
  'windows WindowChrome should defer chrome to native decorations',
);

assertMatch(
  windowChromeTsx,
  /const MAC_TITLEBAR_HEIGHT = 44;/,
  'macOS drag region should reserve the native traffic-light area',
);
assertEqual(mainWindow.trafficLightPosition.x, 16, 'traffic lights should have a 16px left inset');
// In tao's inset_traffic_lights, y only scales the title-bar container (slope 1), so the
// visual top inset ≈ y-14; with left=16 the measured left inset is 21.5px, and y=26 makes
// the top inset equal (x==y does not).
assertEqual(mainWindow.trafficLightPosition.y, 26, 'traffic lights should have an equal visual top inset');
assertEqual(mainWindow.width, 1300, 'main window should use the reviewed default width');
assertEqual(mainWindow.height, 835, 'main window should use the reviewed default height');
assertEqual(mainWindow.resizable, true, 'users should still be able to resize the main window');
assertMatch(
  libRs,
  /show_main_window[\s\S]*?set_focus\(\)/,
  'macOS main window should rely on native traffic lights instead of manually moving standardWindowButton frames',
);
if (/standardWindowButton|setFrameOrigin: origin|tune_macos_main_window_controls/.test(libRs)) {
  throw new Error(
    'macOS traffic lights should not be manually repositioned; keep native AppKit button frames visible',
  );
}
assertMatch(
  tokensCss,
  /--ol-motion-spring:[\s\S]*?--ol-motion-soft:[\s\S]*?--ol-motion-quick:/,
  'shared motion tokens should drive shell animations and transitions',
);

assertMatch(
  floatingShellTsx,
  /className="ol-console-main"[\s\S]*?borderRadius:\s*0,[\s\S]*?boxShadow:\s*'none'/,
  'main content should keep the intentional flush shell treatment',
);

assertMatch(
  coordinatorRs,
  /let visible = !matches!\(state,\s*CapsuleState::Idle\);/,
  'capsule should stay visible until the unified idle hide path runs',
);
assertMatch(
  coordinatorRs,
  /fn hide_capsule_window_if_present\(\)/,
  'windows capsule lifecycle should include an explicit native hide helper',
);
assertMatch(
  coordinatorRs,
  /ShowWindow\(hwnd, SW_HIDE\)/,
  'windows capsule hide helper should force the native window hidden',
);
assertMatch(
  coordinatorRs,
  /SetWindowPos\([\s\S]*?HWND_NOTOPMOST[\s\S]*?SWP_HIDEWINDOW/m,
  'windows capsule hide helper should drop topmost participation when inactive',
);

if (
  !/export function getCapsuleHostMetrics\(\s*os: OS,\s*translationActive: boolean,\s*style: CapsuleStyle = 'siri',?\s*\): CapsuleHostMetrics/.test(
    capsuleLayoutTs,
  )
) {
  throw new Error(
    'capsule layout should define explicit host metrics separate from the visible pill metrics',
  );
}

assertMatch(
  capsuleLayoutTs,
  // The 1.3.14 voice-orb shell replaced the legacy 196px Windows pill and its
  // 12px host inset. Keep the frontend and native stage contracts in sync.
  /const VOICE_ORB_STAGE_WIDTH = 460;[\s\S]*?const VOICE_ORB_STAGE_HEIGHT = 180;/,
  'capsule layout should keep the shared 460x180 voice-orb stage',
);

assertMatch(
  capsuleLayoutTs,
  /const stage = getCapsulePillMetrics\(os\);[\s\S]*?width: stage\.width,[\s\S]*?height: style === 'siri' \? SIRI_HOST_HEIGHT : style === 'classic' \? CLASSIC_HOST_HEIGHT : 128,[\s\S]*?horizontalInset: 0,[\s\S]*?bottomInset: style === 'siri' \? 0 : 16,[\s\S]*?badgeGap: 8,[\s\S]*?boxSizing: 'border-box'/,
  'capsule host metrics should preserve Siri and reserve compact surfaces for Classic and Typeless',
);

if (
  !/const hostMetrics = getCapsuleHostMetrics\(os,\s*translation,\s*capsuleStyle\);/.test(
    capsuleTsx,
  )
) {
  throw new Error('capsule should derive host metrics from the shared layout contract');
}

if (
  !/return\s*\(\s*<div\s*style=\{\{[\s\S]*?width:\s*'100%',[\s\S]*?height:\s*'100%',[\s\S]*?position:\s*'relative',[\s\S]*?display:\s*'flex',[\s\S]*?alignItems:\s*'center',[\s\S]*?justifyContent:\s*'flex-end',[\s\S]*?paddingLeft:\s*hostMetrics\.horizontalInset,[\s\S]*?paddingRight:\s*hostMetrics\.horizontalInset,[\s\S]*?\}\}/.test(
    capsuleTsx,
  )
) {
  throw new Error('capsule host should center the pill within the shared layout contract');
}

if (
  !/paddingLeft:\s*hostMetrics\.horizontalInset,/.test(capsuleTsx) ||
  !/paddingRight:\s*hostMetrics\.horizontalInset,/.test(capsuleTsx)
) {
  throw new Error('capsule host should consume the shared horizontal inset contract');
}

if (!/paddingBottom:\s*hostMetrics\.bottomInset/.test(capsuleTsx)) {
  throw new Error('all capsule hosts should respect their style-specific bottom inset');
}

if (!/const badgeBottom = Math\.round\(metrics\.height \* 0\.73\);/.test(capsuleTsx)) {
  throw new Error('translation badge should anchor proportionally within the voice-orb stage');
}

assertMatch(
  capsuleLayoutTs,
  /const TYPELESS_TRANSLATION_HOST_HEIGHT = 65;[\s\S]*?height: translationActive \? TYPELESS_TRANSLATION_HOST_HEIGHT : TYPELESS_STAGE_HEIGHT,/,
  'typeless translation should reserve native height for its dedicated row',
);
assertMatch(
  capsuleLayoutTs,
  /CAPSULE_TRANSCRIPT_RAIL_HEIGHTS[\s\S]*?siri: 40,[\s\S]*?classic: 52,[\s\S]*?typeless: 52[\s\S]*?getCapsuleTranscriptRailHeight/,
  'all capsule styles should share explicit native/CSS rail heights',
);
assertMatch(
  capsuleLayoutTs,
  /getClassicCapsuleGeometry[\s\S]*?CLASSIC_CAPSULE_HOST_HEIGHT[\s\S]*?CLASSIC_CAPSULE_BADGE_HEIGHT[\s\S]*?transcriptInSameWindow[\s\S]*?railTop/,
  'Classic translation badge and external rail should use one explicit geometry model',
);
assertMatch(
  capsuleTsx,
  /case 'recording':[\s\S]*?AudioBars[\s\S]*?case 'transcribing':[\s\S]*?case 'polishing':[\s\S]*?cap-state-enter[\s\S]*?AudioBars[\s\S]*?cap-shine[\s\S]*?getClassicProcessingLabel\(operating\)/,
  'Classic recording/transcribing/polishing should render waveform bars and preserve thinking/using text',
);
assertMatch(
  capsuleTsx,
  /const cancelEnabled = state === 'recording' \|\| state === 'transcribing' \|\| state === 'polishing'/,
  'Classic cancel should remain available during recording and both processing states',
);
assertMatch(
  capsuleTsx,
  /getClassicCapsuleGeometry\([\s\S]*?transcriptInSameWindow[\s\S]*?badgeBottomOffset/,
  'Classic external rail should use the shared badge/body geometry instead of bypassing offset',
);
assertMatch(
  typelessTsx,
  /<div className="ol-typeless-translation-row">[\s\S]*?ol-typeless-translation[\s\S]*?<div className="ol-typeless-capsule-stage">/,
  'typeless translation should occupy its own layout row between the rail and capsule',
);
assertMatch(
  capsuleStylesCss,
  /\.ol-typeless-translation-row \{[\s\S]*?flex: 0 0 20px;[\s\S]*?height: 20px;[\s\S]*?\.ol-typeless-translation \{[\s\S]*?position: static;/,
  'typeless translation row should prevent badge overlap and clipping',
);

const conditionalReturn = capsuleTsx.indexOf('if (insertFallback)');
const transcriptVisibilityHook = capsuleTsx.indexOf('setCapsuleTranscriptVisible(transcriptVisible)');
if (conditionalReturn < 0 || transcriptVisibilityHook < 0 || transcriptVisibilityHook > conditionalReturn) {
  throw new Error('transcript visibility hook must run before every conditional return');
}
assertMatch(
  capsuleTsx,
  /p\.state === 'idle' \|\| p\.state === 'done' \|\| p\.state === 'cancelled' \|\| p\.state === 'error'[\s\S]*?setLocalAsrText\(''\)/,
  'terminal capsule states should clear the transcript visibility source',
);
if (capsuleTsx.slice(conditionalReturn).includes('useEffect(')) {
  throw new Error('capsule must not call hooks after entering a conditional return branch');
}

assertMatch(
  libRs,
  /fn capsule_window_bounds_for_style_with_transcript_and_translation\([\s\S]*?types::CapsuleStyle::Typeless => 206\.0,[\s\S]*?types::CapsuleStyle::Siri \| types::CapsuleStyle::Classic => 460\.0,[\s\S]*?types::CapsuleStyle::Siri => \{[\s\S]*?228\.0[\s\S]*?180\.0[\s\S]*?types::CapsuleStyle::Classic => \{[\s\S]*?172\.0[\s\S]*?100\.0[\s\S]*?types::CapsuleStyle::Typeless => \{[\s\S]*?65\.0[\s\S]*?57\.0[\s\S]*?bottom_inset: 0\.0,/,
  'native capsule bounds should match the frontend dimensions including the translation row',
);

assertMatch(
  coordinatorRs,
  /transcript_visible: AtomicBool[\s\S]*?set_transcript_visible\([\s\S]*?position_capsule_bottom_center_with_style_and_transcript/,
  'capsule hit area should track whether the transcript rail is visible',
);
assertMatch(
  capsuleTsx,
  /setCapsuleTranscriptVisible\(transcriptVisible\)/,
  'frontend should sync transcript visibility to the native hit area',
);

assertMatch(
  coordinatorRs,
  /capsule_control_hit_rect[\s\S]*?WM_NCHITTEST[\s\S]*?HTTRANSPARENT/,
  'capsule hit testing should cover X/Y and use a real native input region',
);
assertMatch(
  coordinatorRs,
  /control_left:[\s\S]*?control_right:[\s\S]*?control_top:[\s\S]*?control_bottom:/,
  'capsule hit-test state should store both X and Y bounds',
);
assertMatch(coordinatorRs, /CreateRoundRectRgn[\s\S]*?SetWindowRgn/, 'capsule input region should be native');
assertMatch(coordinatorRs, /WM_NCDESTROY[\s\S]*?remove\([\s\S]*?SetWindowLongPtrW/, 'subclass teardown should restore the old WndProc and remove its table entry');
assertMatch(coordinatorRs, /SetLastError[\s\S]*?GetLastError/, 'WndProc installation should detect SetWindowLongPtrW failure');
assertMatch(coordinatorRs, /DeleteObject[\s\S]*?SetWindowRgn failed/, 'failed regions should release their HRGN');
assertMatch(
  coordinatorRs,
  /windows_hit_test_uses_x_and_y_and_translation_bounds/,
  'hit-test regression should cover X/Y and translation height',
);
assertMatch(
  coordinatorRs,
  /HIT_TEST_MODE_CARD[\s\S]*?set_card_hit_test_mode[\s\S]*?restore_capsule_hit_test_mode/,
  'cards should own and restore the shared window hit-test mode',
);
assertMatch(coordinatorRs, /set_card_hit_test_mode[\s\S]*?configure_card_hit_test/, 'card mode configures full client area');
assertMatch(coordinatorRs, /refresh_card_hit_test/, 'card resize refreshes native hit area');
assertMatch(coordinatorRs, /restore_capsule_geometry/, 'card dismissal restores style-aware capsule geometry');
if (/capsule_window_bounds\(false\)|position_capsule_bottom_center\(false\)/.test(
  await readFile(new URL('../src-tauri/src/coordinator.rs', import.meta.url), 'utf-8'),
)) {
  throw new Error('card dismissal must not restore a hard-coded capsule geometry');
}
assertMatch(
  coordinatorRs,
  /hit_test_mode\.load\(Ordering::SeqCst\) == HIT_TEST_MODE_CARD[\s\S]*?hide_transcript_overlay[\s\S]*?return Ok\(\(\)\)/,
  'card-owned windows must ignore transcript visibility layout updates',
);
assertMatch(
  coordinatorRs,
  /emit_to\("capsule-rail", "capsule:state"/,
  'coordinator state events should reach the dedicated transcript rail',
);
assertMatch(
  tauriEventsRs,
  /get_webview_window\("capsule-rail"\)[\s\S]*?emit\("capsule:state"/,
  'direct dictation state events should reach the dedicated transcript rail',
);
assertMatch(mainTsx, /const isCapsuleRail = windowKind === 'capsule-rail'/, 'rail route should be selected');
assertMatch(appTsx, /isCapsuleRail[\s\S]*?CapsuleTranscriptOverlay/, 'rail route should render the overlay');
assertMatch(
  liveTranscriptTsx,
  /export function CapsuleTranscriptOverlay[\s\S]*?listen<CapsulePayload>\('capsule:state'[\s\S]*?listen<BackendEvent>\('backend:event'[\s\S]*?prefs:changed/,
  'rail should own state, transcript, and preference subscriptions',
);
assertMatch(
  liveTranscriptTsx,
  /const replaySnapshot[\s\S]*?getCapsuleSnapshot\(\)[\s\S]*?await replaySnapshot\(preferences\.capsuleStyle\)/,
  'rail should replay the native snapshot with bounded retry after its listeners are ready',
);
assertMatch(
  liveTranscriptTsx,
  /snapshotRetryDelays = \[0, 16, 32, 64, 128, 256\][\s\S]*?if \(!snapshot\) continue/,
  'rail replay should retry transient empty snapshots for a bounded interval',
);
assertMatch(liveTranscriptTsx, /visibleCapsuleTranscript\(text, enabled, state, selectionPolish\)/, 'rail should apply the same selection-polish visibility predicate');
assertMatch(
  coordinatorRs,
  /set_ignore_cursor_events\(true\)[\s\S]*?set_size\([\s\S]*?set_position\([\s\S]*?show_capsule_window_for_recording\(&self\.app, &rail, false\)/,
  'transcript rail should be positioned as its own click-through window',
);
assertMatch(
  coordinatorRs,
  /fn capsule_transcript_rail_position[\s\S]*?CapsuleStyle::Siri[\s\S]*?CapsuleStyle::Classic[\s\S]*?CapsuleStyle::Typeless/,
  'rail positioning should have one style-aware pure geometry function',
);
assertMatch(
  coordinatorRs,
  /capsule_transcript_rail_position\(\s*style,\s*translation_active,\s*visible,?\s*\)[\s\S]*?position\.top_offset/,
  'rail native positioning should use body-relative top offset and translation state',
);
assertMatch(coordinatorRs, /SW_SHOWNOACTIVATE[\s\S]*?SWP_NOACTIVATE/, 'rail show should use the no-activate path');
assertMatch(
  coordinatorRs,
  /no_activate failed: Win32 handle is null/,
  'no-activate display should validate HWND and fall back when SetWindowPos fails',
);
assertMatch(
  coordinatorRs,
  /if let Err\(error\) = unsafe \{[\s\S]*?SetWindowPos\([\s\S]*?return false/,
  'no-activate display should return false when SetWindowPos fails',
);
assertMatch(
  dictationRs,
  /get_capsule_snapshot\([\s\S]*?Result<Option<crate::tauri_coordinator_host::CapsuleSnapshot>, String>[\s\S]*?capsule_snapshot/,
  'rail snapshot command and implementation must use the same CapsuleSnapshot DTO',
);
assertMatch(
  coordinatorRs,
  /struct CapsuleSnapshot[\s\S]*?payload: CapsulePayload[\s\S]*?transcript: String[\s\S]*?sequence: u64[\s\S]*?revision: u64/,
  'native rail snapshot should include the session-bearing payload, transcript, and both watermarks',
);
assertMatch(
  sharedTypesRs,
  /struct CapsulePayload[\s\S]*?session_id: Option<String>/,
  'capsule payload should carry the session generation used by the rail replay',
);
assertMatch(
  coordinatorRs,
  /struct CapsuleSnapshotState[\s\S]*?payload_revision: u64[\s\S]*?snapshot: Mutex<CapsuleSnapshotState>[\s\S]*?capsule_snapshot\([\s\S]*?self\.capsule\.snapshot\.lock\(\)\.clone\(\)[\s\S]*?payload_revision: snapshot\.payload_revision/,
  'native rail snapshot should keep payload and payload commit revision under one lock',
);
assertMatch(
  coordinatorRs,
  /record_backend_event\([\s\S]*?payload_projection_pending[\s\S]*?snapshot\.revision = event\.sequence/,
  'native rail snapshot should advance relevant transcript/session revisions',
);
assertMatch(
  coordinatorRs,
  /snapshot\.pending_payload_revision = Some\(event\.sequence\)/,
  'native rail snapshot should keep revisions pending until a matching payload commit',
);
assertMatch(
  coordinatorRs,
  /pending_payload_revision: Option<u64>[\s\S]*?fn can_commit_capsule_payload\([\s\S]*?captured_revision == current_revision[\s\S]*?commit_capsule_payload\([\s\S]*?can_commit_capsule_payload\([\s\S]*?snapshot\.payload = Some\(committed\)[\s\S]*?snapshot\.payload_revision = current_revision/,
  'capsule payload commits must use strict captured/pending revisions and reject stale frames',
);
assertMatch(
  tauriEventsRs,
  /record_backend_event\(&event\)[\s\S]*?forward_legacy_event\(/,
  'backend event watermark must advance before queued capsule payload presentation',
);
assertMatch(
  liveTranscriptTsx,
  /applyTranscriptSnapshot[\s\S]*?const nextTranscript[\s\S]*?setText\(nextTranscript\.text\)/,
  'rail replay should reject snapshots older than the live event watermark',
);
assertMatch(
  backendEventTs,
  /payloadRevision[\s\S]*?if \(payloadRevision !== snapshot\.revision\) return state/,
  'rail replay should reject snapshots whose payload and transcript revisions differ',
);

assertMatch(
  libRs,
  /fn capsule_visual_height\(_translation_active: bool\) -> f64[\s\S]*?140\.0/,
  'runtime capsule visual anchor should preserve the intentional 140px height',
);

if (!/window\.set_size\(LogicalSize::new\(bounds\.width, bounds\.height\)\)\?/.test(libRs)) {
  throw new Error('capsule positioning should resync runtime size with the computed layout');
}

if (!/let _ = window\.hide\(\);/.test(coordinatorRs)) {
  throw new Error('capsule should be hidden once it leaves active states');
}
