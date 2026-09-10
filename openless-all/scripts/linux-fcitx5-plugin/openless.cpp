/*
 * SPDX-FileCopyrightText: 2025 OpenLess Contributors
 *
 * SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fcitx5 插件 — 供 OpenLess 听写文字提交 + 快捷键监听。
 *
 * DBus 接口: org.fcitx.Fcitx.OpenLess1  (对象路径 /openless)
 *  方法:
 *    CommitText(s: text) -> b      — 将文字提交到当前焦点输入上下文
 *                                    安全性：本接口在会话总线(session bus)上对同用户
 *                                    所有进程开放，此为 fcitx5/IBus 体系的标准安全模型
 *                                    （非特权进程隔离）。
 *    SetHotkey(as: keys)           — 设置听写触发快捷键 (Key::parse 格式)
 *    SetHotkeyRaw(uu: sym, states) — 直接设听写触发 sym+states (不走 parse)
 *    SetCustomDictationTrigger(s: keyString) — 设置自定义组合键 (Key::parse 格式)
 *    SetQaHotkeyRaw(uu: sym, states)     — 直接设 QA 面板触发 sym+states
 *    SetTranslationHotkeyRaw(uu: sym, states) — 直接设翻译模式触发 sym+states
 *    SetLessComputerHotkeyRaw(uu: sym, states) — 直接设 Less Computer 触发 sym+states
 *    SetAuxDown(s: text)                 — 在候选词列表下方显示状态文本
 *    ClearAuxDown()                      — 清除候选词列表下方文本
 *    GetSelectionText() -> s             — 读取当前 PRIMARY 选区文本（由 clipboard addon 维护）
 *    CaptureSelectionTarget(s: ticket) -> s — 捕获选区和原输入上下文
 *    ApplySelectionTarget(sss: ticket, source, replacement) -> b — 校验后替换
 *    RevertSelectionTarget(s: ticket) -> b — 校验光标前文本后撤销替换
 *    RekeySelectionTarget(ss: oldTicket, newTicket) -> b — 把 QA 目标交给 Core 预览
 *    CancelSelectionTarget(s: ticket) -> b — 释放未使用的目标
 *  信号:
 *    DictationKeyEvent(uub: sym, states, isPress) — 听写热键按下/抬起
 *    LessComputerKeyEvent(uub: sym, states, isPress) — Less Computer 热键按下/抬起
 *    LessComputerKeyCombined(uub: sym, states, isPress) — Less Computer 组合键撤销
 *    QaShortcutEvent(uub: sym, states, isPress)   — QA 快捷键按下/抬起
 *    SelectionPolishEvent(uub: sym, states, isPress) — 选区润色快捷键按下/抬起
 *    TranslationModifierEvent(uub: sym, states, isPress) — 翻译修饰键按下/抬起
 */

#include <memory>
#include <string>
#include <unordered_map>
#include <vector>

#include <fcitx-config/configuration.h>
#include <fcitx-config/iniparser.h>
#include <fcitx-config/option.h>
#include <fcitx-utils/dbus/bus.h>
#include <fcitx-utils/dbus/objectvtable.h>
#include <fcitx-utils/handlertable.h>
#include <fcitx-utils/i18n.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/log.h>
#include <fcitx-utils/utf8.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/event.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputcontextmanager.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>
#include <fcitx-module/clipboard/clipboard_public.h>
#include <fcitx-module/dbus/dbus_public.h>

FCITX_DEFINE_LOG_CATEGORY(openless, "openless");

