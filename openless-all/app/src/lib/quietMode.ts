// Quiet mode 状態（capsule のテキストオーバーレイ抑制）の localStorage アクセスを集約。
// Capsule.tsx と SettingsModal.tsx 双方から同じキー（ol-quiet-completion）を参照する。

const QUIET_COMPLETION_KEY = 'ol-quiet-completion';

export function readQuietCompletion(): boolean {
  try {
    return window.localStorage.getItem(QUIET_COMPLETION_KEY) === 'true';
  } catch {
    return false;
  }
}

export function setQuietCompletion(value: boolean): void {
  try {
    window.localStorage.setItem(QUIET_COMPLETION_KEY, value ? 'true' : 'false');
  } catch {
    /* localStorage unavailable: ignore */
  }
}
