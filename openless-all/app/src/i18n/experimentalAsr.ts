// Shared experimental-package copy; other locales use explicit English fallback.
export const experimentalAsrEn = {
  title: 'Experimental local speech models',
  retiredLabel: 'Removed',
  recommended: 'Recommended · Live streaming',
  textStreamLabel: 'TextStream (local · streaming)',
  textStreamDescription:
    'About 1.9 GB of model files plus isolated runtime components · Apple Silicon, macOS 14+ · multilingual live Draft / Finalized transcription. Setup does not load inference.',
  streamHint:
    'While recording, local apps may subscribe to http://localhost:7890/stream. Drafts can change; only the completed transcript is saved to OpenLess history. The worker and endpoint stop with the recording.',
  appleSiliconMac14Required: 'Requires Apple Silicon and macOS 14 or later.',
  description:
    'macOS local speech models are managed here. Enable to download and register a channel; inference loads only when recording starts.',
  memoryHint:
    'Using a local model increases memory usage. Opening Settings or restarting OpenLess does not load it.',
  retiredHint: 'This feature has been removed. Please go to Experiments & extensions.',
  deleteHint:
    'Switching off cancels the task, releases inference and removes its automatic channel. Downloaded files are kept; delete them separately when no longer needed.',
  senseVoiceDescription:
    'About 240 MB · Mandarin, Cantonese, English, Japanese and Korean · local CPU inference.',
  gemmaDescription:
    'About 715 MB · English only · Apple Silicon, macOS 15+ · Core ML. This is speech-to-text, not a language model or a multimodal generation provider. Recognition is experimental; results may contain errors.',
  desktopRuntimeRequired: 'This local inference runtime is not available on this platform yet.',
  appleSiliconMac15Required: 'Requires Apple Silicon and macOS 15 or later.',
  downloadProgress: '{{name}} download progress',
  openChannels: 'Choose this model in AI models & providers',
  retry: 'Retry',
  later: 'Later',
  confirm: 'OK, open speech channels',
  readyTitle: 'Downloaded — choose a speech channel to use it',
  readyBody:
    'The model is ready and its speech channel has been added to Settings → AI models & providers. Downloading does not replace your current channel. Enable the new channel if needed, then drag it to the top of the speech recognition list.',
  pipelineHint:
    'These models work in the traditional speech recognition pipeline. Continuing opens that pipeline and its speech channel list; your language model channel is unchanged.',
  manageHint:
    'This channel is managed in Experiments & extensions → Experimental local speech models. No API key or endpoint is needed.',
  channelsHint:
    'To use a downloaded experimental model, enable its channel and drag it to the top. The top enabled speech channel is used.',
  senseVoiceLabel: 'SenseVoice Small (local experiment)',
  gemmaLabel: 'Embedding Gemma (local · English)',
  phases: {
    disabled: 'Not enabled',
    downloading: 'Downloading',
    preparing: 'Installing runtime components and checking channel metadata',
    ready: 'Downloaded · channel ready',
    removing: 'Stopping / cleaning up',
    failed: 'Could not finish — retry or switch off to clean up',
  },
};
export const experimentalAsrZhCN: typeof experimentalAsrEn = {
  title: '实验性本地语音模型',
  retiredLabel: '已下架',
  recommended: '推荐 · 实时流式',
  textStreamLabel: 'TextStream（本地 · 流式）',
  textStreamDescription:
    '模型文件约 1.9 GB，首次开启还需下载独立运行组件 · Apple Silicon、macOS 14 及以上 · 多语种实时草稿与确认文本。安装过程不加载推理。',
  streamHint:
    '录音期间，本机应用可订阅 http://localhost:7890/stream。草稿可能修订，整段确认后保存至 OpenLess 历史；结束录音即退出运行进程和接口。',
  appleSiliconMac14Required: '需要 Apple Silicon 芯片及 macOS 14 或更高版本。',
  description: 'macOS 本地语音模型统一在此管理。开启后下载并注册渠道，开始录音时才加载推理引擎。',
  memoryHint: '使用本地模型会增加内存占用。打开设置或重启应用不会加载模型。',
  retiredHint: '该功能已下架，请去实验性与扩展里面',
  deleteHint:
    '关闭开关会取消任务、释放推理引擎并移除自动渠道，保留已下载文件；不再需要时可单独删除模型。',
  senseVoiceDescription: '约 240 MB · 普通话、粤语、英语、日语、韩语 · 本地 CPU 推理。',
  gemmaDescription:
    '约 715 MB · 仅英语 · Apple Silicon、macOS 15 及以上 · Core ML 推理。这是语音转文字模型，不是语言模型或多模态生成服务。识别效果仍属实验性，可能出现错词。',
  desktopRuntimeRequired: '此平台暂未接入该本地推理引擎。',
  appleSiliconMac15Required: '需要 Apple Silicon 芯片及 macOS 15 或更高版本。',
  downloadProgress: '{{name}} 下载进度',
  openChannels: '前往 AI 模型与服务商选择使用',
  retry: '重试',
  later: '稍后再说',
  confirm: '确定，前往语音渠道',
  readyTitle: '下载完成，还需选择语音渠道',
  readyBody:
    '模型已准备好，并已在「设置 → AI 模型与服务商」添加语音识别渠道。下载完成不代表已经替换当前渠道。请按需开启新渠道，再将它拖到语音识别列表最上方。',
  pipelineHint:
    '这些模型用于传统语音识别管线。继续后会打开该管线的语音渠道列表，不改变语言模型渠道。',
  manageHint: '此渠道由「实验性与扩展 → 实验性本地语音模型」管理，无需填写 API Key 或服务地址。',
  channelsHint:
    '要使用刚下载的实验模型，请开启对应渠道并拖到最上方。排在最前的已开启语音渠道才会生效。',
  senseVoiceLabel: 'SenseVoice Small（本地实验）',
  gemmaLabel: 'Embedding Gemma（本地 · 英语）',
  phases: {
    disabled: '未开启',
    downloading: '正在下载',
    preparing: '正在安装运行组件并检查渠道配置',
    ready: '已下载 · 渠道已就绪',
    removing: '正在停用或清理',
    failed: '未完成，可重试或关闭开关清理',
  },
};
export const experimentalAsrZhTW: typeof experimentalAsrEn = {
  title: '實驗性本機語音模型',
  retiredLabel: '已下架',
  recommended: '推薦 · 即時串流',
  textStreamLabel: 'TextStream（本機 · 串流）',
  textStreamDescription:
    '模型約 1.9 GB，首次啟用另需下載獨立執行元件 · Apple Silicon、macOS 14 以上 · 多語言即時草稿與確認文字。安裝不載入推論。',
  streamHint:
    '錄音時本機應用可訂閱 http://localhost:7890/stream。草稿可修訂，整段確認後儲存至歷史；結束即退出執行程序與介面。',
  appleSiliconMac14Required: '需要 Apple Silicon 晶片及 macOS 14 或更新版本。',
  description: 'macOS 本機語音模型統一在此管理。開啟後下載並註冊管道，開始錄音時才載入推論引擎。',
  memoryHint: '使用本機模型會增加記憶體占用。開啟設定或重新啟動應用程式不會載入模型。',
  retiredHint: '該功能已下架，請去實驗性與擴充裡面',
  deleteHint:
    '關閉會取消工作、釋放推論引擎並移除自動管道，保留已下載檔案；不再需要時可另行刪除模型。',
  senseVoiceDescription: '約 240 MB · 國語、粵語、英語、日語、韓語 · 本機 CPU 推論。',
  gemmaDescription:
    '約 715 MB · 僅英語 · Apple Silicon、macOS 15 以上 · Core ML 推論。這是語音轉文字模型，不是語言模型或多模態生成服務。辨識效果仍屬實驗性，可能出現錯詞。',
  desktopRuntimeRequired: '此平台尚未接入該本機推論引擎。',
  appleSiliconMac15Required: '需要 Apple Silicon 晶片及 macOS 15 或更新版本。',
  downloadProgress: '{{name}} 下載進度',
  openChannels: '前往 AI 模型與服務商選擇使用',
  retry: '重試',
  later: '稍後再說',
  confirm: '確定，前往語音管道',
  readyTitle: '下載完成，還需選擇語音管道',
  readyBody:
    '模型已準備好，並已在「設定 → AI 模型與服務商」新增語音辨識管道。下載完成不代表已替換目前的管道。請按需開啟新管道，再將它拖到語音辨識清單最上方。',
  pipelineHint:
    '這些模型用於傳統語音辨識管線。繼續後會開啟該管線的語音管道清單，不改變語言模型管道。',
  manageHint: '此管道由「實驗性與擴充 → 實驗性本機語音模型」管理，不需填寫 API Key 或服務位址。',
  channelsHint:
    '要使用剛下載的實驗模型，請開啟對應管道並拖到最上方。排在最前的已開啟語音管道才會生效。',
  senseVoiceLabel: 'SenseVoice Small（本機實驗）',
  gemmaLabel: 'Embedding Gemma（本機 · 英語）',
  phases: {
    disabled: '未開啟',
    downloading: '正在下載',
    preparing: '正在安裝執行元件並檢查管道設定',
    ready: '已下載 · 管道已就緒',
    removing: '正在停用或清理',
    failed: '未完成，可重試或關閉以清理',
  },
};
