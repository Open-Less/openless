//! 最小差异学习算法 —— 纯函数，无平台依赖。
//!
//! 我们刚往用户光标处插了一段文字，用户随手改了一个词。这个模块负责从「改之前」和
//! 「改之后」两段文本里，把那个词单独抠出来：`(source, target)`。
//!
//! ## 为什么是「最小」差异
//!
//! 整段对比会得到「原文 → 新文」这种毫无用处的规则。真正有价值的是**最短的那一处
//! 改动**：「大禹 → 大鱼」能沉淀成词库，「上面那一整句 → 下面那一整句」不能。
//! 所以先剥掉公共前缀、再剥掉公共后缀，剩下的中间段才是用户真正动的地方。
//!
//! ## 六条边界，一条都不能省
//!
//! 每一条都对应一类会污染词库的假阳性 —— 见 [`minimal_edit`] 上的逐条说明。学错的
//! 规则会静默地改掉用户以后所有的听写，代价远高于漏学一条。
//!
//! 全部按 char 计数，不按字节。

/// 允许学习的最大改动长度（char）。
///
/// 超过这个长度的差异几乎一定是「用户重写了这句话」而不是「用户纠了一个词」，
/// 把它当规则收进去只会在下次听写时命中一大段不相关的文本。
const MAX_EDIT_CHARS: usize = 64;

/// 改动点前后各保留多少字作为上下文。
///
/// 留着是为了里程碑 4 做归因（这次改动到底是 ASR 听错还是 LLM 改坏），以及让用户在
/// 确认界面上能看懂「这条规则是从哪句话里学来的」。
const CONTEXT_CHARS: usize = 256;

/// 一处最小改动。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPair {
    /// 改之前的那几个字（恒非空）。
    pub source: String,
    /// 改之后的那几个字（可能为空 —— 纯删除）。
    pub target: String,
    /// 改动点之前最多 [`CONTEXT_CHARS`] 个字。
    pub before: String,
    /// 改动点之后最多 [`CONTEXT_CHARS`] 个字。
    pub after: String,
}

/// 从「改之前 → 改之后」里抠出最小改动；不值得学的一律返回 `None`。
///
/// 拒绝的六种情况，按判定顺序：
///
/// 1. **两段完全相同** —— 没有改动。
/// 2. **`source` 为空（纯插入）** —— 用户只是在补字，不是在纠错。把「空 → 某某」当成
///    规则等于在全局做无条件插入，是最危险的一类假阳性。
/// 3. **`source` 或 `target` 超过 [`MAX_EDIT_CHARS`]** —— 那是重写，不是纠错。
/// 4. **`source` 只由空白构成** —— 排版调整（多打了个空格、换行），没有词汇价值。
/// 5. **`source` 与 `target` 去掉空白后相同** —— 同样是排版调整（「大 鱼」→「大鱼」）。
/// 6. **两段文本都为空** —— 由第 1 条兜住。
///
/// 注意**纯删除是允许学的**（`target` 为空）：「把多余的『的』删掉」是有意义的纠正，
/// 而且它不会像纯插入那样在任何位置无条件触发。
pub fn minimal_edit(before_text: &str, after_text: &str) -> Option<EditPair> {
    if before_text == after_text {
        return None;
    }

    let old: Vec<char> = before_text.chars().collect();
    let new: Vec<char> = after_text.chars().collect();

    // 1) 最长公共前缀。
    let prefix_len = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // 2) 排除前缀之后，再算最长公共后缀。两侧剩余长度都要减去前缀，避免在
    //    "aa" → "aaa" 这类重叠情况下前后缀互相吃掉对方。
    let max_suffix = (old.len() - prefix_len).min(new.len() - prefix_len);
    let suffix_len = (0..max_suffix)
        .take_while(|i| old[old.len() - 1 - i] == new[new.len() - 1 - i])
        .count();

    // 3) 中间段就是用户真正动的地方。
    let source: String = old[prefix_len..old.len() - suffix_len].iter().collect();
    let target: String = new[prefix_len..new.len() - suffix_len].iter().collect();

    // 4) source 必须非空 —— 纯插入不学。
    if source.is_empty() {
        return None;
    }
    // 5) 超长的是重写不是纠错。
    let source_chars = source.chars().count();
    let target_chars = target.chars().count();
    if source_chars.max(target_chars) > MAX_EDIT_CHARS {
        return None;
    }
    // 6) 纯排版调整没有词汇价值。
    if source.trim().is_empty() {
        return None;
    }
    if strip_whitespace(&source) == strip_whitespace(&target) {
        return None;
    }

    let before: String = old[prefix_len.saturating_sub(CONTEXT_CHARS)..prefix_len]
        .iter()
        .collect();
    let after_start = old.len() - suffix_len;
    let after: String = old[after_start..(after_start + CONTEXT_CHARS).min(old.len())]
        .iter()
        .collect();

    Some(EditPair {
        source,
        target,
        before,
        after,
    })
}

