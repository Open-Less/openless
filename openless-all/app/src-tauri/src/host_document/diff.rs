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
    // 比对前先去掉两侧的尾部空白。**这一步不是洁癖，是算法正确性的前提。**
    //
    // 公共后缀是从末尾往前逐字符比的，末尾只要差一个字符，后缀长度立刻判为 0，
    // 于是「改动点到结尾」的整段都成了差异。真机上就这么翻过车：用户只把「压根」
    // 改成「根本」，改完顺手按了回车 —— 基线末尾是「醒」、当前末尾是「\n」，第一个
    // 字符就不匹配，两个字的改动被撑成九个字的整句，卡片上弹出「压根就没有给我提醒
    // → 根本就没有给我提醒」。用户的原话是「我只改了一个词，这么长怎么要」。
    //
    // 尾部空白的差异本身没有词汇价值（多半就是一次回车），去掉它既修好了后缀剥离，
    // 也顺带让「只按了个回车」这种情况在下一行的相等判定里直接出局。
    //
    // **残留的一面**：这个算法只能表达**一处连续**的差异（前缀 + 后缀两刀剥出中间）。
    // 用户同时做两处改动时，两处之间的所有字都会被并进同一个 span。trim_end 只治好了
    // 「第二处是尾部空白」这一种 —— 也是最常见的一种。换成尾部标点（改完词又补了个
    // 句号）仍然会撑开。真要根治得换成 LCS 之类能识别多处改动的算法，那是另一件事；
    // 在那之前，卡片上偶尔出现的超长 pattern 就是这个来源。
    let before_text = before_text.trim_end();
    let after_text = after_text.trim_end();

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

/// 规则 pattern 的最小长度（char）。
///
/// 一个字的 pattern 会在往后每一句话里到处命中：从「大禹 → 大鱼」学出「禹 → 鱼」，
/// 下次说「禹州」就成了「鱼州」。
const MIN_PATTERN_CHARS: usize = 2;

/// 从一次手改里提炼出来的词条建议。
///
/// **一律是建议，没有「自动收」这一档。** 早期版本认为「你把一个词改成英文写法」本身
/// 就足以证明它是专名，于是跨文种的改动静默入库。真机跑了两天，自动收进去 5 条里只有
/// 1 条是对的（`Tailscale` ✓，而 `ype`、`ess` 是逐字打字的半截，`typeless` 是用户本
/// 来就要打的词，` claude` 带着前导空格）—— 因为观察器看到的是**编辑过程中的每一个
/// 中间态**，而中间态在文本上跟「一次纠错」长得完全一样。
///
/// 分不出来就别猜。卡片上一个勾一个叉，是这里唯一可靠的判据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedRule {
    /// 用户改之前那个（错的）写法。不入库，只用来在卡片上给用户看清改的是什么。
    pub pattern: String,
    /// 用户最后要的那个词 —— 要进词汇表的就是它。
    pub replacement: String,
}

/// 词汇表条目的长度上限（char）。超过就不是一个「词」了。
const MAX_PHRASE_CHARS: usize = 12;