namespace fcitx {

FCITX_CONFIGURATION(OpenLessConfig,
    KeyListOption triggerKey{this,
        "TriggerKey",
        _("Dictation trigger key"),
        {},
        KeyListConstrain()};
);

class OpenLess final : public AddonInstance,
                       public dbus::ObjectVTable<OpenLess> {
public:
    OpenLess(Instance *instance)
        : instance_(instance),
          triggerRawSym_(0),
          triggerRawStates_(0),
          qaRawSym_(0),
          qaRawStates_(0),
          selectionPolishRawSym_(0),
          selectionPolishRawStates_(0),
          translationRawSym_(0),
          translationRawStates_(0),
          lessComputerRawSym_(0),
          lessComputerRawStates_(0),
          hasCustomDictationKey_(false),
          dictationTriggerHeld_(false),
          dictationTriggerCombined_(false),
          lessComputerTriggerHeld_(false),
          lessComputerTriggerCombined_(false),
          savedIc_(nullptr),
          selectionIc_(nullptr) {

        // 1. 读取配置
        reloadConfig();

        // 2. 注册 DBus 接口
        auto *dbusMod = instance_->addonManager().addon("dbus", true);
        if (dbusMod) {
            auto *bus = dbusMod->call<IDBusModule::bus>();
            if (bus) {
                bus->addObjectVTable(
                    "/openless",
                    "org.fcitx.Fcitx.OpenLess1",
                    *this);
                FCITX_LOGC(openless, Info)
                    << "DBus interface registered at /openless";
            } else {
                FCITX_LOGC(openless, Warn)
                    << "Failed to get DBus bus";
            }
        } else {
            FCITX_LOGC(openless, Warn)
                << "DBus module not available";
        }

        // 3. 快捷键事件监听。
        // PreInputMethod 在引擎 InputMethod 阶段之前运行，
        // filterAndAccept() 设 filtered+accepted → 引擎跳过 commit → 字符不泄漏。
        eventHandlers_.push_back(
            instance_->watchEvent(
                EventType::InputContextKeyEvent,
                EventWatcherPhase::PreInputMethod,
                [this](Event &event) {
                    auto &keyEvent = static_cast<KeyEvent &>(event);
                    if (!keyEvent.isRelease()) {
                        savedIc_ = keyEvent.inputContext();
                    }
                    auto sym = static_cast<uint32_t>(keyEvent.key().sym());
                    auto states = static_cast<uint32_t>(keyEvent.key().states());
                    bool isPress = !keyEvent.isRelease();

                    if (lessComputerRawSym_ != 0 && sym == lessComputerRawSym_ &&
                        states == lessComputerRawStates_) {
                        lessComputerTriggerHeld_ = isPress;
                        if (isPress) {
                            lessComputerTriggerCombined_ = false;
                        }
                        lessComputerKeyEvent(sym, states, isPress);
                        keyEvent.filterAndAccept();
                        return;
                    }
                    if (isPress && lessComputerTriggerHeld_ && !isModifierKeySym(sym) &&
                        !lessComputerTriggerCombined_) {
                        lessComputerTriggerCombined_ = true;
                        lessComputerKeyCombined(sym, states, true);
                    }

                    // 自定义组合键：Alt 状态下字母 sym 可能大写（A vs a），归一化比较
                    if (hasCustomDictationKey_ && states == static_cast<uint32_t>(customDictationKey_.states()) &&
                        (sym == static_cast<uint32_t>(customDictationKey_.sym()) ||
                         (sym >= 65 && sym <= 90 && sym + 32 == static_cast<uint32_t>(customDictationKey_.sym())) ||
                         (sym >= 97 && sym <= 122 && sym - 32 == static_cast<uint32_t>(customDictationKey_.sym())))) {
                        FCITX_LOGC(openless, Debug)
                            << "Custom dictation: sym=" << sym << " states=" << states;
                        dictationKeyEvent(
                            static_cast<uint32_t>(customDictationKey_.sym()),
                            static_cast<uint32_t>(customDictationKey_.states()),
                            isPress);
                        keyEvent.filterAndAccept();
                        return;
                    }
                    if ((triggerRawSym_ != 0 &&
                         keyEvent.key().check(Key(static_cast<KeySym>(triggerRawSym_),
                                                   static_cast<KeyStates>(triggerRawStates_)))) ||
                        (triggerRawSym_ == 0 && [&]() {
                            for (const auto &hk : triggerKeyList_) {
                                if (sym == static_cast<uint32_t>(hk.sym()) &&
                                    states == static_cast<uint32_t>(hk.states()))
                                    return true;
                            }
                            return false;
                        }())) {
                        // 修复崩溃: raw 路径(SetHotkeyRaw)匹配时若 triggerRawStates_==0,
                        // 原代码会无条件取 triggerKeyList_[0];而 raw 模式常伴随空 KeyList
                        // (见 openless.conf: TriggerKey= 为空), 对空 vector 取下标 [0]
                        // 是未定义行为, 直接导致 fcitx5 段错误 (Key::states 读野指针)。
                        // 修正: 只有 KeyList 路径匹配时才访问列表, raw 路径直接用 raw 值。
                        uint32_t dsym = triggerRawSym_;
                        uint32_t dstates = triggerRawStates_;
                        if (triggerRawSym_ == 0 && !triggerKeyList_.empty()) {
                            dsym = static_cast<uint32_t>(triggerKeyList_[0].sym());
                            dstates = static_cast<uint32_t>(triggerKeyList_[0].states());
                        }
                        if (dsym == 0) {
                            return;
                        }
                        if (!hasCustomDictationKey_ && isModifierKeySym(dsym)) {
                            dictationTriggerHeld_ = isPress;
                            if (isPress) {
                                dictationTriggerCombined_ = false;
                            }
                        }
                        FCITX_LOGC(openless, Debug)
                            << "Dictation hotkey sym=" << dsym;
                        dictationKeyEvent(dsym, dstates, isPress);
                        keyEvent.filterAndAccept();
                        return;
                    }
                    if (isPress && dictationTriggerHeld_ && !isModifierKeySym(sym) &&
                        !dictationTriggerCombined_) {
                        FCITX_LOGC(openless, Debug)
                            << "Dictation hotkey combined with sym=" << sym;
                        dictationTriggerCombined_ = true;
                        dictationKeyCombined(sym, states, true);
                    }
                    if (qaRawSym_ != 0 && sym == qaRawSym_ &&
                        states == qaRawStates_) {
                        if (isPress) selectionIc_ = keyEvent.inputContext();
                        FCITX_LOGC(openless, Debug)
                            << "QA shortcut";
                        qaShortcutEvent(qaRawSym_, qaRawStates_, isPress);
                        keyEvent.filterAndAccept();
                        return;
                    }
                    if (selectionPolishRawSym_ != 0 &&
                        sym == selectionPolishRawSym_ &&
                        states == selectionPolishRawStates_) {
                        if (isPress) selectionIc_ = keyEvent.inputContext();
                        FCITX_LOGC(openless, Debug)
                            << "Selection polish shortcut";
                        selectionPolishEvent(selectionPolishRawSym_,
                                             selectionPolishRawStates_, isPress);
                        keyEvent.filterAndAccept();
                        return;
                    }
                    bool translationMatched = false;
                    if (translationRawSym_ != 0 && sym == translationRawSym_ &&
                        states == translationRawStates_)
                        translationMatched = true;
                    if (translationRawSym_ != 0 &&
                        (sym == 0xffe1 || sym == 0xffe2))
                        translationMatched = true;
                    if (translationMatched) {
                        FCITX_LOGC(openless, Debug)
                            << "Translation modifier: sym=" << sym;
                        translationModifierEvent(sym, states, isPress);
                    }
                }));

        // 4. 监听 InputContext 销毁事件，自动清空 savedIc_ 避免野指针
        eventHandlers_.push_back(
            instance_->watchEvent(
                EventType::InputContextDestroyed,
                EventWatcherPhase::Default,
                [this](Event &event) {
                    auto &icEvent = static_cast<InputContextEvent &>(event);
                    if (icEvent.inputContext() == savedIc_) {
                        savedIc_ = nullptr;
                    }
                    if (icEvent.inputContext() == selectionIc_) {
                        selectionIc_ = nullptr;
                    }
                    for (auto it = selectionTargets_.begin();
                         it != selectionTargets_.end();) {
                        if (it->second.inputContext == icEvent.inputContext()) {
                            it = selectionTargets_.erase(it);
                        } else {
                            ++it;
                        }
                    }
                    for (auto it = dictationTargets_.begin(); it != dictationTargets_.end();) {
                        if (it->second == icEvent.inputContext()) {
                            it = dictationTargets_.erase(it);
                        } else {
                            ++it;
                        }
                    }
                }));

        // 5. 监听焦点切换：用户切窗口时把上次 auxDown 自动补到新 IC，
        //    确保听写状态提示跟随焦点移动。
        eventHandlers_.push_back(
            instance_->watchEvent(
                EventType::InputContextFocusIn,
                EventWatcherPhase::Default,
                [this](Event &event) {
                    if (lastAuxText_.empty()) return;
                    auto &icEvent = static_cast<InputContextEvent &>(event);
                    auto *ic = icEvent.inputContext();
                    if (!ic) return;
                    instance_->flushUI();
                    ic->inputPanel().setAuxDown(Text(lastAuxText_));
                    ic->updatePreedit();
                    ic->updateUserInterface(UserInterfaceComponent::InputPanel, true);
                    instance_->flushUI();
                }));

        // 6. PostInputMethod：恢复 auxDown（fcitx5 内联模式/方向键后可能清掉）
        eventHandlers_.push_back(
            instance_->watchEvent(
                EventType::InputContextKeyEvent,
                EventWatcherPhase::PostInputMethod,
                [this](Event &event) {
                    if (lastAuxText_.empty()) return;
                    auto &keyEvent = static_cast<KeyEvent &>(event);
                    auto *ic = keyEvent.inputContext();
                    if (!ic) return;
                    ic->inputPanel().setAuxDown(Text(lastAuxText_));
                    ic->updateUserInterface(UserInterfaceComponent::InputPanel, true);
                }));

        FCITX_LOGC(openless, Info) << "OpenLess plugin loaded";
    }

    ~OpenLess() = default;

    // ---- DBus 方法 ----
    // 返回 bool，让调用方区分“无焦点输入上下文”的安全失败和实际提交成功。

    bool commitText(const std::string &text) {
        // 优先使用快捷键按下时保存的输入上下文（savedIc_），
        // 此时用户在目标 app 中，此后胶囊窗口抢焦点不影响提交。
        // 若 savedIc_ 为空则兜底用 foreachFocused。
        auto *ic = savedIc_;
        if (!ic) {
            FCITX_LOGC(openless, Warn)
                << "CommitText: savedIc_ is null, trying foreachFocused";
            auto &mgr = instance_->inputContextManager();
            mgr.foreachFocused([&](InputContext *focusedIc) {
                ic = focusedIc;
                return false;
            });
        }
        if (!ic) {
            FCITX_LOGC(openless, Warn)
                << "CommitText: no input context available";
            // A DBus call must not bring down the fcitx5 host when the target
            // application has no focused input context (for example during
            // startup or in a headless session).  The Rust adapter observes
            // the successful method return and can use its own capability or
            // clipboard fallback policy; fcitx5 remains alive either way.
            return false;
        }
        FCITX_LOGC(openless, Debug) << "CommitText: " << text;
        ic->commitString(text);
        return true;
    }

    std::string captureSelectionTarget(const std::string &ticket) {
        if (ticket.empty() || !selectionIc_) {
            return std::string();
        }
        std::string source;
        const auto &surrounding = selectionIc_->surroundingText();
        if (surrounding.isValid()) {
            source = surrounding.selectedText();
        }
        if (source.empty()) {
            source = getSelectionText();
        }
        if (source.empty()) {
            return std::string();
        }
        selectionTargets_[ticket] = {
            selectionIc_, source, std::string(), surrounding.text(),
            surrounding.cursor(), surrounding.anchor(), surrounding.isValid()};
        return source;
    }

    bool captureDictationTarget(const std::string &ticket) {
        if (ticket.empty() || !savedIc_) return false;
        // A session keeps its own native target even when later key events
        // update savedIc_. Destruction invalidates the ticket instead of
        // redirecting the remaining transcript to a different application.
        return dictationTargets_.emplace(ticket, savedIc_).second;
    }

    bool commitDictationTarget(const std::string &ticket, const std::string &text) {
        auto found = dictationTargets_.find(ticket);
        if (found == dictationTargets_.end()) return false;
        found->second->commitString(text);
        return true;
    }

    bool cancelDictationTarget(const std::string &ticket) {
        return dictationTargets_.erase(ticket) > 0;
    }

    bool applySelectionTarget(const std::string &ticket,
                              const std::string &source,
                              const std::string &replacement) {
        // The ticket is the Core session generation. Never fall back to the
        // current focus here: a preview may have focused the OpenLess window,
        // and writing there would corrupt a different application.
        auto found = selectionTargets_.find(ticket);
        if (found == selectionTargets_.end() || source != found->second.source ||
            replacement.empty()) {
            return false;
        }
        auto *ic = found->second.inputContext;
        const auto &surrounding = ic->surroundingText();
        const auto &captured = found->second;
        // PRIMARY can outlive the selection, and the same selected string may
        // occur at several offsets. Only the original IC's complete surrounding
        // snapshot proves that this exact range is still the intended target.
        // Without surrounding-text support, preview/read remains possible but
        // destructive replacement must fail safely.
        if (!captured.surroundingValid || !surrounding.isValid() ||
            captured.surroundingText != surrounding.text() ||
            captured.cursor != surrounding.cursor() ||
            captured.anchor != surrounding.anchor() ||
            surrounding.selectedText() != source) {
            return false;
        }
        ic->commitString(replacement);
        found->second.replacement = replacement;
        return true;
    }

    bool revertSelectionTarget(const std::string &ticket) {
        auto found = selectionTargets_.find(ticket);
        if (found == selectionTargets_.end() || found->second.replacement.empty()) {
            return false;
        }
        auto *ic = found->second.inputContext;
        const auto &replacement = found->second.replacement;
        const auto &surrounding = ic->surroundingText();
        if (!surrounding.isValid()) {
            FCITX_LOGC(openless, Warn)
                << "RevertSelectionTarget: surrounding text is unavailable";
            return false;
        }
        const auto replacementChars = utf8::lengthValidated(replacement);
        const auto textChars = utf8::lengthValidated(surrounding.text());
        if (replacementChars == utf8::INVALID_LENGTH ||
            textChars == utf8::INVALID_LENGTH ||
            surrounding.cursor() > textChars ||
            surrounding.cursor() < replacementChars) {
            return false;
        }
        const auto &captured = found->second;
        if (surrounding.cursor() != std::min(captured.cursor, captured.anchor) + replacementChars ||
            surrounding.anchor() != surrounding.cursor()) {
            return false;
        }
        auto end = utf8::nextNChar(
            surrounding.text().begin(), surrounding.cursor());
        auto begin = utf8::nextNChar(
            surrounding.text().begin(), surrounding.cursor() - replacementChars);
        if (std::string(begin, end) != replacement) {
            FCITX_LOGC(openless, Warn)
                << "RevertSelectionTarget: text changed after replacement";
            return false;
        }
        ic->deleteSurroundingText(-static_cast<int>(replacementChars),
                                  static_cast<unsigned int>(replacementChars));
        ic->commitString(found->second.source);
        selectionTargets_.erase(found);
        return true;
    }

    bool cancelSelectionTarget(const std::string &ticket) {
        return selectionTargets_.erase(ticket) > 0;
    }

    bool rekeySelectionTarget(const std::string &oldTicket,
                              const std::string &newTicket) {
        auto found = selectionTargets_.find(oldTicket);
        if (found == selectionTargets_.end() || newTicket.empty()) {
            return false;
        }
        auto target = std::move(found->second);
        selectionTargets_.erase(found);
        selectionTargets_[newTicket] = std::move(target);
        return true;
    }

    void setAuxDown(const std::string &text) {
        // 优先用当前焦点 IC（输入面板只在焦点 IC 上渲染），
        // 降级到 savedIc_（快捷键按下时捕获的 IC，可能已失焦但指针仍有效）。
        InputContext *ic = nullptr;
        auto &mgr = instance_->inputContextManager();
        mgr.foreachFocused([&](InputContext *focusedIc) {
            ic = focusedIc;
            return false;
        });
        if (!ic) {
            ic = savedIc_;
        }
        if (!ic) {
            FCITX_LOGC(openless, Warn) << "SetStatusCandidates: no IC (focused=null, saved=null)";
            return;
        }
        FCITX_LOGC(openless, Info) << "SetStatusCandidates: " << text
                                    << " ic=" << ic << " focused=" << (ic != savedIc_ ? "current" : "saved");
        lastAuxText_ = text;
        // 先把事件队列里挂起的旧 UI 更新处理掉（例如前一个按键触发的面板重置），
        // 再设置 auxDown，确保不会被待处理事件覆盖。
        instance_->flushUI();
        ic->inputPanel().setAuxDown(Text(text));
        ic->updatePreedit();
        ic->updateUserInterface(UserInterfaceComponent::InputPanel, true);
        instance_->flushUI();
    }

    void clearAuxDown() {
        // 无论是否有可用 IC，都要清掉缓存的状态文字，否则下一次 FocusIn
        // 会把旧状态（如"已插入"）重放到新聚焦的窗口。
        lastAuxText_.clear();
        InputContext *ic = nullptr;
        auto &mgr = instance_->inputContextManager();
        mgr.foreachFocused([&](InputContext *focusedIc) {
            ic = focusedIc;
            return false;
        });
        if (!ic) {
            ic = savedIc_;
        }
        if (!ic) return;
        FCITX_LOGC(openless, Info) << "ClearStatusCandidates";
        ic->inputPanel().setAuxDown(Text());
        ic->updatePreedit();
        ic->updateUserInterface(UserInterfaceComponent::InputPanel, true);
        instance_->flushUI();
    }

    void setHotkey(const std::vector<std::string> &keys) {
        // 切换预设修饰键时清空自定义组合键，避免双发
        hasCustomDictationKey_ = false;
        resetDictationTriggerState();
        KeyList keyList;
        for (const auto &s : keys) {
            Key key(s);
            if (key.isValid()) {
                keyList.push_back(key);
            } else {
                FCITX_LOGC(openless, Warn)
                    << "SetHotkey: invalid key '" << s << "'";
            }
        }
        config_.triggerKey.setValue(keyList);
        // KeyList 路径激活时清空 raw 路径，避免优先级冲突
        triggerRawSym_ = 0;
        triggerRawStates_ = 0;
        safeSaveAsIni(config_, configFile());
        // 同时清除磁盘上残留的 TriggerRawSym/TriggerRawStates（旧 raw 模式的持久化值），
        // 防止下次 fcitx5 重启 reloadConfig 重新加载旧 raw 热键覆盖新配置。
        {
            RawConfig raw;
            readAsIni(raw, configFile());
            raw.setValueByPath("TriggerRawSym", "0");
            raw.setValueByPath("TriggerRawStates", "0");
            safeSaveAsIni(raw, configFile());
        }
        rebuildTriggerKeys();
    }

    void setHotkeyRaw(uint32_t sym, uint32_t states) {
        // 切换预设修饰键时清空自定义组合键，避免双发
        hasCustomDictationKey_ = false;
        resetDictationTriggerState();
        triggerRawSym_ = sym;
        triggerRawStates_ = states;
        // 同时尝试维护 KeyList（如果 sym 可转为有效 key）
        Key key(static_cast<KeySym>(sym),
                static_cast<KeyStates>(states));
        if (key.isValid()) {
            KeyList keys = {key};
            config_.triggerKey.setValue(keys);
        } else {
            // 修饰键无法用 KeyList 表达，清空 KeyList 避免误匹配
            config_.triggerKey.setValue(KeyList{});
        }
        // 合并写入 config 和 raw sym/states
        RawConfig raw;
        raw.setValueByPath("TriggerRawSym", std::to_string(sym));
        raw.setValueByPath("TriggerRawStates", std::to_string(states));
        raw.setValueByPath("CustomDictationKey", "");
        config_.save(raw);
        safeSaveAsIni(raw, configFile());
        rebuildTriggerKeys();
    }

    void setCustomDictationTrigger(const std::string &keyString) {
        Key key(keyString);
        if (!key.isValid()) {
            FCITX_LOGC(openless, Warn)
                << "SetCustomDictationTrigger: invalid key '" << keyString << "'";
            hasCustomDictationKey_ = false;
            resetDictationTriggerState();
            return;
        }
        customDictationKey_ = key;
        hasCustomDictationKey_ = true;
        resetDictationTriggerState();
        // 有自定义键时清空已有 raw+keylist 路径，避免双发
        triggerRawSym_ = 0;
        triggerRawStates_ = 0;
        config_.triggerKey.setValue(KeyList{});
        // 同时持久化清空 TriggerRawSym/TriggerRawStates，防止 fcitx5 重启后从 INI 加载旧值
        {
            RawConfig raw;
            readAsIni(raw, configFile());
            config_.save(raw);
            raw.setValueByPath("TriggerRawSym", "0");
            raw.setValueByPath("TriggerRawStates", "0");
            // Persist the actual custom binding, not only removal of the old
            // raw binding, so an independent fcitx5 restart retains the key.
            raw.setValueByPath("CustomDictationKey", keyString);
            safeSaveAsIni(raw, configFile());
        }
        FCITX_LOGC(openless, Info)
            << "SetCustomDictationTrigger: '" << keyString << "'"
            << " sym=" << static_cast<uint32_t>(key.sym())
            << " states=" << static_cast<uint32_t>(key.states());
    }

    void setQaHotkeyRaw(uint32_t sym, uint32_t states) {
        qaRawSym_ = sym;
        qaRawStates_ = states;
        RawConfig raw;
        readAsIni(raw, configFile());
        raw.setValueByPath("QaRawSym", std::to_string(sym));
        raw.setValueByPath("QaRawStates", std::to_string(states));
        safeSaveAsIni(raw, configFile());
        FCITX_LOGC(openless, Info)
            << "SetQaHotkeyRaw: sym=" << sym << " states=" << states;
    }

    void setSelectionPolishHotkeyRaw(uint32_t sym, uint32_t states) {
        selectionPolishRawSym_ = sym;
        selectionPolishRawStates_ = states;
        RawConfig raw;
        readAsIni(raw, configFile());
        raw.setValueByPath("SelectionPolishRawSym", std::to_string(sym));
        raw.setValueByPath("SelectionPolishRawStates", std::to_string(states));
        safeSaveAsIni(raw, configFile());
        FCITX_LOGC(openless, Info)
            << "SetSelectionPolishHotkeyRaw: sym=" << sym << " states=" << states;
    }

    void setTranslationHotkeyRaw(uint32_t sym, uint32_t states) {
        translationRawSym_ = sym;
        translationRawStates_ = states;
        RawConfig raw;
        readAsIni(raw, configFile());
        raw.setValueByPath("TranslationRawSym", std::to_string(sym));
        raw.setValueByPath("TranslationRawStates", std::to_string(states));
        safeSaveAsIni(raw, configFile());
        FCITX_LOGC(openless, Info)
            << "SetTranslationHotkeyRaw: sym=" << sym << " states=" << states;
    }

    void setLessComputerHotkeyRaw(uint32_t sym, uint32_t states) {
        lessComputerRawSym_ = sym;
        lessComputerRawStates_ = states;
        lessComputerTriggerHeld_ = false;
        lessComputerTriggerCombined_ = false;
        RawConfig raw;
        readAsIni(raw, configFile());
        raw.setValueByPath("LessComputerRawSym", std::to_string(sym));
        raw.setValueByPath("LessComputerRawStates", std::to_string(states));
        safeSaveAsIni(raw, configFile());
    }

    /// 读取当前 PRIMARY 选区文本。空字符串表示无选区或 clipboard addon 不可用。
    std::string getSelectionText() {
        auto *clipboard = instance_->addonManager().addon("clipboard");
        if (!clipboard) {
            FCITX_LOGC(openless, Debug)
                << "GetSelectionText: clipboard addon not loaded";
            return std::string();
        }
        // primary() 签名接收 const InputContext*，clipboard 模块实现中未使用该参数
        // （读的是全局 primary_ 缓存），这里传 nullptr 即可。
        std::string text = clipboard->call<IClipboard::primary>(nullptr);
        FCITX_LOGC(openless, Debug)
            << "GetSelectionText: " << text.size() << " chars";
        return text;
    }

    FCITX_OBJECT_VTABLE_METHOD(commitText, "CommitText", "s", "b");
    FCITX_OBJECT_VTABLE_METHOD(captureDictationTarget, "CaptureDictationTarget", "s", "b");
    FCITX_OBJECT_VTABLE_METHOD(commitDictationTarget, "CommitDictationTarget", "ss", "b");
    FCITX_OBJECT_VTABLE_METHOD(cancelDictationTarget, "CancelDictationTarget", "s", "b");
    FCITX_OBJECT_VTABLE_METHOD(captureSelectionTarget, "CaptureSelectionTarget", "s", "s");
    FCITX_OBJECT_VTABLE_METHOD(applySelectionTarget, "ApplySelectionTarget", "sss", "b");
    FCITX_OBJECT_VTABLE_METHOD(revertSelectionTarget, "RevertSelectionTarget", "s", "b");
    FCITX_OBJECT_VTABLE_METHOD(rekeySelectionTarget, "RekeySelectionTarget", "ss", "b");
    FCITX_OBJECT_VTABLE_METHOD(cancelSelectionTarget, "CancelSelectionTarget", "s", "b");
    FCITX_OBJECT_VTABLE_METHOD(setAuxDown, "SetAuxDown", "s", "");
    FCITX_OBJECT_VTABLE_METHOD(clearAuxDown, "ClearAuxDown", "", "");
    FCITX_OBJECT_VTABLE_METHOD(setHotkey, "SetHotkey", "as", "");
    FCITX_OBJECT_VTABLE_METHOD(setHotkeyRaw, "SetHotkeyRaw", "uu", "");
    FCITX_OBJECT_VTABLE_METHOD(setCustomDictationTrigger, "SetCustomDictationTrigger", "s", "");
    FCITX_OBJECT_VTABLE_METHOD(setQaHotkeyRaw, "SetQaHotkeyRaw", "uu", "");
    FCITX_OBJECT_VTABLE_METHOD(setSelectionPolishHotkeyRaw, "SetSelectionPolishHotkeyRaw", "uu", "");
    FCITX_OBJECT_VTABLE_METHOD(setTranslationHotkeyRaw, "SetTranslationHotkeyRaw", "uu", "");
    FCITX_OBJECT_VTABLE_METHOD(setLessComputerHotkeyRaw, "SetLessComputerHotkeyRaw", "uu", "");
    FCITX_OBJECT_VTABLE_METHOD(getSelectionText, "GetSelectionText", "", "s");
    FCITX_OBJECT_VTABLE_SIGNAL(dictationKeyEvent, "DictationKeyEvent", "uub");
    FCITX_OBJECT_VTABLE_SIGNAL(dictationKeyCombined, "DictationKeyCombined", "uub");
    FCITX_OBJECT_VTABLE_SIGNAL(lessComputerKeyEvent, "LessComputerKeyEvent", "uub");
    FCITX_OBJECT_VTABLE_SIGNAL(lessComputerKeyCombined, "LessComputerKeyCombined", "uub");
    FCITX_OBJECT_VTABLE_SIGNAL(qaShortcutEvent, "QaShortcutEvent", "uub");
    FCITX_OBJECT_VTABLE_SIGNAL(selectionPolishEvent, "SelectionPolishEvent", "uub");
    FCITX_OBJECT_VTABLE_SIGNAL(translationModifierEvent, "TranslationModifierEvent", "uub");

    Instance *instance() { return instance_; }

    void reloadConfig() override {
        resetDictationTriggerState();
        readAsIni(config_, configFile());
        // 加载原始 sym/states（由 SetHotkeyRaw / SetQaHotkeyRaw / SetTranslationHotkeyRaw 写入的持久化键值）
        RawConfig raw;
        readAsIni(raw, configFile());
        {
            auto *v = raw.valueByPath("TriggerRawSym");
            triggerRawSym_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        {
            auto *v = raw.valueByPath("TriggerRawStates");
            triggerRawStates_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        {
            auto *v = raw.valueByPath("QaRawSym");
            qaRawSym_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        {
            auto *v = raw.valueByPath("QaRawStates");
            qaRawStates_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        {
            auto *v = raw.valueByPath("SelectionPolishRawSym");
            selectionPolishRawSym_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        {
            auto *v = raw.valueByPath("SelectionPolishRawStates");
            selectionPolishRawStates_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        {
            auto *v = raw.valueByPath("TranslationRawSym");
            translationRawSym_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        {
            auto *v = raw.valueByPath("TranslationRawStates");
            translationRawStates_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        {
            auto *v = raw.valueByPath("LessComputerRawSym");
            lessComputerRawSym_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        {
            auto *v = raw.valueByPath("LessComputerRawStates");
            lessComputerRawStates_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        lessComputerTriggerHeld_ = false;
        lessComputerTriggerCombined_ = false;
        rebuildTriggerKeys();
        hasCustomDictationKey_ = false;
        if (auto *value = raw.valueByPath("CustomDictationKey"); value && !value->empty()) {
            Key key(*value);
            if (key.isValid()) {
                customDictationKey_ = key;
                hasCustomDictationKey_ = true;
                triggerRawSym_ = 0;
                triggerKeyList_.clear();
            }
        }
    }

    const Configuration *getConfig() const override {
        return &config_;
    }

    void setConfig(const RawConfig &rawConfig) override {
        config_.load(rawConfig, true);
        safeSaveAsIni(config_, configFile());
        rebuildTriggerKeys();
    }

private:
    // The native-boundary contract fixture supplies real in-process IC handles
    // without synthesizing DBus signals or touching the user's input devices.
    friend struct OpenLessInputTargetContract;
    struct SelectionTarget {
        InputContext *inputContext;
        std::string source;
        std::string replacement;
        // Keep an owned value snapshot, not the live fcitx object: Ubuntu 22.04's
        // Fcitx 5.0.14 SurroundingText is neither copyable nor movable. Plain
        // values also keep the captured range unchanged as the client updates
        // its live context or Core transfers this ticket from QA to a preview.
        std::string surroundingText;
        // Fcitx cursor/anchor offsets count Unicode characters, not UTF-8 bytes;
        // preserve those units for both stale-range checks and undo placement.
        unsigned int cursor;
        unsigned int anchor;
        bool surroundingValid;
    };

    static constexpr const char *configFile() {
        return "conf/openless.conf";
    }

    static bool isModifierKeySym(uint32_t sym) {
        // X11 modifier keysyms.  CapsLock is included to match the desktop hook's
        // treatment of lock keys: pressing it alongside a trigger must not abort
        // dictation as if it were a printable companion key.
        return sym >= 0xffe1 && sym <= 0xffee;
    }

    void resetDictationTriggerState() {
        dictationTriggerHeld_ = false;
        dictationTriggerCombined_ = false;
    }

    void rebuildTriggerKeys() {
        triggerKeyList_ = config_.triggerKey.value();
    }

    Instance *instance_;
    OpenLessConfig config_;
    KeyList triggerKeyList_;
    uint32_t triggerRawSym_;
    uint32_t triggerRawStates_;
    uint32_t qaRawSym_;
    uint32_t qaRawStates_;
    uint32_t selectionPolishRawSym_;
    uint32_t selectionPolishRawStates_;
    uint32_t translationRawSym_;
    uint32_t translationRawStates_;
    uint32_t lessComputerRawSym_;
    uint32_t lessComputerRawStates_;
    Key customDictationKey_;
    bool hasCustomDictationKey_;
    bool dictationTriggerHeld_;
    bool dictationTriggerCombined_;
    bool lessComputerTriggerHeld_;
    bool lessComputerTriggerCombined_;
    /// 快捷键按下时保存的输入上下文指针，用于 commitText 在失焦后仍能提交文字。
    /// 事件处理线程和 DBus 处理线程都是 fcitx5 主事件循环，无竞态。
    /// 通过 InputContextDestroyed 事件监听 IC 销毁时自动清空指针。
    InputContext *savedIc_;
    /// QA/Selection 快捷键按下时的原输入上下文。该指针只能由 fcitx5 主事件循环
    /// 访问，并在 InputContextDestroyed 中与所有关联 ticket 一起失效。
    InputContext *selectionIc_;
    /// Core session UUID -> Host 原生目标。map 只保存 effect 所需的句柄和回滚文本；
    /// Preview/Apply/Completed/Cancelled 状态仍由 Core 独占。
    std::unordered_map<std::string, SelectionTarget> selectionTargets_;
    std::unordered_map<std::string, InputContext *> dictationTargets_;
    /// 上一次 SetAuxDown 的文本；焦点切换时用于自动补到新 IC。
    std::string lastAuxText_;
    std::vector<std::unique_ptr<HandlerTableEntry<EventHandler>>>
        eventHandlers_;
};

class OpenLessFactory : public AddonFactory {
public:
    AddonInstance *create(AddonManager *manager) override {
        return new OpenLess(manager->instance());
    }
};

} // namespace fcitx

FCITX_ADDON_FACTORY(fcitx::OpenLessFactory);
