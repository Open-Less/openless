// i18n 入口 — 必须在任意 UI 组件 import 之前完成 init。
//
// 设计说明：
// - 资源在打包时静态注入（zh-CN.ts / en.ts）。无需后端推送，无网络请求。
// - LocalStorage key `ol.locale` 持久化用户选择；首次启动按 navigator.language 推断。
// - fallback 永远是 zh-CN：已知的产品权威文案，且 zh-CN.ts 是 source of truth。
// - 不用 LanguageDetector 插件：它的异步 init 在 Tauri WebView 里会让首次渲染拿到的
//   `t()` 返回 key（react-i18next useSuspense 默认 false 时返回 key 而非阻塞）。
//   手写检测 + initImmediate: false 让 init 同步完成，渲染前 t 就能用。

import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import { zhCN } from './zh-CN';
import type { UserPreferences } from '../lib/types';
import { setRemoteLocale } from '../lib/ipc';

export const SUPPORTED_LOCALES = ['zh-CN', 'zh-TW', 'en', 'ja', 'ko'] as const;
export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

export const LOCALE_STORAGE_KEY = 'ol.locale';
const FOLLOW_SYSTEM_VALUE = 'system';

function detectSystemLocale(): SupportedLocale {
  if (typeof navigator === 'undefined') return 'zh-CN';
  const nav = (navigator.language || '').toLowerCase();
  if (nav.startsWith('zh')) {
    if (nav.includes('hant') || nav.includes('tw') || nav.includes('hk') || nav.includes('mo')) return 'zh-TW';
    return 'zh-CN';
  }
  if (nav.startsWith('ja')) return 'ja';
  if (nav.startsWith('ko')) return 'ko';
  return 'en';
}

function resolveLocalePreference(pref: SupportedLocale | typeof FOLLOW_SYSTEM_VALUE): SupportedLocale {
  if (pref === FOLLOW_SYSTEM_VALUE) return detectSystemLocale();
  return pref;
}

function getStoredLocale(): SupportedLocale | null {
  if (typeof window === 'undefined') return null;
  const raw = window.localStorage.getItem(LOCALE_STORAGE_KEY);
  return raw === 'zh-CN' || raw === 'zh-TW' || raw === 'en' || raw === 'ja' || raw === 'ko' ? raw : null;
}

const initialLng: SupportedLocale = getStoredLocale() ?? detectSystemLocale();

// 只把 fallback / source-of-truth 的 zh-CN 静态打进 index —— 5 个 webview 都加载 index，
// 把 en / ja / ko / zh-TW 四份语言包拆成按需 chunk，减小每个 webview 的常驻体积。
// 代价：非 zh-CN 用户首屏先显示 zh-CN 一瞬，语言包异步到位后切换；最坏情况（加载失败）
// 也只停在 zh-CN（可读），不会出现 key 字面量。
const localeLoaders: Record<Exclude<SupportedLocale, 'zh-CN'>, () => Promise<typeof zhCN>> = {
  'zh-TW': () => import('./zh-TW').then(m => m.zhTW),
  en: () => import('./en').then(m => m.en),
  ja: () => import('./ja').then(m => m.ja),
  ko: () => import('./ko').then(m => m.ko),
};

async function ensureLocaleLoaded(lng: SupportedLocale): Promise<void> {
  if (lng === 'zh-CN' || i18n.hasResourceBundle(lng, 'translation')) return;
  try {
    i18n.addResourceBundle(lng, 'translation', await localeLoaders[lng]());
  } catch (e) {
    console.warn('[i18n] load locale failed, staying on zh-CN fallback:', lng, e);
  }
}

void i18n.use(initReactI18next).init({
  // 仅内联 zh-CN（fallback）。init 仍同步完成，首屏 t() 立刻可用（非 zh-CN 暂回退 zh-CN）。
  resources: { 'zh-CN': { translation: zhCN } },
  lng: initialLng,
  fallbackLng: 'zh-CN',
  supportedLngs: SUPPORTED_LOCALES as unknown as string[],
  partialBundledLanguages: true, // 告诉 i18next 内联资源已完整，无需 backend 拉取
  interpolation: { escapeValue: false },
  react: { useSuspense: false }, // 不悬挂；首屏必须能拿到译文（zh-CN 已内联，同步完成）
});

// 初始语言非 zh-CN：异步补加载该语言包后切换（首屏先 zh-CN，到位后无缝切过去）。
if (initialLng !== 'zh-CN') {
  void ensureLocaleLoaded(initialLng).then(() => i18n.changeLanguage(initialLng));
}

export default i18n;

/**
 * 当前持久化偏好。'system' 表示跟随系统；具体语言 tag 表示用户已显式选择。
 * 与 i18n.language 不同：i18n.language 永远是已 resolve 的具体语言。
 */
export function getLocalePreference(): SupportedLocale | typeof FOLLOW_SYSTEM_VALUE {
  return getStoredLocale() ?? FOLLOW_SYSTEM_VALUE;
}

/**
 * 写入用户偏好并立即切换 i18n 语言。
 * pref === 'system' 时清除存储项，重新走 navigator 检测。
 */
export async function setLocalePreference(
  pref: SupportedLocale | typeof FOLLOW_SYSTEM_VALUE,
): Promise<SupportedLocale> {
  const resolved = resolveLocalePreference(pref);
  if (pref === FOLLOW_SYSTEM_VALUE) {
    window.localStorage.removeItem(LOCALE_STORAGE_KEY);
  } else {
    window.localStorage.setItem(LOCALE_STORAGE_KEY, pref);
  }
  await ensureLocaleLoaded(resolved);
  await i18n.changeLanguage(resolved);
  syncRemoteLocale(resolved);
  return resolved;
}

// 远程输入 H5 录音页跟随 PC 界面语言：把已解析的 locale 推给后端（后端只存内存
// 镜像，H5 请求首页时据此渲染）。非 Tauri（浏览器 dev）环境走 mock no-op，失败静默。
function syncRemoteLocale(resolved: SupportedLocale): void {
  void setRemoteLocale(resolved).catch(() => {});
}
// 启动时同步一次当前语言，覆盖“开机即自动开启远程服务”的场景。
syncRemoteLocale(i18n.language as SupportedLocale);

export const FOLLOW_SYSTEM = FOLLOW_SYSTEM_VALUE;

export function outputPrefsForLocale(
  resolved: SupportedLocale,
): Pick<UserPreferences, 'chineseScriptPreference' | 'outputLanguagePreference'> {
  if (resolved === 'zh-CN') {
    return {
      chineseScriptPreference: 'simplified',
      outputLanguagePreference: 'zhCn',
    };
  }
  if (resolved === 'zh-TW') {
    return {
      chineseScriptPreference: 'traditional',
      outputLanguagePreference: 'zhTw',
    };
  }
  if (resolved === 'en') {
    return {
      chineseScriptPreference: 'auto',
      outputLanguagePreference: 'en',
    };
  }
  if (resolved === 'ja') {
    return {
      chineseScriptPreference: 'auto',
      outputLanguagePreference: 'ja',
    };
  }
  if (resolved === 'ko') {
    return {
      chineseScriptPreference: 'auto',
      outputLanguagePreference: 'ko',
    };
  }
  return {
    chineseScriptPreference: 'auto',
    outputLanguagePreference: 'auto',
  };
}
