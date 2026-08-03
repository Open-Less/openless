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

/// 一处改动该以什么方式进词汇表。
///
/// **只写词汇表，不再写纠正规则。** 学来的东西配不上「见字面就替换」这份权力：
///
/// - 纠正规则是字面替换，错了是静默的、全局的、用户看不见。真机上学到过
///   `小鱼 → x` 这种半截规则，它会毁掉以后每一个「小鱼」。
/// - 词汇表是提示：送给 ASR 提高听对的概率，也进润色 prompt 让 LLM **带着上下文**
///   判断该不该改。错了最多是没帮上忙。
///
/// 而且两者并存会直接打架：词汇表里有 `Codex` 热词（「我要这个词」），纠正规则却写着
/// `Codex → 扣的爱思`（「把这个词换掉」）—— 真机上就撞出过这个环。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleTier {
    /// 自动收进词汇表（打 `learned` 标记，用户能看到并删掉）。
    Auto,
    /// 弹卡片问一下，用户点了才收。
    Confirm,
}

/// 规则 pattern 的最小长度（char）。
///
/// 一个字的 pattern 会在往后每一句话里到处命中：从「大禹 → 大鱼」学出「禹 → 鱼」，
/// 下次说「禹州」就成了「鱼州」。
const MIN_PATTERN_CHARS: usize = 2;

/// 从一次手改里提炼出来的词条建议。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedRule {
    /// 用户改之前那个（错的）写法。不入库，只用来在卡片上给用户看清改的是什么。
    pub pattern: String,
    /// 用户最后要的那个词 —— 要进词汇表的就是它。
    pub replacement: String,
    pub tier: RuleTier,
}

/// 词汇表条目的长度上限（char）。超过就不是一个「词」了。
const MAX_PHRASE_CHARS: usize = 12;

/// 判定用户改出来的这个词该不该进词汇表、要不要先问一声。
///
/// **只看 `target`（用户最后要的那个词），不看 `source → target` 这个映射。** 语义变了：
/// 问的不再是「这个替换安不安全」，而是「这个**词**值不值得记住」。方向问题也随之消失
/// —— 你把中文改成英文还是反过来，都不影响「你最后要的是哪个词」。
///
/// 返回 `None` = 那不是一个词：
///
/// - **`target` 为空**（纯删除）—— 没有词可记。
/// - **跨行或跨句**（换行、中文句读标点、`?!;`）—— 真机上抓到的假阳性正是这类：在聊天
///   框里按回车发送，输入框清空换成占位符，形式上是「把一整句换成另一句」。
/// - **超过 [`MAX_PHRASE_CHARS`]** —— 一整句话不是词条。
pub fn classify_edit(edit: &EditPair) -> Option<RuleTier> {
    let target = edit.target.trim();
    if target.is_empty() || edit.source.trim().is_empty() {
        return None;
    }
    if crosses_a_sentence_boundary(&edit.source) || crosses_a_sentence_boundary(target) {
        return None;
    }
    if target.chars().count() > MAX_PHRASE_CHARS {
        return None;
    }
    // 拉丁字母/数字构成的词（Codex、Node.js、GPT-5）自动收：你把一个词改成英文写法，
    // 这件事本身就说明它是个专名 —— 没人为了换语气把中文改成英文。
    if is_pure_ascii_word(target) {
        return Some(RuleTier::Auto);
    }
    // 其余（主要是汉字词）问一声。「大鱼」可能是公司名也可能就是字面意思，「接口」是
    // 术语也是极常见的普通词 —— 光看字分不出来，而普通词进了热词表会让识别对它过度敏感。
    Some(RuleTier::Confirm)
}

