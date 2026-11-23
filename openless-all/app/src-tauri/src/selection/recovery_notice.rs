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
