import type { SupportedLocale } from './index';

type DownloadMessages = { downloading: string; preparing: string; ready: string };

export const modelDownloadMessages: Record<SupportedLocale, DownloadMessages> = {
  'zh-CN': {
    downloading: '正在下载 {{name}}',
    preparing: '正在准备 {{name}}',
    ready: '{{name}} 已就绪',
  },
  'zh-TW': {
    downloading: '正在下載 {{name}}',
    preparing: '正在準備 {{name}}',
    ready: '{{name}} 已就緒',
  },
  en: {
    downloading: 'Downloading {{name}}',
    preparing: 'Preparing {{name}}',
    ready: '{{name}} is ready',
  },
  ja: {
    downloading: '{{name}} をダウンロード中',
    preparing: '{{name}} を準備中',
    ready: '{{name}} の準備完了',
  },
  ko: {
    downloading: '{{name}} 다운로드 중',
    preparing: '{{name}} 준비 중',
    ready: '{{name}} 준비 완료',
  },
  es: {
    downloading: 'Descargando {{name}}',
    preparing: 'Preparando {{name}}',
    ready: '{{name}} está listo',
  },
  fr: {
    downloading: 'Téléchargement de {{name}}',
    preparing: 'Préparation de {{name}}',
    ready: '{{name}} est prêt',
  },
  de: {
    downloading: '{{name}} wird heruntergeladen',
    preparing: '{{name}} wird vorbereitet',
    ready: '{{name}} ist bereit',
  },
};
