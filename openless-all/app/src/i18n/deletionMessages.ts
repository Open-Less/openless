import type { SupportedLocale } from './index';
import type { zhCN } from './zh-CN';

const en = {
  history: {
    confirmClearLocal:
      'Permanently clear history on this device? OpenLess also attempts to remove the associated recordings. Quick Notes and cumulative activity are kept. If encrypted sync is enabled, deletion of synced text can reach other devices.',
    confirmDeleteLocal:
      'Permanently delete this entry? OpenLess also attempts to remove any associated recording from this device. If encrypted sync is enabled, deletion of synced text can reach other devices.',
    cleared: 'History cleared',
    deleted: 'Entry deleted',
  },
  usage: {
    clear: 'Clear recorded tokens',
    clearScope:
      'Permanently clear this device’s usage ledger? Dictation history and cumulative activity are kept.',
    clearFailed: 'Could not confirm that the local usage ledger was cleared.',
    cleared: 'Local usage ledger cleared',
  },
  vocab: {
    deleteWord: 'Delete “{{phrase}}”?',
    deleteLearned: 'Delete all collected words ({{count}})?',
    deleteScope:
      'Remove the specified entries from this device’s dictionary? If encrypted sync is enabled, the deletion can sync to other devices. This cannot be undone.',
    deleteIncomplete: 'Retry to delete the remaining dictionary entries ({{count}}).',
    deleted: 'Words deleted',
    corrections: {
      deleteLearnedTarget: '{{count}} automatically collected rules',
      deleteScope:
        'Target: {{target}}. Remove these rules from this device? If encrypted sync is enabled, the deletion can sync to other devices. This cannot be undone.',
      deleteIncomplete: 'Retry to delete the remaining correction rules ({{count}}).',
      deleted: 'Correction rules deleted',
    },
  },
  stylePack: {
    resetBuiltinScope:
      'Permanently replace the saved customizations for “{{name}}” with its defaults? If encrypted sync is enabled, the reset can sync to other devices.',
    deleteLocalScope:
      'Permanently delete “{{name}}” from this device? If encrypted sync is enabled, the deletion can sync to other devices. Any marketplace listing is kept.',
  },
  marketplaceDetail: {
    withdrawScope:
      'Permanently withdraw “{{name}}” from the marketplace? Previously installed copies are kept.',
  },
  expressionLearning: {
    clearScope: 'Permanently delete learned expression habits on this device? Dictation history is kept.',
  },
  channels: {
    deleteScope:
      'Permanently delete “{{name}}” and its stored credentials from this device? If encrypted sync is enabled, deletion of synced channels can reach other devices.',
  },
  localAsr: {
    cleanupScope:
      'Permanently delete the incomplete download for {{name}} from this device? Installed models are kept.',
    cleanupBatchScope:
      'Permanently delete the incomplete downloads on this device ({{count}})? Installed models are kept.',
    cleanupIncompleteError: 'Retry to delete the remaining incomplete downloads ({{count}}).',
  },
};

const zhHans: typeof en = {
  history: {
    confirmClearLocal:
      '永久清空本机历史记录？OpenLess 也会尝试删除相关录音。速记与累计活动数据会保留。如果已开启加密同步，已同步的文本记录也可能从其他设备删除。',
    confirmDeleteLocal:
      '永久删除这条记录？OpenLess 也会尝试删除本机上的相关录音。如果已开启加密同步，已同步的文本记录也可能从其他设备删除。',
    cleared: '已清空历史',
    deleted: '已删除记录',
  },
  usage: {
    clear: '清除已记录 Token',
    clearScope: '永久清除本机用量账本中的记录？听写历史和累计活动数据会保留。',
    clearFailed: '无法确认本机用量账本已清除。',
    cleared: '已清除本地账本',
  },
  vocab: {
    deleteWord: '删除「{{phrase}}」？',
    deleteLearned: '删除全部已收集的词条（{{count}} 条）？',
    deleteScope: '从本机词库中删除这些词条？如果已开启加密同步，删除可能同步到其他设备。此操作无法撤销。',
    deleteIncomplete: '请重试以删除剩余的 {{count}} 条词条。',
    deleted: '已删除词条',
    corrections: {
      deleteLearnedTarget: '{{count}} 条自动收集的规则',
      deleteScope: '删除目标：{{target}}。从本机删除这些规则？如果已开启加密同步，删除可能同步到其他设备。此操作无法撤销。',
      deleteIncomplete: '请重试以删除剩余的 {{count}} 条纠正规则。',
      deleted: '已删除纠正规则',
    },
  },
  stylePack: {
    resetBuiltinScope: '恢复「{{name}}」的默认设置并永久清除已保存的自定义设置？如果已开启加密同步，重置可能同步到其他设备。',
    deleteLocalScope: '从本机永久删除「{{name}}」？如果已开启加密同步，删除可能同步到其他设备。市场上已发布的版本会保留。',
  },
  marketplaceDetail: {
    withdrawScope: '从市场永久下架「{{name}}」？已安装的副本会保留。',
  },
  expressionLearning: {
    clearScope: '永久删除本机学习到的表达习惯？听写历史会保留。',
  },
  channels: {
    deleteScope: '从本机永久删除「{{name}}」和已保存的凭据？如果已开启加密同步，已同步的渠道也可能从其他设备删除。',
  },
  localAsr: {
    cleanupScope: '永久删除本机上 {{name}} 未下载完成的文件？已安装的模型会保留。',
    cleanupBatchScope: '永久删除本机上 {{count}} 项未完成的下载？已安装的模型会保留。',
    cleanupIncompleteError: '请重试以删除剩余的 {{count}} 项未完成的下载。',
  },
};

