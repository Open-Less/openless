const en = {
  title: 'This turn reference',
  add: 'Add a reference',
  attach: 'Attach',
  startWithReference: 'Start a new conversation with reference',
  text: 'Reference text',
  userProvided: 'Provided by you',
  originalInputAccessibility: 'Original page selection',
  permissionHint: 'Use the original page’s selection as a temporary Ask reference.',
  remove: 'Remove reference',
  close: 'Close',
  cancel: 'Cancel',
  fresh: 'New conversation',
  askReferenceEmpty: 'Enter a reference.',
  askReferenceLimit: 'Reference too long.',
  askReferenceNewConversationRequired: 'Start a new conversation.',
  askSessionChanged: 'Session ended.',
  askReferenceUnavailable: 'Reference unavailable.',
};

export const askTurnReferenceMessages: Record<string, typeof en> = {
  en,
  'zh-CN': {
    title: '本轮引用', add: '添加引用', attach: '引用', startWithReference: '引用并开始新对话', text: '引用文字',
    userProvided: '手工材料', originalInputAccessibility: '原页选区',
    permissionHint: '允许「随便问」将原页面的选区用作本轮临时引用。',
    remove: '撤回引用', close: '关闭', cancel: '取消', fresh: '新对话',
    askReferenceEmpty: '请输入引用。', askReferenceLimit: '引用过长。',
    askReferenceNewConversationRequired: '请开始新对话。', askSessionChanged: '会话已结束。', askReferenceUnavailable: '引用已失效。',
  },
  'zh-TW': {
    title: '本輪引用', add: '加入引用', attach: '引用', startWithReference: '引用並開始新對話', text: '引用文字',
    userProvided: '手動提供的資料', originalInputAccessibility: '原頁選取範圍',
    permissionHint: '允許「隨便問」將原頁面的選取範圍用於本輪暫時引用。',
    remove: '撤回引用', close: '關閉', cancel: '取消', fresh: '新對話',
    askReferenceEmpty: '請輸入引用。', askReferenceLimit: '引用過長。',
    askReferenceNewConversationRequired: '請開始新對話。', askSessionChanged: '對話已結束。', askReferenceUnavailable: '引用已失效。',
  },
  ja: {
    title: '今回の参照', add: '参照を追加', attach: '添付', startWithReference: '参照を添付して新しい会話を開始', text: '参照テキスト',
    userProvided: '手動で追加', originalInputAccessibility: '元のページの選択範囲',
    permissionHint: '元のページの選択範囲を、今回の質問の一時的な参照として使います。',
    remove: '参照を解除', close: '閉じる', cancel: 'キャンセル', fresh: '新しい会話',
    askReferenceEmpty: '参照を入力してください。', askReferenceLimit: '参照が長すぎます。',
    askReferenceNewConversationRequired: '新しい会話を開始してください。', askSessionChanged: '会話が終了しました。', askReferenceUnavailable: '参照を利用できません。',
  },
  ko: {
    title: '이번 질문의 참고 자료', add: '참고 자료 추가', attach: '첨부', startWithReference: '첨부하고 새 대화 시작', text: '참고 텍스트',
    userProvided: '직접 입력한 자료', originalInputAccessibility: '원본 페이지의 선택 영역',
    permissionHint: '원본 페이지의 선택 영역을 이번 질문의 임시 참고 자료로 사용합니다.',
    remove: '참고 자료 제거', close: '닫기', cancel: '취소', fresh: '새 대화',
    askReferenceEmpty: '참고 자료를 입력하세요.', askReferenceLimit: '참고 자료가 너무 깁니다.',
    askReferenceNewConversationRequired: '새 대화를 시작하세요.', askSessionChanged: '대화가 종료되었습니다.', askReferenceUnavailable: '참고 자료를 사용할 수 없습니다.',
  },
  es: {
    title: 'Referencia de este turno', add: 'Añadir referencia', attach: 'Adjuntar', startWithReference: 'Adjuntar e iniciar una conversación nueva', text: 'Texto de referencia',
    userProvided: 'Aportada por ti', originalInputAccessibility: 'Selección de la página original',
    permissionHint: 'Usa la selección de la página original como referencia temporal para la consulta.',
    remove: 'Quitar referencia', close: 'Cerrar', cancel: 'Cancelar', fresh: 'Nueva conversación',
    askReferenceEmpty: 'Introduce una referencia.', askReferenceLimit: 'La referencia es demasiado larga.',
    askReferenceNewConversationRequired: 'Inicia una conversación nueva.', askSessionChanged: 'La sesión terminó.', askReferenceUnavailable: 'La referencia ya no está disponible.',
  },
  fr: {
    title: 'Référence de cet échange', add: 'Ajouter une référence', attach: 'Joindre', startWithReference: 'Joindre et démarrer une nouvelle conversation', text: 'Texte de référence',
    userProvided: 'Fourni par vous', originalInputAccessibility: 'Sélection de la page d’origine',
    permissionHint: 'Utilise la sélection de la page d’origine comme référence temporaire pour la question.',
    remove: 'Retirer la référence', close: 'Fermer', cancel: 'Annuler', fresh: 'Nouvelle conversation',
    askReferenceEmpty: 'Saisissez une référence.', askReferenceLimit: 'La référence est trop longue.',
    askReferenceNewConversationRequired: 'Démarrez une nouvelle conversation.', askSessionChanged: 'La session est terminée.', askReferenceUnavailable: 'La référence est indisponible.',
  },
  de: {
    title: 'Referenz für diese Anfrage', add: 'Referenz hinzufügen', attach: 'Anhängen', startWithReference: 'Anhängen und neues Gespräch beginnen', text: 'Referenztext',
    userProvided: 'Von dir hinzugefügt', originalInputAccessibility: 'Auswahl auf der ursprünglichen Seite',
    permissionHint: 'Verwendet die Auswahl der ursprünglichen Seite als temporäre Referenz für die Anfrage.',
    remove: 'Referenz entfernen', close: 'Schließen', cancel: 'Abbrechen', fresh: 'Neues Gespräch',
    askReferenceEmpty: 'Gib eine Referenz ein.', askReferenceLimit: 'Die Referenz ist zu lang.',
    askReferenceNewConversationRequired: 'Beginne ein neues Gespräch.', askSessionChanged: 'Die Sitzung ist beendet.', askReferenceUnavailable: 'Die Referenz ist nicht verfügbar.',
  },
};

export function askReferenceLabels(locale: string): typeof en {
  if (/^zh-(TW|HK|MO|Hant)\b/i.test(locale)) return askTurnReferenceMessages['zh-TW'];
  const primary = locale.toLowerCase().split('-')[0];
  return primary === 'zh' ? askTurnReferenceMessages['zh-CN'] : askTurnReferenceMessages[primary] ?? en;
}
