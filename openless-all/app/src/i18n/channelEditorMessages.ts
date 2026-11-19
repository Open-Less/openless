const messages = {
  en: {
    save: 'Save',
    saved: 'Channel saved',
    saveFailed: 'Could not save the channel. Please try again.',
    deleting: 'Deleting channel…',
    deleteFailed: 'Could not delete the channel. Please try again.',
  },
  'zh-CN': {
    save: '保存',
    saved: '渠道已保存',
    saveFailed: '保存渠道失败，请重试。',
    deleting: '正在删除渠道…',
    deleteFailed: '删除渠道失败，请重试。',
  },
  'zh-TW': {
    save: '儲存',
    saved: '渠道已儲存',
    saveFailed: '儲存渠道失敗，請重試。',
    deleting: '正在刪除渠道…',
    deleteFailed: '刪除渠道失敗，請重試。',
  },
  ja: {
    save: '保存',
    saved: 'チャンネルを保存しました',
    saveFailed: 'チャンネルを保存できませんでした。もう一度お試しください。',
    deleting: 'チャンネルを削除しています…',
    deleteFailed: 'チャンネルを削除できませんでした。もう一度お試しください。',
  },
  ko: {
    save: '저장',
    saved: '채널이 저장되었습니다',
    saveFailed: '채널을 저장하지 못했습니다. 다시 시도해 주세요.',
    deleting: '채널을 삭제하는 중…',
    deleteFailed: '채널을 삭제하지 못했습니다. 다시 시도해 주세요.',
  },
  es: {
    save: 'Guardar',
    saved: 'Canal guardado',
    saveFailed: 'No se pudo guardar el canal. Inténtalo de nuevo.',
    deleting: 'Eliminando canal…',
    deleteFailed: 'No se pudo eliminar el canal. Inténtalo de nuevo.',
  },
  fr: {
    save: 'Enregistrer',
    saved: 'Canal enregistré',
    saveFailed: 'Impossible d’enregistrer le canal. Réessayez.',
    deleting: 'Suppression du canal…',
    deleteFailed: 'Impossible de supprimer le canal. Réessayez.',
  },
  de: {
    save: 'Speichern',
    saved: 'Kanal gespeichert',
    saveFailed: 'Der Kanal konnte nicht gespeichert werden. Bitte erneut versuchen.',
    deleting: 'Kanal wird gelöscht…',
    deleteFailed: 'Der Kanal konnte nicht gelöscht werden. Bitte erneut versuchen.',
  },
};

export function channelEditorMessages(locale: string) {
  return messages[locale as keyof typeof messages] ?? messages.en;
}