/// 把一处改动变成一条可以入库的规则。
///
/// 关键的一步是**向外扩到安全长度**：中文同音词纠错的最小差异往往只有一个字（「大禹
/// → 大鱼」剥掉公共前缀后只剩「禹 → 鱼」），而单字规则会到处误伤。所以用 `before` /
/// `after` 里存着的上下文把两侧同步补长，补出来的正是用户心里想的那个词——「大禹 →
/// 大鱼」而不是「禹 → 鱼」。
///
/// 优先从左边补（词的前半部分更能定位它），左边不够再从右边补。补进来的字必须是实
/// 字：把换行或空格卷进 literal 规则，它就再也匹配不上任何东西了。上下文两侧都凑不
/// 够时返回 `None` —— 宁可不学。
///
/// 分级在扩长**之前**判定：扩长会把中文上下文粘到英文 pattern 上，之后再判跨文种就
/// 永远判不出来了。
pub fn learned_rule(edit: &EditPair) -> Option<LearnedRule> {
    let tier = classify_edit(edit)?;
    let (pattern, replacement) = pad_to_min_length(edit)?;
    Some(LearnedRule {
        pattern,
        replacement,
        tier,
    })
}

fn pad_to_min_length(edit: &EditPair) -> Option<(String, String)> {
    let before: Vec<char> = edit.before.chars().collect();
    let after: Vec<char> = edit.after.chars().collect();
    let base = edit.source.chars().count();
    let (mut left, mut right) = (0usize, 0usize);

    // 借一个字的条件：那一侧还有字，且那个字不是空白。
    let can_borrow = |chars: &[char], taken: usize, from_end: bool| {
        let idx = if from_end {
            chars.len().checked_sub(taken + 1)
        } else {
            (taken < chars.len()).then_some(taken)
        };
        idx.is_some_and(|i| !chars[i].is_whitespace())
    };

    while base + left + right < MIN_PATTERN_CHARS {
        if can_borrow(&before, left, true) {
            left += 1;
        } else if can_borrow(&after, right, false) {
            right += 1;
        } else {
            return None;
        }
    }

    let prefix: String = before[before.len() - left..].iter().collect();
    let suffix: String = after[..right].iter().collect();
    Some((
        format!("{prefix}{}{suffix}", edit.source),
        format!("{prefix}{}{suffix}", edit.target),
    ))
}

/// 这段文字里有没有句子边界（换行或句读标点）。
///
/// 只看中文标点和 ASCII 的 `?!;` —— **不看 ASCII 句点**，`Node.js`、`co.uk`、`v1.2`
/// 都带点，把它们当句子边界会误杀一整类技术名词，而那正是这个功能最该学会的东西。
fn crosses_a_sentence_boundary(s: &str) -> bool {
    s.chars()
        .any(|c| matches!(c, '\n' | '\r' | '。' | '？' | '！' | '；' | '，' | '、' | '：' | '?' | '!' | ';'))
}

