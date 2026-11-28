pub(crate) fn original_recovery_message(
    locale: &str,
    original: &str,
    inserted: bool,
) -> Option<&'static str> {
    if !inserted || original.chars().count() <= 4000 {
        return None;
    }
    Some(localized_message(locale))
}

pub(crate) fn capacity_failure_message(locale: &str) -> &'static str {
    match locale {
        "zh-CN" => "服务无法完整处理全文，未替换；原文已保存在 App 的历史记录",
        "zh-TW" => "服務無法完整處理全文，未替換；原文已儲存在 App 的歷史記錄",
        "ja" => "全文を処理できませんでした。置換せず、原文をアプリの履歴に保存しました。",
        "ko" => "전체 텍스트를 처리할 수 없어 바꾸지 않았습니다. 원문은 앱 기록에 저장했습니다.",
        "es" => "El servicio no pudo procesar todo el texto. No se reemplazó; el original está en el historial.",
        "fr" => "Le service n’a pas pu traiter le texte entier. Aucun remplacement ; l’original est dans l’historique.",
        "de" => "Der Dienst konnte den vollständigen Text nicht verarbeiten. Nichts ersetzt; das Original ist im Verlauf.",
        _ => "The service could not process the full text. Nothing replaced; the original is saved in app history.",
    }
}

pub(crate) fn uncertain_write_message(locale: &str) -> &'static str {
    match locale {
        "zh-CN" => "可能已写入，请先核对原输入框；原文和结果保留在历史记录",
        "zh-TW" => "可能已寫入，請先核對原輸入框；原文和結果保留在歷史記錄",
        "ja" => "入力済みの可能性があります。元の欄を確認してください。原文と結果は履歴にあります。",
        "ko" => "이미 입력되었을 수 있습니다. 원래 입력란을 확인하세요. 원문과 결과는 기록에 있습니다.",
        "es" => "Puede que el texto ya se haya insertado. Comprueba el campo original; ambos textos están en el historial.",
        "fr" => "Le texte a peut-être été inséré. Vérifiez le champ d’origine ; les deux textes sont dans l’historique.",
        "de" => "Der Text wurde möglicherweise eingefügt. Prüfe das ursprüngliche Feld; beide Texte sind im Verlauf.",
        _ => "The text may already be inserted. Check the original field first; both texts are saved in history.",
    }
}

pub(crate) fn no_selection_message(locale: &str) -> &'static str {
    match locale {
        "zh-CN" => "未读取到选区，请选择文字后重试",
        "zh-TW" => "未讀取到選取範圍，請選取文字後重試",
        "ja" => "選択範囲を読み取れませんでした。文字を選択して再試行してください。",
        "ko" => "선택 영역을 읽지 못했습니다. 텍스트를 선택한 후 다시 시도하세요.",
        "es" => "No se pudo leer la selección. Selecciona texto y vuelve a intentarlo.",
        "fr" => "Impossible de lire la sélection. Sélectionnez du texte et réessayez.",
        "de" => "Die Auswahl konnte nicht gelesen werden. Text auswählen und erneut versuchen.",
        _ => "Could not read a selection. Select text and try again.",
    }
}

fn localized_message(locale: &str) -> &'static str {
    match locale {
        "zh-CN" => "如果有掉词，可前往 App 的历史记录获取原文。",
        "zh-TW" => "如果有漏字，可前往 App 的歷史記錄取得原文。",
        "ja" => "抜けた言葉がある場合は、アプリの履歴から原文を取得できます。",
        "ko" => "누락된 단어가 있으면 앱의 기록에서 원문을 확인할 수 있습니다.",
        "es" => "Si faltan palabras, recupera el original en el historial de la app.",
        "fr" => "Si des mots manquent, retrouvez le texte original dans l’historique de l’app.",
        "de" => "Falls Wörter fehlen, findest du das Original im Verlauf der App.",
        _ => "If any words are missing, retrieve the original from the app’s history.",
    }
}
