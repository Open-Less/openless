# 光标上下文 + 手改学习 —— 装机测试案例

装的版本：`local/daily`。开关在 **设置 → 隐私 → 数据存储 → 光标上下文（实验）**，默认关。

盯日志：

```bash
tail -f ~/Library/Logs/OpenLess/openless.log | grep -E "cursor-context|cursor context|vocab"
```

重装后辅助功能授权会短暂失效（ad-hoc 签名每次构建 cdhash 都变），但 app 自己会恢复——实测重试到第 23 次时自己起来，约 70 秒。看到 `hotkey listener installed` 就能用了；万一一直刷 `CGEventTapCreate 失败`，去 系统设置 → 隐私与安全性 → 辅助功能 → OpenLess 关掉再打开。

---

## 这个功能干什么

开关打开后，每次听写会读**你当时正在写的那个文档里、光标附近的几百个字**，跟着请求一起发给 LLM。这样它知道你在写什么，「接口」不会写成「借口」。

落字之后它还会盯一小会儿：如果你手动改了它插进去的某个词，那个词可能进你的词汇表。

**只写词汇表，不写纠正规则。** 词条是提示（送给 ASR、进润色 prompt 让 LLM 带上下文判断），错了最多是没帮上忙；纠正规则是字面替换，错了是静默的、全局的。学来的东西配不上后者那份权力。

---

## 1. AX 覆盖率 —— 不用开口说话

**设置 → 高级 → 调试工具 → 光标上下文探针**

点「探测（5 秒后）」→ 切到目标 app → 在正文里点一下让光标进去 → 切回来看结果。

```
ok · 11ms
备忘录 (com.apple.Notes)
我们这个模块的接口设计得不太好，⟦光标⟧
```

`⟦光标⟧` 是光标位置，左边上文右边下文。

| app | 预期 |
|---|---|
| 备忘录 / 文本编辑 | `ok` |
| VS Code / Notion / Claude 桌面版（Electron） | 能读到，但光标位置常常不准 |
| 微信 / 飞书 | `ok` |
| 浏览器普通输入框 | `ok` |
| 浏览器**密码框** | **必须 `blocked` / `secure_text_field`** |
| 终端 / iTerm / Warp | **必须 `blocked` / `blocked_app`** |
| 1Password | **必须 `blocked` / `blocked_app`** |

后三行是安全验收，任何一条没拦住立刻停下来说。

**屏幕上显示出来的原文，就是会发给 LLM 的内容。** 哪个 app 里蹦出了你不希望离开这台机器的东西，那是必须知道的发现。

---

## 2. 开关关闭时行为不变

关掉开关 → 听写几句 → 日志里**不该有任何 `cursor-context` 行**。

关着的时候一次 AX 都不发，prompt 也和这个功能不存在时逐字节相同（有单测钉死）。

---

## 3. 上下文真的进 prompt 了吗

同一个 app、同一个位置，开/关各听一次同样的话：

```bash
grep "effective_prompt_chars" ~/Library/Logs/OpenLess/openless.log | tail -2
```

开着的那次应该多 **427 + 读到的字数**（427 = 上下文块固定措辞 314 + 注入防御条款 113）。

必须同一个 app 比——prompt 里带了前台应用名，「备忘录 (com.apple.Notes)」和「Claude (com.anthropic.claudefordesktop)」差 19 个字符，换 app 比会对不上账。

**更该盯的是 LLM 拿它干了什么。** 最容易翻车的是把上文复述进输出：光标前写着「这个模块的接口设计得不太好，」，你口述「还得再改」，它输出「这个模块的接口设计得不太好，还得再改」——把你已有的字又插了一遍。prompt 里明令禁止了，但那是软约束。

---

## 4. 手改 → 词条

前提：开关开着，**在备忘录这类原生 app 里测**（Electron 的通知不稳）。

### 4a. 英文词 —— 自动收，不问你

