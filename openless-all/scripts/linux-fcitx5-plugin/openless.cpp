/*
 * SPDX-FileCopyrightText: 2025 OpenLess Contributors
 *
 * SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fcitx5 插件 — 供 OpenLess 听写文字提交 + 快捷键监听。
 *
 * DBus 接口: org.fcitx.Fcitx.OpenLess1  (对象路径 /openless)
 *  方法:
 *    CommitText(s: text)           — 将文字提交到当前焦点输入上下文
 *                                    安全性：本接口在会话总线(session bus)上对同用户
 *                                    所有进程开放，此为 fcitx5/IBus 体系的标准安全模型
 *                                    （非特权进程隔离）。
 *    SetHotkey(as: keys)           — 设置听写触发快捷键 (Key::parse 格式)
 *    SetHotkeyRaw(uu: sym, states) — 直接设听写触发 sym+states (不走 parse)
 *    SetCustomDictationTrigger(s: keyString) — 设置自定义组合键 (Key::parse 格式)
 *    SetQaHotkeyRaw(uu: sym, states)     — 直接设 QA 面板触发 sym+states
 *    SetTranslationHotkeyRaw(uu: sym, states) — 直接设翻译模式触发 sym+states
 *    SetAuxDown(s: text)                 — 在候选词列表下方显示状态文本
 *    ClearAuxDown()                      — 清除候选词列表下方文本
 *  信号:
 *    DictationKeyEvent(uub: sym, states, isPress) — 听写热键按下/抬起
 *    QaShortcutEvent(uub: sym, states, isPress)   — QA 快捷键按下/抬起
 *    TranslationModifierEvent(uub: sym, states, isPress) — 翻译修饰键按下/抬起
 */

#include <memory>
#include <string>
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
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/event.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputcontextmanager.h>
#include <fcitx/inputpanel.h>
#include <fcitx/instance.h>
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
          translationRawSym_(0),
          translationRawStates_(0),
          hasCustomDictationKey_(false),
          savedIc_(nullptr) {

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
                        auto dsym = triggerRawSym_ != 0 ? triggerRawSym_
                            : static_cast<uint32_t>(triggerKeyList_[0].sym());
                        auto dstates = triggerRawStates_ != 0 ? triggerRawStates_
                            : static_cast<uint32_t>(triggerKeyList_[0].states());
                        FCITX_LOGC(openless, Debug)
                            << "Dictation hotkey sym=" << dsym;
                        dictationKeyEvent(dsym, dstates, isPress);
                        keyEvent.filterAndAccept();
                        return;
                    }
                    if (qaRawSym_ != 0 && sym == qaRawSym_ &&
                        states == qaRawStates_) {
                        FCITX_LOGC(openless, Debug)
                            << "QA shortcut";
                        qaShortcutEvent(qaRawSym_, qaRawStates_, isPress);
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
    // 返回 void 而非 std::tuple<>，以匹配 FCITX_OBJECT_VTABLE_METHOD 的 RET("")

    void commitText(const std::string &text) {
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
            throw std::runtime_error("no focused input context");
        }
        FCITX_LOGC(openless, Debug) << "CommitText: " << text;
        ic->commitString(text);
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
            return;
        }
        customDictationKey_ = key;
        hasCustomDictationKey_ = true;
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

    FCITX_OBJECT_VTABLE_METHOD(commitText, "CommitText", "s", "");
    FCITX_OBJECT_VTABLE_METHOD(setAuxDown, "SetAuxDown", "s", "");
    FCITX_OBJECT_VTABLE_METHOD(clearAuxDown, "ClearAuxDown", "", "");
    FCITX_OBJECT_VTABLE_METHOD(setHotkey, "SetHotkey", "as", "");
    FCITX_OBJECT_VTABLE_METHOD(setHotkeyRaw, "SetHotkeyRaw", "uu", "");
    FCITX_OBJECT_VTABLE_METHOD(setCustomDictationTrigger, "SetCustomDictationTrigger", "s", "");
    FCITX_OBJECT_VTABLE_METHOD(setQaHotkeyRaw, "SetQaHotkeyRaw", "uu", "");
    FCITX_OBJECT_VTABLE_METHOD(setTranslationHotkeyRaw, "SetTranslationHotkeyRaw", "uu", "");
    FCITX_OBJECT_VTABLE_SIGNAL(dictationKeyEvent, "DictationKeyEvent", "uub");
    FCITX_OBJECT_VTABLE_SIGNAL(qaShortcutEvent, "QaShortcutEvent", "uub");
    FCITX_OBJECT_VTABLE_SIGNAL(translationModifierEvent, "TranslationModifierEvent", "uub");

    Instance *instance() { return instance_; }

    void reloadConfig() override {
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
            auto *v = raw.valueByPath("TranslationRawSym");
            translationRawSym_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        {
            auto *v = raw.valueByPath("TranslationRawStates");
            translationRawStates_ = v ? std::stoul(*v, nullptr, 0) : 0;
        }
        rebuildTriggerKeys();
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
    static constexpr const char *configFile() {
        return "conf/openless.conf";
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
    uint32_t translationRawSym_;
    uint32_t translationRawStates_;
    Key customDictationKey_;
    bool hasCustomDictationKey_;
    /// 快捷键按下时保存的输入上下文指针，用于 commitText 在失焦后仍能提交文字。
    /// 事件处理线程和 DBus 处理线程都是 fcitx5 主事件循环，无竞态。
    /// 通过 InputContextDestroyed 事件监听 IC 销毁时自动清空指针。
    InputContext *savedIc_;
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