/// 这是不是一个由拉丁字母/数字构成的词（`Codex`、`Node.js`、`GPT-5`、`v1.2`）。
///
/// 要求至少有一个字母 —— 纯数字（"2026"）不是值得记的词。连字符、下划线、点号放行，
/// 技术名词到处是它们。
fn is_pure_ascii_word(s: &str) -> bool {
    let mut saw_alpha = false;
    for ch in s.chars() {
        if ch.is_whitespace() || ch == '-' || ch == '_' || ch == '.' {
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            saw_alpha |= ch.is_ascii_alphabetic();
        } else {
            return false;
        }
    }
    saw_alpha
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

    /// 真机抓到的假阳性：在聊天框里按回车发送，输入框清空并显示占位符。
    ///
    /// 形式上这是一次「把整句话替换成另一句」的编辑，`MAX_EDIT_CHARS`（64）拦不住
    /// ——那句话才 25 个字。要是没这条，它会被建议成一条纠正规则，以后每次说那句话
    /// 都被替换成占位符。
    #[test]
    fn submitting_a_chat_box_never_becomes_a_rule() {
        let e = minimal_edit(
            "还有哪些是我们明明有，但 status 看板没有的模型呢？",
            "Type / for commands",
        )
        .expect("形式上确实是一处改动 —— 检测到它没问题");
        assert_eq!(
            classify_edit(&e),
            None,
            "整句被替换不该变成规则：以后每次说那句话都会被换成占位符"
        );
    }

    #[test]
    fn a_technical_name_with_a_dot_is_still_learned() {
        // 句子边界守卫不看 ASCII 句点：Node.js / co.uk / v1.2 全带点，把它们当句子
        // 边界会误杀一整类技术名词 —— 而那正是这个功能最该学会的东西。
        let e = EditPair {
            source: "诺德点 JS".to_string(),
            target: "Node.js".to_string(),
            before: "用".to_string(),
            after: "写".to_string(),
        };
        assert_eq!(classify_edit(&e), Some(RuleTier::Auto));
    }

    #[test]
    fn a_whole_sentence_is_not_a_word() {
        // 词汇表条目是「词」。一整句话进热词表毫无意义，还会把识别带偏。
        let e = EditPair {
            source: "短的".to_string(),
            target: "这是一句很长的话完全不像一个词".to_string(),
            before: String::new(),
            after: String::new(),
        };
        assert_eq!(classify_edit(&e), None);
    }

    #[test]
    fn a_sentence_ending_in_a_period_never_becomes_a_rule() {
        // 第二条真机假阳性：用户清空了输入框里已经写完的一句话。
        let e = EditPair {
            source: "界面和界面之间的问题倒不大。".to_string(),
            target: "改成别的".to_string(),
            before: String::new(),
            after: String::new(),
        };
        assert_eq!(classify_edit(&e), None);
    }

    #[test]
    fn a_multiline_change_never_becomes_a_rule() {
        // 词级字面替换装不下换行：要么永远匹配不上，要么一命中就改掉一整段。
        let edit = EditPair {
            source: "第一行\n第二行".to_string(),
            target: "改过的内容".to_string(),
            before: "上文".to_string(),
            after: "下文".to_string(),
        };
        assert_eq!(classify_edit(&edit), None);

        let edit = EditPair {
            source: "一个词".to_string(),
            target: "换成\n两行".to_string(),
            before: "上文".to_string(),
            after: "下文".to_string(),
        };
        assert_eq!(classify_edit(&edit), None);
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

    // ─────────────────────── 分级 ───────────────────────

    fn tier(before: &str, after: &str) -> Option<RuleTier> {
        classify_edit(&minimal_edit(before, after).expect("应当是一处有效改动"))
    }

    #[test]
    fn a_latin_word_is_collected_automatically() {
        // 你把一个词改成英文写法，这件事本身就说明它是专名 —— 没人为了换语气这么做。
        assert_eq!(
            tier("我们用扣德克斯写代码", "我们用Codex写代码"),
            Some(RuleTier::Auto)
        );
    }

    #[test]
    fn direction_no_longer_matters() {
        // 旧设计按「中文→英文」还是反过来分档，真机上撞出过一个环：词汇表里的 `Codex`
        // 热词让识别把中文听成英文，用户改回中文，系统又学一条规则把 `Codex` 换掉。
        //
        // 现在只看「你最后要的是哪个词」，方向不再参与判定。英文被改回中文时，要记的
        // 是那个中文词 —— 汉字词一律先问一声。
        assert_eq!(tier("打开setting页", "打开设置页"), Some(RuleTier::Confirm));
    }

    #[test]
    fn a_chinese_homophone_needs_confirmation() {
        // 「大禹 → 大鱼」和「明天 → 后天」在文本上长得一模一样，光看字分不出「纠错」
        // 和「改主意」。这正是不引入拼音之后必须问用户的那一类。
        assert_eq!(tier("今天讲大禹养殖", "今天讲大鱼养殖"), Some(RuleTier::Confirm));
        assert_eq!(tier("我们明天见面", "我们后天见面"), Some(RuleTier::Confirm));
    }

    // ─────────────────────── 扩到安全长度 ───────────────────────

    fn rule(before: &str, after: &str) -> Option<LearnedRule> {
        learned_rule(&minimal_edit(before, after).expect("应当是一处有效改动"))
    }

    #[test]
    fn a_single_char_diff_is_widened_using_the_left_context() {
        // 最小差异是「禹 → 鱼」。直接入库会让往后每个「禹」都变成「鱼」；向左扩一个字
        // 得到的「大禹 → 大鱼」才是用户心里想的那条规则。
        let learned = rule("今天讲大禹养殖", "今天讲大鱼养殖").unwrap();
        assert_eq!(learned.pattern, "大禹");
        assert_eq!(learned.replacement, "大鱼");
        assert_eq!(learned.tier, RuleTier::Confirm);
    }

    #[test]
    fn a_single_char_diff_at_the_start_is_widened_using_the_right_context() {
        // 左边没有上下文（改动就在开头），只能向右扩。
        let learned = rule("接口设计文档", "借口设计文档").unwrap();
        assert_eq!(learned.pattern, "接口");
        assert_eq!(learned.replacement, "借口");
    }

    #[test]
    fn an_already_long_enough_diff_is_not_widened() {
        let learned = rule("我们用扣德克斯写代码", "我们用Codex写代码").unwrap();
        assert_eq!(learned.pattern, "扣德克斯");
        assert_eq!(learned.replacement, "Codex");
        assert_eq!(learned.tier, RuleTier::Auto);
    }

    #[test]
    fn widening_never_swallows_whitespace() {
        // 把换行或空格卷进 literal 规则，它就再也匹配不上任何东西了。
        // 左边是换行 → 只能往右扩。
        let learned = rule("上一行\n甲乙", "上一行\n丙乙").unwrap();
        assert_eq!(learned.pattern, "甲乙");
        assert_eq!(learned.replacement, "丙乙");
    }

    #[test]
    fn an_edit_with_no_usable_context_is_not_learned() {
        // 两侧都没有实字可借 —— 宁可不学，也不要一条到处误伤的单字规则。
        assert!(rule("甲", "乙").is_none());
        assert!(rule(" 甲 ", " 乙 ").is_none());
    }

    #[test]
    fn tier_is_decided_before_widening() {
        // 扩长会把中文上下文粘到英文词上；先扩再判就永远判不出这是个拉丁词了。
        let learned = rule("装了容器之后", "装了docker之后").unwrap();
        assert_eq!(learned.tier, RuleTier::Auto);
    }

    #[test]
    fn a_semantic_rewrite_needs_confirmation() {
        assert_eq!(
            tier("这个方案挺好的", "这个方案还行吧"),
            Some(RuleTier::Confirm)
        );
    }

    #[test]
    fn a_pure_deletion_never_becomes_a_rule() {
        // 做成全局替换就是「以后所有听写里这个词一律删掉」。风险与收益完全不对等。
        assert_eq!(tier("这个的的接口", "这个的接口"), None);
        assert_eq!(tier("多余的词组在这", "在这"), None);
    }

    #[test]
    fn swapping_one_latin_name_for_another_is_still_a_word_worth_keeping() {
        // 「Codex → Cursor」大概率是换工具而不是纠错，但要记的是 `Cursor` 这个词
        // 本身 —— 它值得进词汇表，跟这次改动的动机无关。词条只是提示，不做替换。
        assert_eq!(
            tier("我们用 Codex 写", "我们用 Cursor 写"),
            Some(RuleTier::Auto)
        );
    }

    #[test]
    fn a_side_that_mixes_scripts_is_not_treated_as_a_loanword_mishearing() {
        // 「一侧纯 CJK、另一侧纯 ASCII」要求的是「纯」。混着的那种更可能是用户在换
        // 说法，不是我们把外来词听成了汉字。
        let edit = EditPair {
            source: "Docker容器".to_string(),
            target: "容器引擎".to_string(),
            before: "用".to_string(),
            after: "跑".to_string(),
        };
        assert_eq!(classify_edit(&edit), Some(RuleTier::Confirm));
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