1. 口述一句带中文音译词的话（「这个功能我打算用扣德克斯来改」）
2. 把那个音译词改成 `Codex`
3. **把光标点到别处**（这是判定「你改完了」的信号）

```
user edit detected: source="扣德克斯" target="Codex"
learned vocabulary entry: "Codex" (was "扣德克斯")
```

去 **词汇表** 页，分割线下面的「自动收集」区应该多出 `Codex`。

### 4b. 中文词 —— 弹卡片问你

1. 口述一句，把某个词改成另一个**中文**词
2. 光标点到别处
3. 屏幕底部（胶囊那个位置）弹出卡片：

```
要记住这个词吗？              都不用
大禹 → 大鱼                    [好]
```

- 点「好」→ 进词汇表分割线下面
- 点「都不用」/ 等 10 秒 → 消失，什么都不记（没有拒绝名单，你下次再改同一个词它还会问）
- 连着改两个词 → 合并到同一张卡片，倒计时重置

卡片只挡住它自己那一块的鼠标（窗口会缩到卡片大小），周围照常能点。

### 4c. 该拒绝的时候确实拒绝了

| 操作 | 期待 |
|---|---|
| 改完切到别的 app 再改 | 无输出（观察器已解除） |
| 落字后等 60 秒再改 | 无输出（硬上限） |
| 落字后再听写一次，回头改第一段 | 无输出（新会话解除旧观察器） |
| 改你自己之前写的内容（不是它插的） | 无输出（只认落在插入文本里的改动） |
| 只是补几个字（纯插入） | 不学 |
| 把一个词删掉（纯删除） | 检测到但不入库 |
| 在聊天框里按回车发送 | 不该产生任何建议 ← 这条曾经翻过车 |
| 慢慢逐字打出一个英文词 | 只学最终那个词，**不该出现 `→ C`、`→ Co` 这种半截的** |

最后两条是真机上抓到过的 bug，值得专门试。

---

## 5. 词汇表页

```
词汇表
  [你自己加的...]
  ───────── 自动收集（N）  [全部删除]
  [自动收的...]
```

试一下「全部删除」，确认**手动加的不会被一起删掉**。

早期版本往**纠正规则**里写过 learned 条目（现在不写了）。如果你的 `correction-rules.json` 里还有，纠正规则区有个「只看自动收集的」筛选可以把它们挑出来删。

---

## 6. 不会冻住界面

对着一个卡死的 app 触发听写。AX 调用 200ms 超时、整次读取 1.2 秒封顶、跑在独立线程，不占 tokio worker。

---

## 已知限制

1. **建议只在内存里**，OpenLess 重启就没了。卡片消失即当没发生——下次改同一个词会再问。
2. **Electron 类 app 的光标位置常常不准**，日志里表现为 `before=0 after=N`（上文读成空）。上下文对润色的价值主要在上文，那种情况下收益有限。
3. **风格包预览里看不到 `<cursor_context>`**，跟 `front_app` 一样是运行时才有值的东西。

---

## 出问题时给我这些

```bash
# 相关日志（诊断细节是 debug 级别，日常不记；要更细的得改 LevelFilter 重编译）
grep -E "cursor-context|cursor context|vocab" ~/Library/Logs/OpenLess/openless.log | tail -100

# 学到的词条
python3 -c "import json,os;[print(' ',e['phrase']) for e in json.load(open(os.path.expanduser('~/Library/Application Support/OpenLess/dictionary.json'))) if e.get('note')=='从手改中自动收集']"

# 开关状态
python3 -c "import json,os;print(json.load(open(os.path.expanduser('~/Library/Application Support/OpenLess/preferences.json'))).get('cursorContextEnabled'))"
```

`edit watch disarmed` 那一行带两个数字——收到几次通知、学到几处改动。这两个数字足够判断某个 app 到底发不发通知，也就是逐 app 的覆盖率数据。