/// 这处改动值不值得拿去问用户「要记住这个词吗」。
///
/// **只看 `target`（用户最后要的那个词），不看 `source → target` 这个映射。** 问的不是
/// 「这个替换安不安全」，而是「这个**词**值不值得记住」。方向也就不重要了 —— 你把中文
/// 改成英文还是反过来，都不影响「你最后要的是哪个词」。
///
/// 这里只做**廉价的粗筛**，把连问都不值得问的滤掉；真正的判断交给卡片上的勾叉。
/// 返回 `false` = 那根本不是一个词：
///
/// - **`target` 为空**（纯删除）—— 没有词可记。
/// - **跨行或跨句**（换行、中文句读标点、`?!;`）—— 真机上抓到的假阳性正是这类：在聊天
///   框里按回车发送，输入框清空换成占位符，形式上是「把一整句换成另一句」。
/// - **任一侧超过 [`MAX_PHRASE_CHARS`]** —— 一整句话不是词条。
///
/// **两侧都要量。** 只量 `target` 的话，「把一长串不带标点的话改成 `ok`」能过关：
/// `minimal_edit` 那道 64 char 的闸门放它过去，句读检查也拦不住不带标点的长句。
/// 那是一次改写，不是一次纠错 —— 拿去问用户「要记住 ok 这个词吗」纯属噪声，
/// 卡片上那条 `pattern` 还会长到显示不下。一个词被听错，错的写法不会比它长太多。
pub fn is_vocab_worthy(edit: &EditPair) -> bool {
    let target = edit.target.trim();
    let source = edit.source.trim();
    if target.is_empty() || source.is_empty() {
        return false;
    }
    if crosses_a_sentence_boundary(source) || crosses_a_sentence_boundary(target) {
        return false;
    }
    target.chars().count() <= MAX_PHRASE_CHARS && source.chars().count() <= MAX_PHRASE_CHARS
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
/// 最后那一步 `trim` 不能省：最小差异是按 char 剥前后缀剥出来的，边界上很容易挂着一
/// 个空格。真机上就学到过 ` claude`（带前导空格），那种词条永远匹配不上任何东西。
pub fn learned_rule(edit: &EditPair) -> Option<LearnedRule> {
    if !is_vocab_worthy(edit) {
        return None;
    }
    let (pattern, replacement) = pad_to_min_length(edit)?;
    let pattern = pattern.trim().to_string();
    let replacement = replacement.trim().to_string();
    if pattern.is_empty() || replacement.is_empty() {
        return None;
    }
    Some(LearnedRule {
        pattern,
        replacement,
    })
}

fn pad_to_min_length(edit: &EditPair) -> Option<(String, String)> {
    let before: Vec<char> = edit.before.chars().collect();
    let after: Vec<char> = edit.after.chars().collect();
    // 按 **trim 之后**的长度算，因为最终入库的也是 trim 之后的。
    //
    // 用原始长度会漏掉一整类：「大 禹」→「大鱼」的最小差异是 `" 禹"` → `"鱼"`，
    // 带空格数出来是 2 char，正好够 MIN_PATTERN_CHARS，于是不扩长；trim 之后却只剩
    // 单字的「禹 → 鱼」—— 恰好是这个常量存在的意义所要挡的那种。
    let base = edit.source.trim().chars().count();
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

/// 这处改动是不是落在「我们刚插进去的那段文字」里。
///
/// 观察器盯的是整个控件，用户在文档别处改自己的旧内容照样会触发通知。那种改动跟本次
/// 听写毫无关系，学进来纯属噪声 —— 而噪声进了词库就会去改用户以后所有的听写。
///
/// 抽成纯函数是为了能脱离 AXObserver 测：这条判据是「只学我们自己的错」与「见什么学
/// 什么」之间唯一的分界线。
///
/// ## 已知限制：按内容匹配，不按位置
///
/// 判的是「这几个字在我们插入的文本里出现过」，不是「这处改动发生在我们插入的那一段
/// 里」。同一个词在文档别处也有时，用户改那一处会被误算到我们头上 —— 比如我们插了
/// 「好的，我明白了」，用户回头把上一段的另一个「好的」改成「好滴」。
///
/// 没有收紧成位置判定，是权衡的结果：
///
/// - **代价是可见且可撤销的。** 现在每条建议都要用户在卡片上点勾才入库，误算最多是多
///   一次询问，点叉即消。
/// - **收紧的代价是不可见的。** 位置判定要在锚定时记下插入偏移，再和改动位置比对。可
///   目标 app 会加工插入的文本（智能引号、自动补全、字形转换）—— 那正是 `anchored` 那
///   套兜底存在的原因。偏移对不上时会**静默地不学**，而用户看不见自己少学了什么。
/// - 用错方向换掉对方向：宁可多问一次，不可悄悄漏学。
///
/// 真机上这种误算到底多常见，是装机自用才能回答的问题。真出现了再按数据收紧。
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
        assert!(!is_vocab_worthy(&e), "整句被替换不该变成规则：以后每次说那句话都会被换成占位符");
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
        assert!(is_vocab_worthy(&e));
    }

    /// 长度上限两侧都要量，不能只量 target。
    ///
    /// 「一长串不带标点的话 → ok」：`minimal_edit` 的 64 char 闸门放它过去（没超），
    /// 句读检查也拦不住（没标点）。只量 target 的话它就成了一条建议 ——「要记住 ok
    /// 这个词吗」，而卡片上那条 pattern 长到显示不下。那是改写，不是纠错。
    /// 真机翻车：用户只改了两个字，改完按了回车，建议却变成整句。
    ///
    /// 公共后缀从末尾往前比，末尾差一个字符（`醒` vs `\n`）后缀就判为 0，于是「改动点
    /// 到结尾」整段都成了差异。用户看到卡片上弹出九个字的短语，原话是「我只改了一个词，
    /// 这么长怎么要」。
    #[test]
    fn a_trailing_newline_must_not_swallow_the_whole_tail() {
        let e = minimal_edit("我压根就没有给我提醒", "我根本就没有给我提醒\n")
            .expect("是一处有效改动");
        assert_eq!(e.source, "压根", "只该抠出真正改掉的那两个字");
        assert_eq!(e.target, "根本");
    }

    /// 只按了个回车不算改动。
    #[test]
    fn pressing_enter_alone_is_not_an_edit() {
        assert!(minimal_edit("写完了", "写完了\n").is_none());
        assert!(minimal_edit("写完了", "写完了  \n\n").is_none());
    }

    #[test]
    fn a_long_source_is_a_rewrite_not_a_correction() {
        let e = EditPair {
            source: "这一长串话完全没有任何标点符号所以句读检查拦不住它".to_string(),
            target: "ok".to_string(),
            before: String::new(),
            after: String::new(),
        };
        assert!(e.source.chars().count() <= 64, "前提：没被 minimal_edit 拦掉");
        assert!(!is_vocab_worthy(&e));
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
        assert!(!is_vocab_worthy(&e));
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
        assert!(!is_vocab_worthy(&e));
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
        assert!(!is_vocab_worthy(&edit));

        let edit = EditPair {
            source: "一个词".to_string(),
            target: "换成\n两行".to_string(),
            before: "上文".to_string(),
            after: "下文".to_string(),
        };
        assert!(!is_vocab_worthy(&edit));
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

    // ─────────────────────── 粗筛 ───────────────────────

    fn worthy(before: &str, after: &str) -> bool {
        is_vocab_worthy(&minimal_edit(before, after).expect("应当是一处有效改动"))
    }

    #[test]
    fn a_latin_word_is_worth_asking_about() {
        assert!(worthy("我们用扣德克斯写代码", "我们用Codex写代码"));
    }

    #[test]
    fn direction_does_not_matter() {
        // 旧设计按「中文→英文」还是反过来分档，真机上撞出过一个环：词汇表里的 `Codex`
        // 热词让识别把中文听成英文，用户改回中文，系统又学一条规则把 `Codex` 换掉。
        //
        // 现在只看「你最后要的是哪个词」，方向不参与判定。
        assert!(worthy("打开setting页", "打开设置页"));
    }

    #[test]
    fn a_chinese_homophone_is_worth_asking_about() {
        // 「大禹 → 大鱼」和「明天 → 后天」在文本上长得一模一样，光看字分不出「纠错」
        // 和「改主意」。分不出就问 —— 这正是不引入拼音之后卡片存在的理由。
        assert!(worthy("今天讲大禹养殖", "今天讲大鱼养殖"));
        assert!(worthy("我们明天见面", "我们后天见面"));
    }

    /// 真机日志里自动收进词汇表的 5 条，有 4 条是这种「打字打到一半」的中间态：
    /// 用户在逐字敲 `Type`，观察器在 `ap` 变成 `ype` 的那一帧收到通知。
    ///
    /// 这一类**在文本上跟一次真正的纠错完全没有区别**，粗筛拦不住也不该硬拦。这个用例
    /// 钉的是：它们照旧会被提成建议，但建议只能通过卡片入库 —— 见 `LearnedRule` 的
    /// 文档，以及 `dictation::handle_user_edit` 里没有第二条分支这件事。
    #[test]
    fn a_half_typed_word_is_still_only_a_suggestion() {
        let learned = rule("按 ap 键", "按 ype 键").unwrap();
        assert_eq!(learned.replacement, "ype");
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
    }

    #[test]
    fn widening_never_swallows_whitespace() {
        // 把换行或空格卷进 literal 规则，它就再也匹配不上任何东西了。
        // 左边是换行 → 只能往右扩。
        let learned = rule("上一行\n甲乙", "上一行\n丙乙").unwrap();
        assert_eq!(learned.pattern, "甲乙");
        assert_eq!(learned.replacement, "丙乙");
    }

    /// 差异里夹着空格时，扩长必须按 trim 后的长度判，否则单字规则会溜过去。
    ///
    /// 「大 禹」→「大鱼」的最小差异是 `" 禹"` → `"鱼"`。带着空格数是 2 char，正好够
    /// MIN_PATTERN_CHARS 于是不扩长；可最终入库的是 trim 之后的，只剩单字「禹 → 鱼」
    /// —— 正是 MIN_PATTERN_CHARS 存在的意义所要挡的那种（下次说「禹州」就成了「鱼州」）。
    #[test]
    fn a_diff_padded_with_whitespace_still_gets_widened() {
        let learned = rule("今天讲大 禹养殖", "今天讲大鱼养殖").unwrap();
        assert_eq!(
            learned.replacement, "大鱼",
            "trim 之后必须仍然是个词，不能退化成单字"
        );
        assert!(
            learned.pattern.trim().chars().count() >= 2,
            "pattern 也不该是单字，实际是 {:?}",
            learned.pattern
        );
    }

    #[test]
    fn an_edit_with_no_usable_context_is_not_learned() {
        // 两侧都没有实字可借 —— 宁可不学，也不要一条到处误伤的单字规则。
        assert!(rule("甲", "乙").is_none());
        assert!(rule(" 甲 ", " 乙 ").is_none());
    }

    /// 真机上学到过 ` claude`（带前导空格）。词条前面挂个空格，它永远匹配不上任何东西
    /// —— 白白占一条，还让用户在词汇表里看见一个「怎么看都没错但就是不生效」的词。
    #[test]
    fn a_stray_space_on_the_boundary_is_trimmed_off() {
        let edit = EditPair {
            source: "cloud".to_string(),
            target: " claude".to_string(),
            before: "用".to_string(),
            after: "写".to_string(),
        };
        let learned = learned_rule(&edit).unwrap();
        assert_eq!(learned.replacement, "claude");
        assert_eq!(learned.pattern, "cloud");
    }

    #[test]
    fn a_semantic_rewrite_is_still_worth_asking_about() {
        assert!(worthy("这个方案挺好的", "这个方案还行吧"));
    }

    #[test]
    fn a_pure_deletion_never_becomes_a_rule() {
        // 没有词可记 —— 「以后所有听写里这个词一律删掉」不该是一次手改能表达的意思。
        assert!(!worthy("这个的的接口", "这个的接口"));
        assert!(!worthy("多余的词组在这", "在这"));
    }

    #[test]
    fn swapping_one_latin_name_for_another_is_still_a_word_worth_keeping() {
        // 「Codex → Cursor」大概率是换工具而不是纠错，但要记的是 `Cursor` 这个词
        // 本身 —— 它值得问一声，跟这次改动的动机无关。词条只是提示，不做替换。
        assert!(worthy("我们用 Codex 写", "我们用 Cursor 写"));
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