fn strip_whitespace(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 这处改动是不是落在「我们刚插进去的那段文字」里。
///
/// 观察器盯的是整个控件，用户在文档别处改自己的旧内容照样会触发通知。那种改动跟本次
/// 听写毫无关系，学进来纯属噪声 —— 而噪声进了词库就会去改用户以后所有的听写。
///
/// 抽成纯函数是为了能脱离 AXObserver 测：这条判据是「只学我们自己的错」与「见什么学
/// 什么」之间唯一的分界线。
pub fn edit_is_within_typed_text(edit: &EditPair, typed_text: &str) -> bool {
    !edit.source.is_empty() && typed_text.contains(&edit.source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(before: &str, after: &str) -> Option<(String, String)> {
        minimal_edit(before, after).map(|e| (e.source, e.target))
    }

    #[test]
    fn extracts_a_single_changed_word() {
        assert_eq!(
            edit("今天讲一下大禹的养殖", "今天讲一下大鱼的养殖"),
            Some(("禹".to_string(), "鱼".to_string()))
        );
    }

    #[test]
    fn extracts_a_cross_script_correction() {
        assert_eq!(
            edit("我们用扣德克斯写代码", "我们用 Codex 写代码"),
            Some(("扣德克斯".to_string(), " Codex ".to_string()))
        );
    }

    #[test]
    fn identical_text_is_not_an_edit() {
        assert_eq!(edit("完全一样", "完全一样"), None);
        assert_eq!(edit("", ""), None);
    }

    #[test]
    fn pure_insertion_is_rejected() {
        // 用户只是在补字。学成规则就是「在任意位置无条件插入」，最危险的假阳性。
        assert_eq!(edit("这个接口", "这个接口设计"), None);
        assert_eq!(edit("", "全新内容"), None);
        assert_eq!(edit("前后", "前中后"), None);
    }

    #[test]
    fn pure_deletion_is_learned() {
        // 删除和插入不对称：删除是「这里不该有这个词」，有明确语义且不会到处触发。
        assert_eq!(
            edit("这个的接口设计", "这个接口设计"),
            Some(("的".to_string(), String::new()))
        );
    }

    #[test]
    fn an_edit_longer_than_the_cap_is_rejected() {
        let before = "开头".to_string() + &"甲".repeat(65) + "结尾";
        let after = "开头".to_string() + &"乙".repeat(65) + "结尾";
        assert_eq!(edit(&before, &after), None);
    }

    #[test]
    fn an_edit_exactly_at_the_cap_is_accepted() {
        let before = "开头".to_string() + &"甲".repeat(64) + "结尾";
        let after = "开头".to_string() + &"乙".repeat(64) + "结尾";
        let (source, target) = edit(&before, &after).expect("64 字应当仍在可学范围内");
        assert_eq!(source.chars().count(), 64);
        assert_eq!(target.chars().count(), 64);
    }

    #[test]
    fn a_long_source_replaced_by_a_short_target_is_still_rejected() {
        // 上限看的是两侧的最大值，不是差值 —— 「删掉一大段」也是重写。
        let before = "开头".to_string() + &"甲".repeat(100) + "结尾";
        assert_eq!(edit(&before, "开头乙结尾"), None);
    }

    #[test]
    fn whitespace_only_changes_are_rejected() {
        // 排版调整没有词汇价值。
        assert_eq!(edit("大 鱼", "大鱼"), None);
        assert_eq!(edit("一句话  另一句", "一句话 另一句"), None);
    }

    #[test]
    fn no_common_prefix_or_suffix_yields_the_whole_texts() {
        assert_eq!(
            edit("甲乙丙", "丁戊己"),
            Some(("甲乙丙".to_string(), "丁戊己".to_string()))
        );
    }

    #[test]
    fn whole_text_replaced_by_empty_is_a_deletion() {
        assert_eq!(
            edit("整段删光", ""),
            Some(("整段删光".to_string(), String::new()))
        );
    }

    #[test]
    fn overlapping_prefix_and_suffix_do_not_double_count() {
        // "aa" → "aaa"：前缀吃掉 2、后缀若不设上限会再吃 2，中间段会算出负长度。
        assert_eq!(edit("aa", "aaa"), None); // 纯插入，被拒
        assert_eq!(
            edit("aaa", "aa"),
            Some(("a".to_string(), String::new()))
        );
    }

    #[test]
    fn cjk_is_counted_by_char_not_by_byte() {
        // 每个汉字 3 字节。按字节算前后缀会切出无效 UTF-8 或错位的边界。
        let pair = minimal_edit("接口设计文档", "借口设计文档").unwrap();
        assert_eq!(pair.source, "接");
        assert_eq!(pair.target, "借");
        assert_eq!(pair.before, "");
        assert_eq!(pair.after, "口设计文档");
    }

    #[test]
    fn emoji_boundaries_are_not_split() {
        let pair = minimal_edit("好的🍎结束", "好的🍊结束").unwrap();
        assert_eq!(pair.source, "🍎");
        assert_eq!(pair.target, "🍊");
    }

    #[test]
    fn context_is_captured_around_the_edit() {
        let pair = minimal_edit("前面的内容大禹后面的内容", "前面的内容大鱼后面的内容").unwrap();
        assert_eq!(pair.source, "禹");
        assert_eq!(pair.target, "鱼");
        assert_eq!(pair.before, "前面的内容大");
        assert_eq!(pair.after, "后面的内容");
    }

    #[test]
    fn an_edit_inside_the_inserted_text_is_attributed_to_us() {
        let edit = minimal_edit("上文我们用大禹养殖下文", "上文我们用大鱼养殖下文").unwrap();
        assert!(edit_is_within_typed_text(&edit, "我们用大禹养殖"));
    }

    #[test]
    fn an_edit_elsewhere_in_the_document_is_not_ours() {
        // 用户在同一个输入框里改自己之前写的东西 —— 观察器照样会收到通知，但这跟本次
        // 听写无关，学进来就是噪声。
        let edit = minimal_edit("用户旧内容甲\n我们插的话", "用户旧内容乙\n我们插的话").unwrap();
        assert_eq!(edit.source, "甲");
        assert!(!edit_is_within_typed_text(&edit, "我们插的话"));
    }

    #[test]
    fn context_is_capped_on_both_sides() {
        let long = "字".repeat(500);
        let before = format!("{long}甲{long}");
        let after = format!("{long}乙{long}");
        let pair = minimal_edit(&before, &after).unwrap();
        assert_eq!(pair.source, "甲");
        assert_eq!(pair.before.chars().count(), CONTEXT_CHARS);
        assert_eq!(pair.after.chars().count(), CONTEXT_CHARS);
    }
}