const zhHant: typeof en = {
  history: {
    confirmClearLocal:
      '永久清空本機歷史記錄？OpenLess 也會嘗試刪除相關錄音。速記與累計活動資料會保留。若已開啟加密同步，已同步的文字記錄也可能從其他裝置刪除。',
    confirmDeleteLocal:
      '永久刪除這筆記錄？OpenLess 也會嘗試刪除本機上的相關錄音。若已開啟加密同步，已同步的文字記錄也可能從其他裝置刪除。',
    cleared: '已清空歷史',
    deleted: '已刪除記錄',
  },
  usage: {
    clear: '清除已記錄 Token',
    clearScope: '永久清除本機用量帳本中的記錄？聽寫歷史與累計活動資料會保留。',
    clearFailed: '無法確認本機用量帳本已清除。',
    cleared: '已清除本機帳本',
  },
  vocab: {
    deleteWord: '刪除「{{phrase}}」？',
    deleteLearned: '刪除全部已收集的詞條（{{count}} 條）？',
    deleteScope: '從本機詞庫刪除這些詞條？若已開啟加密同步，刪除可能同步到其他裝置。此操作無法復原。',
    deleteIncomplete: '請重試以刪除剩餘的 {{count}} 條詞條。',
    deleted: '已刪除詞條',
    corrections: {
      deleteLearnedTarget: '{{count}} 條自動收集的規則',
      deleteScope: '刪除目標：{{target}}。從本機刪除這些規則？若已開啟加密同步，刪除可能同步到其他裝置。此操作無法復原。',
      deleteIncomplete: '請重試以刪除剩餘的 {{count}} 條糾正規則。',
      deleted: '已刪除糾正規則',
    },
  },
  stylePack: {
    resetBuiltinScope: '恢復「{{name}}」的預設設定並永久清除已儲存的自訂設定？若已開啟加密同步，重設可能同步到其他裝置。',
    deleteLocalScope: '從本機永久刪除「{{name}}」？若已開啟加密同步，刪除可能同步到其他裝置。市場上已發布的版本會保留。',
  },
  marketplaceDetail: {
    withdrawScope: '從市場永久下架「{{name}}」？已安裝的副本會保留。',
  },
  expressionLearning: {
    clearScope: '永久刪除本機學到的表達習慣？聽寫歷史會保留。',
  },
  channels: {
    deleteScope: '從本機永久刪除「{{name}}」和已儲存的憑證？若已開啟加密同步，已同步的渠道也可能從其他裝置刪除。',
  },
  localAsr: {
    cleanupScope: '永久刪除本機上 {{name}} 未下載完成的檔案？已安裝的模型會保留。',
    cleanupBatchScope: '永久刪除本機上 {{count}} 項未完成的下載？已安裝的模型會保留。',
    cleanupIncompleteError: '請重試以刪除剩餘的 {{count}} 項未完成的下載。',
  },
};

export function withDeletionMessages(base: typeof zhCN, locale: SupportedLocale) {
  // Preserve the existing English fallback for deletion copy in other languages.
  const messages = locale === 'zh-CN' ? zhHans : locale === 'zh-TW' ? zhHant : en;
  return {
    ...base,
    history: { ...base.history, ...messages.history },
    usage: messages.usage,
    vocab: {
      ...base.vocab,
      ...messages.vocab,
      corrections: { ...base.vocab.corrections, ...messages.vocab.corrections },
    },
    style: { ...base.style, pack: { ...base.style.pack, ...messages.stylePack } },
    marketplace: {
      ...base.marketplace,
      detail: { ...base.marketplace.detail, ...messages.marketplaceDetail },
    },
    settings: {
      ...base.settings,
      channels: { ...base.settings.channels, ...messages.channels },
      expressionLearning: { ...base.settings.expressionLearning, ...messages.expressionLearning },
    },
    localAsr: { ...base.localAsr, ...messages.localAsr },
  };
}
