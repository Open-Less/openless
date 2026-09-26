package com.openless.app

/** JNI bridge from Kotlin overlay / lifecycle code into Rust Coordinator. */
object OpenLessNative {
    private const val BACKEND_CONTRACT_VERSION = "2.0.0"

    init {
        try {
            System.loadLibrary("openless_lib")
        } catch (error: UnsatisfiedLinkError) {
            android.util.Log.e("OpenLessNative", "failed to load openless_lib", error)
        }
    }

    @JvmStatic external fun nativeStartDictation()

    @JvmStatic external fun nativeStartDictationForIme()

    @JvmStatic external fun nativeStartDictationWithTranslation(translation: Boolean)

    @JvmStatic external fun nativeStopDictation()

    @JvmStatic external fun nativeStopDictationForIme()

    /** Same as nativeStopDictationForIme(), but skips the LLM polish step for this utterance — the IME keyboard's mic-button swipe-up gesture. */
    @JvmStatic external fun nativeStopDictationForImeWithRaw(raw: Boolean)

    @JvmStatic external fun nativeStopDictationWithTranslation(translation: Boolean)

    @JvmStatic external fun nativeStopDictationAsQuickNote()

    @JvmStatic external fun nativeCancelDictation()

    // Adds a word/phrase straight to the global Dictionary (same store
    // add_vocab exposes to the desktop UI) instead of a CorrectionRule —
    // see native_bridge.rs's spawn_add_vocabulary_word() doc comment for
    // why the IME's own edit/correction flows moved to this. The older
    // CorrectionRule bindings below are kept too — upstream/beta's own
    // code paths still use them; this file just adds an alternative for
    // this branch's own IME flows, not a replacement.
    @JvmStatic external fun nativeAddVocabularyWord(phrase: String)

    /** Records a hand-corrected span from the IME's "edit result" flow into the shared correction dictionary. */
    @JvmStatic external fun nativeAddCorrectionRule(pattern: String, replacement: String)

    /** JSON array of every existing correction rule's pattern — lets the clipboard swipe-left gesture show "add" vs. "remove" before the drag finishes. */
    @JvmStatic external fun nativeCorrectionRulePatterns(): String

    /** Removes every correction rule whose pattern exactly matches — the clipboard swipe-left "remove" action. */
    @JvmStatic external fun nativeRemoveCorrectionRule(pattern: String)

    /** JSON array of every existing Dictionary entry's phrase — same purpose as nativeCorrectionRulePatterns(), for the clipboard swipe-left zone now that adding writes to the Dictionary instead. */
    @JvmStatic external fun nativeVocabularyPhrases(): String

    /** Removes every Dictionary entry whose phrase exactly matches — the clipboard swipe-left "remove" action's counterpart to nativeAddVocabularyWord(). */
    @JvmStatic external fun nativeRemoveVocabularyWord(phrase: String)

    /**
     * Registers this Context as the one with_android_env() (every
     * Rust->Kotlin JNI call, including dictation/waveform capsule updates)
     * routes through — replaces whatever was registered before, since a
     * GlobalRef stays valid for the registrant's whole lifecycle. Despite
     * the name, this was never actually Activity-specific on the Rust side
     * (register_active_activity() in jni.rs just stores a generic JObject),
     * and no longer requires one here either: OpenLessRuntimeService now
     * registers itself, not OpenLessBackendWarmupActivity — a foreground
     * Service that starts/stops in lockstep with the IME being active is a
     * far more stable registrant than an Activity that spends nearly its
     * entire life backgrounded via moveTaskToBack() and can be reclaimed by
     * the OS at any point during that (see OpenLessRuntimeService.onCreate()
     * for the current registrant, and OpenLessProcessRestartStats' "actkill"
     * history for what depending on the Activity instead used to cost).
     * Call from onCreate(); pair with nativeUnregisterActivityContext() in
     * onDestroy().
     */
    @JvmStatic external fun nativeRegisterActivityContext(context: android.content.Context)

    /** Clears the registration from nativeRegisterActivityContext() — only takes effect if `context` is still the currently-registered one. */
    @JvmStatic external fun nativeUnregisterActivityContext(context: android.content.Context)

    /**
     * True once some registrant (see nativeRegisterActivityContext()) has
     * called it and nothing has unregistered since. The Rust backend can
     * stay perfectly healthy for a long time after its last registered
     * Context is gone (that's the whole point of Phase 1/2's recovery
     * design) — requireBackendContract() alone can't see that gap, since it
     * only checks whether the backend itself is running, not whether
     * there's still something around for it to notify.
     */
    @JvmStatic external fun nativeHasRegisteredActivityContext(): Boolean

    @JvmStatic external fun nativeBackendSnapshot(): String

    @JvmStatic
    fun requireBackendContract() {
        val response = org.json.JSONObject(nativeBackendSnapshot())
        val version = response.optString("contractVersion")
        check(version == BACKEND_CONTRACT_VERSION) {
            "unsupported backend contract version: $version"
        }
        check(response.optBoolean("ok")) {
            response.optString("error", "backend unavailable")
        }
    }

    @JvmStatic external fun nativeSwitchStylePack()

    @JvmStatic external fun nativeFinalizeQaFromOverlay()

    @JvmStatic external fun nativeNotifyOverlayDestroyed()

    /**
     * Idempotent — safe to call from every OpenLessBackendWarmupActivity.onCreate().
     * No-ops when the "main" WebviewWindow already exists; otherwise rebuilds it,
     * attaching to whichever Activity instance most recently registered with Tao
     * (this one). See android/native_bridge.rs's ensure_main_webview_window() doc
     * comment for why a fresh Activity instance never gets onWebViewCreate()
     * without this — the settings-reopen black screen fix.
     */
    @JvmStatic external fun nativeEnsureMainWebviewWindow(): Boolean

    // Settings export/import (OpenLessKeyboardSettingsActivity's "导出/导入配置")
    // — see native_bridge.rs's export/import*Json() functions for what each
    // one actually reads/writes. Synchronous, unlike the dictation lifecycle
    // calls above: plain file reads/writes, safe to call straight from the
    // settings Activity's own UI thread for a one-off export/import tap.

    /** JSON: {activeAsrProvider, activeLlmProvider, activeStylePackId, selectionPolishStylePackId}. */
    @JvmStatic external fun nativeExportPreferencesSubset(): String

    /** Applies whichever of the four fields above are present in [json]; others are left untouched. */
    @JvmStatic external fun nativeImportPreferencesSubset(json: String)

    /** JSON: the same shape as the Rust-side CredentialsSnapshot (ASR/LLM provider credential fields) — see persistence/credentials.rs. */
    @JvmStatic external fun nativeExportCredentialsSnapshot(): String

    /** Applies whichever fields are present (non-null) in [json]; others are left untouched. */
    @JvmStatic external fun nativeImportCredentialsSnapshot(json: String)
}
