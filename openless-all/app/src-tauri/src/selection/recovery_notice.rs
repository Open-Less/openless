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
