/// 靜態載入語系特定的風格預設 JSON 資源。
/// 在編譯時由 include_str! 嵌入，避免執行時檔案讀取。

use serde_json;

/// JSON 資源中一個風格模式的定義（中間格式，不含 Tauri 執行時欄位）
#[derive(Debug, Clone)]
pub struct StylePackJsonDef {
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub examples: Vec<StylePackJsonExample>,
    pub tags: Vec<String>,
}

/// JSON 範例定義
#[derive(Debug, Clone)]
pub struct StylePackJsonExample {
    pub title: Option<String>,
    pub input: String,
    pub output: String,
}

/// 取得指定語言的風格預設 JSON 字串。
/// 若語言不支援，退回簡體中文預設。
fn get_style_pack_defaults_json(lang: &str) -> &'static str {
    match lang {
        "zh-CN" => include_str!("style_pack_defaults/zh-CN.json"),
        "zh-TW" => include_str!("style_pack_defaults/zh-TW.json"),
        "en" => include_str!("style_pack_defaults/en.json"),
        // 日文、韓文今日尚未建立，暫時退回英文
        "ja" | "ko" => include_str!("style_pack_defaults/en.json"),
        _ => include_str!("style_pack_defaults/zh-CN.json"),
    }
}

/// 從 JSON 資源分析一個風格模式的定義。
/// mode_key 應為 "raw" | "light" | "structured" | "formal"
pub fn load_style_pack_json_def(lang: &str, mode_key: &str) -> Option<StylePackJsonDef> {
    let json_str = get_style_pack_defaults_json(lang);
    let json_value: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let mode_obj = &json_value[mode_key];
    if mode_obj.is_null() {
        return None;
    }

    let name = mode_obj["name"].as_str()?.to_string();
    let description = mode_obj["description"].as_str().unwrap_or("").to_string();
    let prompt = mode_obj["prompt"].as_str().unwrap_or("").to_string();

    let examples = mode_obj["examples"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|ex| {
            let input = ex["input"].as_str()?.to_string();
            let output = ex["output"].as_str()?.to_string();
            let title = ex["title"].as_str().map(|s| s.to_string());
            Some(StylePackJsonExample { title, input, output })
        })
        .collect();

    let tags = mode_obj["tags"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|t| t.as_str().map(|s| s.to_string()))
        .collect();

    Some(StylePackJsonDef { name, description, prompt, examples, tags })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_zh_cn_raw() {
        let def = load_style_pack_json_def("zh-CN", "raw").expect("should load zh-CN raw");
        assert_eq!(def.name, "原文");
        assert!(!def.prompt.is_empty());
        assert!(!def.examples.is_empty());
    }

    #[test]
    fn test_load_zh_tw_raw() {
        let def = load_style_pack_json_def("zh-TW", "raw").expect("should load zh-TW raw");
        assert_eq!(def.name, "原文");
        // 繁體 prompt 應含繁體字
        assert!(def.prompt.contains("語音"));
    }

    #[test]
    fn test_load_en_formal() {
        let def = load_style_pack_json_def("en", "formal").expect("should load en formal");
        assert_eq!(def.name, "Formal");
    }

    #[test]
    fn test_unsupported_language_fallback() {
        // 不支持的語言應退回簡體
        let def = load_style_pack_json_def("xx-XX", "light").expect("should fallback to zh-CN");
        assert_eq!(def.name, "轻度润色");
    }

    #[test]
    fn test_all_modes_all_languages() {
        for lang in &["zh-CN", "zh-TW", "en"] {
            for mode in &["raw", "light", "structured", "formal"] {
                let def = load_style_pack_json_def(lang, mode)
                    .unwrap_or_else(|| panic!("should load {} {}", lang, mode));
                assert!(!def.name.is_empty());
                assert!(!def.prompt.is_empty());
            }
        }
    }
}

