package com.openless.app

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.content.Context
import android.content.Intent

/**
 * Starts the Tauri/Rust runtime — usually invisibly (warmup/recovery), but
 * also the app's actual launcher target (the manifest's LAUNCHER
 * intent-filter lives here, not on bare MainActivity — see
 * merge-android-overlay-manifest.mjs's moveLauncherIntentFilterToWarmupActivity()
 * for why: a direct launcher tap used to open untracked plain MainActivity,
 * a second Tauri host whose WebView never got anything attached).
 */
class OpenLessBackendWarmupActivity : MainActivity() {
    // Activity 实例化阶段尚未 attach Context，不能访问 Activity.mainLooper。
    private val warmupHandler = Handler(Looper.getMainLooper())
    // How long sendToBackground() has already spent polling for the WebView
    // to finish its first load, this warmup cycle — reset in onCreate()
    // right before the first postDelayed(). Not reset anywhere else: this
    // Activity's onCreate() only ever runs once per singleTask instance, and
    // sendToBackground only ever runs once per instance too (onNewIntent()
    // cancels the pending one instead of letting a second cycle start).
    private var webViewReadyWaitElapsedMs = 0L
    // lateinit + assigned in init{}, not a plain `val = Runnable { ... }`:
    // the lambda below reschedules itself by referencing this same property
    // (needed so warmupHandler.removeCallbacks(sendToBackground) elsewhere
    // in this class always cancels the exact instance that was posted), and
    // Kotlin can't resolve that self-reference inside a single property
    // initializer expression (fails with "Variable must be initialized" /
    // a recursive type-checking error) — splitting the declaration from the
    // assignment gives the lambda a property that already exists to close over.
    private lateinit var sendToBackground: Runnable

    init {
        sendToBackground = Runnable {
            if (settingsRequested || isFinishing || isDestroyed) return@Runnable
            // On-device logs showed a genuinely black settings page whose window
            // itself was drawn and focused fine, but whose WebView had never
            // rendered a single frame — no renderer process ever spawned, no
            // crash either. The likely cause: this Runnable used to fire
            // unconditionally 180ms after onCreate(), which is well within the
            // time a cold Tauri/WebView init can still be in flight (the exact
            // race the overridePendingTransition/moveTaskToBack comment below
            // already worried about for HWUI's worker pool) — backgrounding the
            // window mid-init apparently can wedge the WebView itself, not just
            // its renderer process, and since this Activity is singleTask and
            // never recreated, every later "genuine settings open" just reopens
            // the same permanently-wedged instance. Polling WebView.progress
            // (a plain getter, no WebViewClient override needed — Tauri/Wry
            // installs its own client for the JS<->Rust bridge and overriding it
            // here would break that) instead of trusting a fixed delay lets a
            // slow cold start finish before this window ever gets backgrounded.
            // The wait is capped (WEBVIEW_READY_MAX_WAIT_MS) so a WebView that
            // never reaches 100 for some unrelated reason can't keep the silent
            // warmup path visible forever.
            // webViewRef is still null until onWebViewCreate() fires — on a
            // cold process start that hasn't happened yet by the time this
            // first runs, which is exactly the case most in need of waiting,
            // not a "nothing to wait for" free pass. (An earlier version of
            // this check read `webView == null || ...`, treating "not
            // created yet" as ready — that let a cold start still hit the
            // exact bug this polling was meant to fix.)
            val webView = webViewRef
            val webViewReady = webView != null && webView.progress >= 100
            if (!webViewReady && webViewReadyWaitElapsedMs < WEBVIEW_READY_MAX_WAIT_MS) {
                webViewReadyWaitElapsedMs += WEBVIEW_READY_POLL_INTERVAL_MS
                warmupHandler.postDelayed(sendToBackground, WEBVIEW_READY_POLL_INTERVAL_MS)
                return@Runnable
            }
            android.util.Log.i(
                "OpenLessBackendWarmupActivity",
                "sendToBackground firing waitedMs=$webViewReadyWaitElapsedMs webViewReady=$webViewReady",
            )
            // Tauri/Rust runtime is owned by this Activity. Keep it alive as the
            // single UI/runtime host, but never relaunch the editor's package here:
            // a package launch intent only knows that app's launcher Activity, which
            // for apps like Settings or WeChat mini programs is not the screen the
            // user was actually typing in, and replacing it destroys their context.
            // Moving this task behind the current one preserves the exact
            // Activity/window that requested the IME.
            overridePendingTransition(0, 0)
            moveTaskToBack(true)
            // Only bother trying to restore the panel if it was actually the
            // thing on screen right before this Activity stole focus (see
            // restoreInputPanelAfterWarmup's own doc comment) — a warmup
            // triggered by an unrelated app's onStartInput(), or by the
            // periodic backend heartbeat while nothing was being typed into,
            // has nothing to restore.
            if (restoreInputPanelAfterWarmup) {
                restoreInputPanelAfterWarmup = false
                OpenLessImeService.requestInputPanelAfterWarmup(260L)
            }
        }
    }

    private var settingsRequested = false
    private var webViewRef: android.webkit.WebView? = null

    // See onCreate()'s settingsRequested branch and onWebViewCreate() below.
    //
    // Restarts the whole process on the very first stuck instance, not just
    // finishing it — on-device testing found that once Wry/Tauri's Android
    // CreateWebView dispatch silently fails once, the "main" window label it
    // partially registered can never be freed again: destroy()/close() only
    // clear tauri-runtime-wry's own internal window reference, while Tauri's
    // higher-level WindowManager only drops a window's label in response to
    // a genuine platform WindowEvent::Destroyed — which never fires for a
    // WebView that was never actually created. Every later
    // ensureMainWebviewWindow() call then hits a permanent "a webview with
    // label `main` already exists" error, with no way to clear it from here
    // (that removal logic is private to the tauri crate). Confirmed by
    // repeated identical failures across many retries in one test session,
    // recovering only after the whole process was killed and relaunched —
    // restartProcessAsLastResort() mirrors that manual recovery
    // automatically instead of leaving the user stuck. (An earlier version
    // of this only restarted after 2 consecutive failures; user feedback was
    // that a single stuck instance already reads as broken, so this is now
    // unconditional — restartProcessAsLastResort()'s own cooldown is what
    // guards against a restart loop.)
    private val webViewCreationWatchdog = Runnable {
        if (webViewRef != null || isFinishing || isDestroyed) return@Runnable
        android.util.Log.w(
            "OpenLessBackendWarmupActivity",
            "webView never created within ${WEBVIEW_CREATION_WATCHDOG_MS}ms; restarting stuck instance " +
                "activityHash=${System.identityHashCode(this)}",
        )
        // restartProcessAsLastResort() can decline (its own cooldown) —
        // still finish this stuck instance either way, so the next Logo tap
        // at least gets a fresh retry instead of staring at this same dead
        // black window.
        if (!restartProcessAsLastResort(applicationContext)) {
            finishAndRemoveTask()
        }
    }

    // See requestSettingsReattach()/maybePerformPendingSettingsReattach()/
    // performWebViewReattach() below. settingsOpenRequestId increments on
    // every settings-open (cold onCreate() or warm onNewIntent()), purely
    // for correlating log lines — only the onNewIntent() (warm reuse) path
    // ever sets pendingSettingsReattach, since a fresh onCreate() already
    // gets a genuinely fresh WebView/Surface pairing with nothing to fix.
    private var settingsOpenRequestId = 0L
    private var pendingSettingsReattach = false
    private var reattachedForRequestId = -1L
    private var activityResumed = false

    // WryActivity's own hook, fired once when the WebView is first created
    // for this Activity instance.
    override fun onWebViewCreate(webView: android.webkit.WebView) {
        super.onWebViewCreate(webView)
        webViewRef = webView
        warmupHandler.removeCallbacks(webViewCreationWatchdog)
        android.util.Log.i(
            "OpenLessBackendWarmupActivity",
            "onWebViewCreate webView=${System.identityHashCode(webView)} activityHash=${System.identityHashCode(this)} " +
                "isAttachedToWindow=${webView.isAttachedToWindow}",
        )
        // Diagnostic only — confirms a reattach (see performWebViewReattach())
        // actually ran a real detach/attach cycle rather than just having
        // its code path executed, and doubles as one of the triggers that
        // re-checks maybePerformPendingSettingsReattach()'s conditions,
        // since a reattach's own addView() causes this to fire too.
        webView.addOnAttachStateChangeListener(object : android.view.View.OnAttachStateChangeListener {
            override fun onViewAttachedToWindow(v: android.view.View) {
                android.util.Log.i(
                    "OpenLessBackendWarmupActivity",
                    "webView attached requestId=$settingsOpenRequestId webView=${System.identityHashCode(v)} " +
                        "windowVisibility=${v.windowVisibility} hasWindowFocus=${hasWindowFocus()} " +
                        "parent=${System.identityHashCode(v.parent)}",
                )
                maybePerformPendingSettingsReattach("onViewAttachedToWindow")
            }

            override fun onViewDetachedFromWindow(v: android.view.View) {
                android.util.Log.i(
                    "OpenLessBackendWarmupActivity",
                    "webView detached requestId=$settingsOpenRequestId webView=${System.identityHashCode(v)}",
                )
            }
        })
    }

    /**
     * Re-mounts the already-alive WebView onto the Activity's own content
     * view: removes it from its parent, then — on the parent's own next
     * Choreographer animation frame (the parent, not the WebView, since it
     * stays attached to the window the whole time; posting on the
     * just-detached WebView itself would queue into a run-queue that only
     * flushes once *something else* reattaches it, i.e. never) — adds it
     * back at the same index with the same LayoutParams, restoring scroll
     * position and focus. This is a controlled experiment targeting a
     * specific, evidence-backed correlation, not a proven fix: on-device
     * logs show this Activity's outer Window Surface gets torn down
     * (BLASTBufferQueue disconnect → destroySurface → NO_SURFACE) every
     * time sendToBackground() calls moveTaskToBack() and a fresh Surface
     * gets allocated the next time this singleTask instance is brought
     * back via onNewIntent() — while the Activity, WebView, renderer
     * process, and Rust runtime all stay alive throughout (confirmed via
     * dumpsys: mHasSurface=true, isReadyForDisplay()=true,
     * mDrawState=HAS_DRAWN by the time the black screen is reported, and
     * the WebView renderer process itself never frozen or gone). The
     * failure correlates with this WebView being reused across that
     * Surface teardown/recreate cycle without ever going through a real
     * View detach/attach of its own — Android does not document a
     * guarantee that forcing one causes Chromium to recomposite onto the
     * new Surface, so this needs real-device verification over many
     * background/foreground cycles, not just a build that compiles.
     *
     * Deliberately NOT: reload()ing the page (resets JS/app state and
     * doesn't address a Surface-binding problem), any postVisualStateCallback
     * / WebView.progress based "did it actually render" detection (both
     * proved unreliable — see the git history this replaced), or
     * recreate()ing the Activity (walks WryActivity through a real
     * onDestroy()/onCreate() cycle, notifying Rust of an Activity+WebView
     * destroy/recreate it doesn't actually need, and this project has
     * separately hit real JNI/Context staleness bugs around exactly that
     * kind of Activity churn before).
     */
    private fun performWebViewReattach(webView: android.webkit.WebView, requestId: Long) {
        val parent = webView.parent as? android.view.ViewGroup
        if (parent == null) {
            android.util.Log.w(
                "OpenLessBackendWarmupActivity",
                "reattach skipped requestId=$requestId: webView has no ViewGroup parent",
            )
            return
        }
        val index = parent.indexOfChild(webView)
        if (index < 0) {
            android.util.Log.w(
                "OpenLessBackendWarmupActivity",
                "reattach skipped requestId=$requestId: webView not found among its own parent's children",
            )
            return
        }
        val layoutParams = webView.layoutParams
        val hadFocus = webView.hasFocus()
        val scrollX = webView.scrollX
        val scrollY = webView.scrollY
        android.util.Log.i(
            "OpenLessBackendWarmupActivity",
            "reattach removeView requestId=$requestId webView=${System.identityHashCode(webView)} " +
                "parent=${System.identityHashCode(parent)} index=$index hadFocus=$hadFocus",
        )
        parent.removeView(webView)
        parent.postOnAnimation {
            if (isFinishing || isDestroyed) {
                android.util.Log.w(
                    "OpenLessBackendWarmupActivity",
                    "reattach addView skipped requestId=$requestId: activity finishing/destroyed",
                )
                return@postOnAnimation
            }
            parent.addView(webView, index, layoutParams)
            webView.scrollTo(scrollX, scrollY)
            if (hadFocus) webView.requestFocus()
            // Wry's own onResume()/onPause() overrides already call this —
            // see WryActivity.kt — so this is redundant insurance, not the
            // thing actually expected to matter here; the cross-frame
            // detach/attach above is.
            webView.onResume()
            webView.requestLayout()
            webView.invalidate()
            parent.requestLayout()
            parent.invalidate()
            android.util.Log.i(
                "OpenLessBackendWarmupActivity",
                "reattach addView complete requestId=$requestId webView=${System.identityHashCode(webView)}",
            )
        }
    }

    /**
     * Arms a pending reattach for the current settingsOpenRequestId and
     * immediately checks whether it can run right away — a settings
     * request arriving while the Activity is already resumed/focused
     * shouldn't wait for a lifecycle callback that isn't coming again.
     */
    private fun requestSettingsReattach() {
        settingsOpenRequestId += 1
        pendingSettingsReattach = true
        android.util.Log.i(
            "OpenLessBackendWarmupActivity",
            "requestSettingsReattach requestId=$settingsOpenRequestId",
        )
        maybePerformPendingSettingsReattach("requestSettingsReattach")
    }

    /**
     * Only actually reattaches once every one of these is true, checked
     * fresh on each call from whichever lifecycle/attach callback fires
     * next (onResume/onWindowFocusChanged/the WebView's own attach
     * listener) — no fixed delay, no polling, just re-evaluating real
     * state every time something relevant changes:
     *  - the Activity is resumed and has window focus (not just RESUMED —
     *    window focus is what actually implies the Surface is usable);
     *  - the WebView is attached to its window and that window is visible;
     *  - the Activity isn't finishing/destroyed.
     * reattachedForRequestId makes this idempotent per
     * settingsOpenRequestId — onResume() and onWindowFocusChanged(true)
     * can both fire for the same settings-open, and only the first one to
     * see every condition satisfied should act.
     */
    private fun maybePerformPendingSettingsReattach(trigger: String) {
        if (!pendingSettingsReattach) return
        val requestId = settingsOpenRequestId
        val webView = webViewRef
        // Logged unconditionally (not just once conditions are all met) —
        // the earlier version of this function returned silently on the
        // first failing check, which made it impossible to tell from logs
        // alone which of the five conditions was the blocker on a given
        // trigger.
        android.util.Log.i(
            "OpenLessBackendWarmupActivity",
            "maybePerformPendingSettingsReattach trigger=$trigger requestId=$requestId " +
                "reattachedForRequestId=$reattachedForRequestId isFinishing=$isFinishing isDestroyed=$isDestroyed " +
                "activityResumed=$activityResumed hasWindowFocus=${hasWindowFocus()} " +
                "webView=${webView?.let { System.identityHashCode(it) }} " +
                "webViewAttached=${webView?.isAttachedToWindow} webViewWindowVisibility=${webView?.windowVisibility}",
        )
        if (reattachedForRequestId == requestId) return
        if (isFinishing || isDestroyed) return
        if (!activityResumed || !hasWindowFocus()) return
        if (webView == null) return
        if (!webView.isAttachedToWindow || webView.windowVisibility != android.view.View.VISIBLE) return
        reattachedForRequestId = requestId
        pendingSettingsReattach = false
        android.util.Log.i(
            "OpenLessBackendWarmupActivity",
            "reattach conditions met trigger=$trigger requestId=$requestId webView=${System.identityHashCode(webView)}",
        )
        performWebViewReattach(webView, requestId)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Idempotent no-op when Wry's own cold-start CreateWebView already
        // fired for this instance (the common, first-ever-Activity case) —
        // only does real work for a fresh instance created after the
        // previous one was truly destroyed, where Wry's android_setup()
        // never sends CreateWebView on its own (no WEBVIEW_ATTRIBUTES entry
        // for a never-seen-before activity_id). See
        // OpenLessNative.nativeEnsureMainWebviewWindow()'s doc comment.
        runCatching { OpenLessNative.nativeEnsureMainWebviewWindow() }
            .onSuccess { ok -> android.util.Log.i("OpenLessBackendWarmupActivity", "ensureMainWebviewWindow ok=$ok") }
            .onFailure { error -> android.util.Log.w("OpenLessBackendWarmupActivity", "ensure main webview window failed", error) }
        activeInstance = java.lang.ref.WeakReference(this)
        // with_android_env()'s JNI Context registration is no longer this
        // Activity's concern at all — see
        // OpenLessNative.nativeRegisterActivityContext()'s doc comment:
        // OpenLessRuntimeService registers itself instead, since it's a
        // far more stable registrant than an Activity that spends nearly
        // its whole life backgrounded via moveTaskToBack().
        //
        // A direct launcher-icon tap arrives here as a plain ACTION_MAIN/
        // CATEGORY_LAUNCHER intent (no EXTRA_SHOW_SETTINGS) — treated the
        // same as an explicit settings request: the user tapped the icon
        // expecting to see the app, not an invisible warmup that vanishes
        // 180ms later.
        val launchedFromLauncher = intent.action == Intent.ACTION_MAIN &&
            intent.hasCategory(Intent.CATEGORY_LAUNCHER)
        // settingsOpenPending covers a cold-start race: openSettings()/
        // openSettingsIfRunning() set it synchronously before ever calling
        // startActivity(), so it is already true here even if this onCreate()
        // actually happened to be triggered by a concurrent, unrelated
        // ensureBackendReady() warmup racing to create the same singleTask
        // instance first (observed on-device: a silent warmup and a
        // launcher-icon tap landing within ~10ms of each other after the
        // OS killed the process in the background) — the alternative,
        // reading only this Intent's own extras, depends on onNewIntent()
        // winning that race, which is not guaranteed.
        settingsRequested = launchedFromLauncher || intent.getBooleanExtra(EXTRA_SHOW_SETTINGS, false) || settingsOpenPending
        android.util.Log.i(
            "OpenLessBackendWarmupActivity",
            "onCreate settingsRequested=$settingsRequested launchedFromLauncher=$launchedFromLauncher " +
                "hasExtra=${intent.getBooleanExtra(EXTRA_SHOW_SETTINGS, false)} settingsOpenPending=$settingsOpenPending " +
                "activityHash=${System.identityHashCode(this)}",
        )
        settingsOpenPending = false
        // Only when visibly opened for settings: a permission dialog here
        // during the invisible warmup path would get dragged to the
        // background along with this Activity by sendToBackground() 180ms
        // later, before the user could ever answer it. Android never
        // auto-requests POST_NOTIFICATIONS (API 33+) — without asking
        // explicitly at least once, OpenLessRuntimeService's foreground
        // notification stays silently blocked and "Manage notifications"
        // shows as a fixed, non-interactive "don't allow" in Settings,
        // since there is nothing granted to manage.
        if (settingsRequested) {
            requestNotificationPermissionIfNeeded()
            // No reattach requested here (see requestSettingsReattach()):
            // a fresh onCreate() means a fresh WebView on a fresh Window
            // Surface, the exact pairing that's already proven to render
            // correctly (a cold app launch always recovers) — nothing to
            // fix. Only bumped for log correlation.
            settingsOpenRequestId += 1
            android.util.Log.i(
                "OpenLessBackendWarmupActivity",
                "settings open via cold onCreate requestId=$settingsOpenRequestId — no reattach needed",
            )
            // Safety net: nativeEnsureMainWebviewWindow() above can report
            // success while the actual WebView creation still silently
            // never reaches this Activity (observed on-device even with a
            // genuinely unique activity_id — some deeper Tauri/Wry window-
            // creation dispatch issue not yet root-caused). Left alone,
            // this Activity stays alive forever with webViewRef == null: a
            // permanent black screen, and every later openSettings() call
            // just redelivers to this same broken instance via
            // onNewIntent() (never onCreate() again), so it can never
            // recover on its own. Finishing it here instead — cancelled by
            // onWebViewCreate() the moment a real WebView does show up —
            // means the next Logo tap creates a genuinely fresh instance
            // (with its own new activity_id) rather than being stuck.
            warmupHandler.postDelayed(webViewCreationWatchdog, WEBVIEW_CREATION_WATCHDOG_MS)
        }

        // 不再修改窗口透明度或触摸属性。主 Activity 必须以正常窗口完成
        // Tauri/WebView 初始化，完成后仅退到后台，避免留下黑色/空白窗口状态。
        // Tauri/WebView keeps initializing natively after super.onCreate() returns.
        // Backgrounding this window while that is still in flight has produced a
        // native "destroyed mutex" abort in HWUI's worker pool; suppressing the
        // enter transition avoids extra render work racing with that teardown.
        overridePendingTransition(0, 0)
        warmupHandler.postDelayed(sendToBackground, 180L)
    }

    @Suppress("DEPRECATION", "MissingSuperCall")
    override fun onBackPressed() {
        overridePendingTransition(0, 0)
        if (settingsRequested) {
            // Trial change, being verified on-device for a HWUI/native-mutex
            // regression before trusting it: closing settings now finishes
            // this instance outright instead of moveTaskToBack()-hiding it.
            // The old worry (see below) was specifically about backgrounding
            // *mid cold-start init* — sendToBackground()'s own early-return
            // (`if (settingsRequested) return@Runnable`) already means
            // moveTaskToBack() was never reachable here while settings was
            // open anyway, so this isn't adding a new code path so much as
            // replacing the one genuinely reachable one. Now that a fresh
            // rebuild reliably gets a real WebView (activity_id collision
            // fixed, see ensureMainWebviewWindow's call site above, with
            // webViewCreationWatchdog as a backup), a clean finish + rebuild
            // is simpler than reattaching a WebView across a Surface that
            // moveTaskToBack() tore down.
            finishAndRemoveTask()
            return
        }
        // This Activity is the single, process-lifetime Tauri/Rust host and must
        // never actually finish() while the process is alive: finishing destroys
        // the window Surface (unlike moveTaskToBack, which only hides it), and
        // that race with HWUI's worker-pool teardown is what produces the native
        // "destroyed mutex" abort. The default back behavior would finish() this
        // Activity once there is no more back-stack, so always background it
        // instead — skipping super.onBackPressed() is intentional here.
        moveTaskToBack(true)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        val launchedFromLauncher = intent.action == Intent.ACTION_MAIN &&
            intent.hasCategory(Intent.CATEGORY_LAUNCHER)
        if (launchedFromLauncher || intent.getBooleanExtra(EXTRA_SHOW_SETTINGS, false) || settingsOpenPending) {
            settingsRequested = true
            settingsOpenPending = false
            android.util.Log.i(
                "OpenLessBackendWarmupActivity",
                "onNewIntent settingsRequested=true launchedFromLauncher=$launchedFromLauncher " +
                    "hasExtra=${intent.getBooleanExtra(EXTRA_SHOW_SETTINGS, false)}",
            )
            warmupHandler.removeCallbacks(sendToBackground)
            // Covers openSettingsIfRunning() bringing an already-alive
            // instance forward, and the launcher icon being tapped again
            // while this Activity is already alive (singleTask redelivers
            // via onNewIntent instead of a fresh onCreate) — either way,
            // onCreate()'s own call to this never runs again for those
            // cases, so this is the only other place a visible moment happens.
            requestNotificationPermissionIfNeeded()
            // The common case in practice: this singleTask instance's
            // WebView has likely been sitting on a torn-down Window
            // Surface since whenever sendToBackground() last backgrounded
            // it — see performWebViewReattach()'s doc comment.
            requestSettingsReattach()
        }
    }

    private fun requestNotificationPermissionIfNeeded() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED) return
        requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQUEST_POST_NOTIFICATIONS)
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQUEST_POST_NOTIFICATIONS) return
        val granted = grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED
        android.util.Log.i("OpenLessBackendWarmupActivity", "POST_NOTIFICATIONS result granted=$granted")
    }

    override fun onResume() {
        super.onResume()
        activityResumed = true
        android.util.Log.i(
            "OpenLessBackendWarmupActivity",
            "onResume requestId=$settingsOpenRequestId settingsRequested=$settingsRequested " +
                "webView=${webViewRef?.let { System.identityHashCode(it) }}",
        )
        maybePerformPendingSettingsReattach("onResume")
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        android.util.Log.i(
            "OpenLessBackendWarmupActivity",
            "onWindowFocusChanged hasFocus=$hasFocus requestId=$settingsOpenRequestId",
        )
        if (hasFocus) maybePerformPendingSettingsReattach("onWindowFocusChanged")
    }

    override fun onPause() {
        super.onPause()
        activityResumed = false
        android.util.Log.i(
            "OpenLessBackendWarmupActivity",
            "onPause settingsRequested=$settingsRequested isChangingConfigurations=$isChangingConfigurations",
        )
    }

    override fun onStop() {
        super.onStop()
        android.util.Log.i(
            "OpenLessBackendWarmupActivity",
            "onStop settingsRequested=$settingsRequested isChangingConfigurations=$isChangingConfigurations isFinishing=$isFinishing",
        )
    }

    override fun onDestroy() {
        warmupHandler.removeCallbacks(sendToBackground)
        warmupHandler.removeCallbacks(webViewCreationWatchdog)
        if (activeInstance?.get() === this) {
            activeInstance = null
        }
        // onBackPressed() always backgrounds rather than finishing (see
        // above), and the old stuck-WebView recreate() escalation is gone
        // (see performWebViewReattach()'s doc comment) — but
        // webViewCreationWatchdog above now *does* call
        // finishAndRemoveTask() deliberately, for a narrower reason: this
        // instance's WebView silently never got created at all. That still
        // reports as "finishing" below, same as any other unexpected
        // isFinishing; it isn't split into its own category since it's
        // rare enough that OpenLessKeyboardSettingsActivity's restart-stats
        // table doesn't need a fourth bucket for it yet.
        val reason = when {
            isChangingConfigurations -> "config"
            isFinishing -> "finishing"
            else -> "os"
        }
        android.util.Log.w(
            "OpenLessBackendWarmupActivity",
            "onDestroy reason=$reason settingsRequested=$settingsRequested",
        )
        // Routed through the service rather than calling ensureBackendReady()
        // directly here: the service is what decides whether/when to
        // relaunch, this Activity dying is just one input to that decision.
        OpenLessRuntimeService.notifyRuntimeActivityDestroyed(applicationContext, reason)
        super.onDestroy()
    }

    companion object {
        @Volatile
        private var activeInstance: java.lang.ref.WeakReference<OpenLessBackendWarmupActivity>? = null

        private const val EXTRA_SHOW_SETTINGS = "com.openless.app.extra.SHOW_SETTINGS"
        // Same literal wry's WryActivity.kt uses for its own private
        // ACTIVITY_ID_KEY (top-level `private val`, file-scoped in Kotlin —
        // not visible here even though it's the same package, hence the
        // duplicated literal rather than a shared reference).
        private const val WRY_ACTIVITY_ID_KEY = "__wryActivityId"
        private const val REQUEST_POST_NOTIFICATIONS = 9102
        private const val WEBVIEW_READY_POLL_INTERVAL_MS = 60L
        private const val WEBVIEW_READY_MAX_WAIT_MS = 4000L
        // Successful onWebViewCreate() has always landed within ~300ms on
        // device even under load; this is generous headroom before treating
        // it as genuinely stuck rather than just slow.
        private const val WEBVIEW_CREATION_WATCHDOG_MS = 4000L

        private const val LAST_PROCESS_RESTART_KEY = "last_stuck_process_restart_wall_time"
        // Purely a crash-loop guard in case this same stuck state somehow
        // recurs immediately after relaunch — restartProcessAsLastResort()
        // itself has no way to know whether the relaunch actually fixed
        // anything, only that it happened.
        private const val PROCESS_RESTART_COOLDOWN_MS = 60_000L

        // Set synchronously by openSettings()/openSettingsIfRunning() BEFORE
        // startActivity() is ever called, and consumed by onCreate()/
        // onNewIntent() — a settings request is "in flight" the instant one
        // of those functions is called, not only once its Intent happens to
        // be delivered. Closes a race observed on-device: a concurrent,
        // unrelated ensureBackendReady() warmup can create/reuse this same
        // singleTask instance a few milliseconds earlier (e.g. right after
        // the OS killed the process in the background and the user's tap
        // triggers a cold start), and depending purely on whose Intent
        // reaches onCreate()/onNewIntent() first left the 180ms
        // sendToBackground() timer free to fire before the real settings
        // request ever got a chance to cancel it — the window would flash
        // and vanish instead of staying open.
        @Volatile
        private var settingsOpenPending = false

        /** The single Tauri host is usable only when its WebView still exists. */
        fun isRunning(): Boolean {
            val activity = activeInstance?.get() ?: return false
            return !activity.isFinishing && !activity.isDestroyed && activity.webViewRef != null
        }

        /** Bring the existing Tauri host forward instead of creating a black second host. */
        fun openSettingsIfRunning(context: Context): Boolean {
            val activity = activeInstance?.get() ?: return false
            if (activity.isFinishing || activity.isDestroyed || activity.webViewRef == null) return false
            android.util.Log.i("OpenLessBackendWarmupActivity", "openSettingsIfRunning: reusing live instance")
            settingsOpenPending = true
            // Also applied directly to the live instance right here, not
            // left to onNewIntent() delivery timing: this is the common
            // case (host already running) and the one most exposed to the
            // race described above, since the instance — and its pending
            // sendToBackground() timer — already exist by the time this runs.
            activity.settingsRequested = true
            activity.warmupHandler.removeCallbacks(activity.sendToBackground)
            context.startActivity(Intent(context, OpenLessBackendWarmupActivity::class.java).apply {
                putExtra(EXTRA_SHOW_SETTINGS, true)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
                addFlags(Intent.FLAG_ACTIVITY_NO_ANIMATION)
            })
            return true
        }

        /**
         * Show settings, reusing the existing Tauri host if one is alive. Only
         * starts a fresh Activity when none exists yet. Always targets this
         * class (never the bare MainActivity) so there is ever only one
         * tracked Tauri host, regardless of whether it was created for warmup
         * or for settings — starting MainActivity directly here would spin up
         * an untracked second host and re-run Tauri/Rust setup from scratch.
         */
        fun openSettings(context: Context) {
            settingsOpenPending = true
            if (openSettingsIfRunning(context)) return
            // No live instance (activeInstance is null, or the one we have is
            // finishing/destroyed/never got a WebView) — treat all of those
            // uniformly as "the previous host is gone, start fresh". A fresh
            // Activity's onCreate() now calls
            // OpenLessNative.nativeEnsureMainWebviewWindow() itself (see
            // above), which is what actually makes this reliable — no
            // separate finish-then-relaunch dance needed here anymore.
            android.util.Log.i("OpenLessBackendWarmupActivity", "openSettings: no live instance, starting fresh")
            context.startActivity(Intent(context, OpenLessBackendWarmupActivity::class.java).apply {
                putExtra(EXTRA_SHOW_SETTINGS, true)
                // Explicit __wryActivityId, matching what WryActivity's own
                // startActivity(cls) helper does — WITHOUT this, WryActivity.
                // onCreate()'s `intent.extras?.getInt(ACTIVITY_ID_KEY) ?: hashCode()`
                // silently resolves to 0 for *every* Intent here, because
                // Bundle.getInt() returns 0 (not null) for a missing key once
                // extras is non-null (which it always is once EXTRA_SHOW_SETTINGS
                // is set above) — so every rebuilt instance collided on the same
                // activity_id=0, racing its dying predecessor's async cleanup for
                // that same id and intermittently losing (confirmed via a wry
                // patch: android_setup()/InnerWebView::new() both logged
                // activity_id=0 for every fresh rebuild). A real random id per
                // launch makes each instance's Wry-side records genuinely its own.
                putExtra(WRY_ACTIVITY_ID_KEY, kotlin.random.Random.nextInt())
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                addFlags(Intent.FLAG_ACTIVITY_NO_ANIMATION)
            })
        }

        /** Compact live lifecycle snapshot shown in native keyboard settings. */
        fun debugSnapshot(): String {
            val activity = activeInstance?.get()
                ?: return "host=none activityContext=${nativeActivityContextState()}"
            val webView = activity.webViewRef
            return "host=present resumed=${activity.activityResumed} focus=${activity.hasWindowFocus()} " +
                "finishing=${activity.isFinishing} destroyed=${activity.isDestroyed} " +
                "webview=${webView != null} attached=${webView?.isAttachedToWindow ?: false} " +
                "visible=${webView?.windowVisibility == android.view.View.VISIBLE} " +
                "progress=${webView?.progress ?: -1} activityContext=${nativeActivityContextState()}"
        }

        private fun nativeActivityContextState(): String =
            runCatching { OpenLessNative.nativeHasRegisteredActivityContext().toString() }
                .getOrElse { "error" }

        private const val BACKEND_WARMUP_ATTEMPT_KEY = "backend_warmup_attempt_wall_time"
        private const val BACKEND_WARMUP_RETRY_DELAY_MS = 30_000L
        @Volatile
        private var lastWarmupAttemptElapsed = 0L

        // Snapshot of OpenLessImeService.isInputPanelCurrentlyShown() taken
        // right before this Activity steals the foreground (see the
        // postDelayed block below) — consumed once by sendToBackground() to
        // decide whether requestInputPanelAfterWarmup() is even worth
        // calling. Not reset here on a false read: sendToBackground() only
        // acts on it when true, and always clears it back to false itself
        // right after, so a stale true never lingers across warmup cycles.
        @Volatile
        private var restoreInputPanelAfterWarmup = false

        /**
         * Warms the Tauri/Rust backend if it isn't ready yet, from whatever
         * Context happens to notice first — not just the IME service reacting
         * to a focused text field. Called from OpenLessImeService.onCreate()
         * (the original path) and now also from OpenLessRuntimeService's
         * START_STICKY restart, so a system-triggered service restart (which
         * can happen before the user ever taps a field again) gets a chance to
         * finish this warmup — and the disruptive foreground-stealing
         * Activity launch it requires — before that tap happens, instead of
         * only ever reacting to it.
         *
         * The 30s persisted cooldown (SharedPreferences, survives a process
         * crash) is shared across every caller, so calling this from more
         * places never launches the warmup Activity more often than before —
         * it only widens the chance that one of those launches lands before
         * the user is looking at some other app's text field.
         */
        fun ensureBackendReady(context: Context) {
            val now = android.os.SystemClock.elapsedRealtime()
            if (now - lastWarmupAttemptElapsed < 5_000L) return
            if (isRunning()) return
            val backendError = try {
                OpenLessNative.requireBackendContract()
                null
            } catch (error: Throwable) {
                error
            }
            if (backendError != null) {
                launchWarmup(context, now, backendError)
                return
            }
            // A healthy Rust backend does not require a visible Activity.
            // In particular, do not turn a missing Activity Context into a
            // foreground Activity launch: that steals focus from Dialer,
            // Camera, Alipay, etc. The next explicit settings/launcher open
            // will register a fresh Activity Context when a UI is actually
            // needed.
            if (!OpenLessNative.nativeHasRegisteredActivityContext()) {
                android.util.Log.i(
                    "OpenLessBackendWarmupActivity",
                    "backend healthy but no Activity context; leaving UI closed",
                )
            }
        }

        private fun launchWarmup(context: Context, nowElapsed: Long, cause: Throwable) {
            val runtimePrefs = context.getSharedPreferences("openless_runtime", Context.MODE_PRIVATE)
            val wallNow = System.currentTimeMillis()
            val lastAttempt = runtimePrefs.getLong(BACKEND_WARMUP_ATTEMPT_KEY, 0L)
            if (wallNow >= lastAttempt && wallNow - lastAttempt < BACKEND_WARMUP_RETRY_DELAY_MS) return
            lastWarmupAttemptElapsed = nowElapsed
            runtimePrefs.edit().putLong(BACKEND_WARMUP_ATTEMPT_KEY, wallNow).apply()
            android.util.Log.i("OpenLessBackendWarmupActivity", "backend is not ready; launching warmup", cause)
            OpenLessProcessRestartStats(context, "warmup").recordStart()
            Handler(Looper.getMainLooper()).postDelayed({
                // Re-checked here, not just by the caller 120ms ago: something
                // else (a launcher-icon tap, a Logo tap) can have already
                // created/resumed the host in the meantime. Without this,
                // this launch still fires and delivers a no-extras Intent to
                // that same singleTask instance — harmless by itself, but it
                // was one of the two ingredients (together with imprecise
                // settingsRequested timing) behind an on-device black-screen
                // race: a genuine settings-open request and this silent
                // warmup landing within milliseconds of each other right
                // after the OS killed the process in the background.
                if (isRunning()) return@postDelayed
                // Snapshot taken right here, the last possible moment before
                // this launch steals the foreground — see
                // restoreInputPanelAfterWarmup's own doc comment.
                restoreInputPanelAfterWarmup = OpenLessImeService.isInputPanelCurrentlyShown()
                runCatching {
                    context.startActivity(Intent(context, OpenLessBackendWarmupActivity::class.java).apply {
                        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                        addFlags(Intent.FLAG_ACTIVITY_EXCLUDE_FROM_RECENTS)
                        addFlags(Intent.FLAG_ACTIVITY_NO_ANIMATION)
                    })
                }.onFailure { launchError ->
                    restoreInputPanelAfterWarmup = false
                    android.util.Log.w("OpenLessBackendWarmupActivity", "failed to launch warmup", launchError)
                }
            }, 120L)
        }

        /**
         * Last-resort recovery for the permanently-stuck "main" window
         * label state (see webViewCreationWatchdog's doc comment) — Tauri's
         * AppHandle.restart() is desktop-only (spawns a new process via
         * Command::new(current_exe), meaningless on Android, and would just
         * exit(0) the process with nothing to bring it back), so this does
         * the Android-native equivalent directly: schedule an alarm to
         * relaunch the launcher Activity a moment after this process is
         * gone, then kill the process outright. OpenLessRuntimeService's
         * dictation/IME state is lost same as any other process kill (the
         * same thing already happens whenever the OS reclaims this process
         * in the background) — the alternative is leaving the user stuck on
         * a black settings screen with no way back except doing this
         * exact same recovery manually.
         */
        /** @return true if the process was actually restarted, false if skipped (cooldown). */
        private fun restartProcessAsLastResort(context: Context): Boolean {
            val runtimePrefs = context.getSharedPreferences("openless_runtime", Context.MODE_PRIVATE)
            val wallNow = System.currentTimeMillis()
            val lastRestart = runtimePrefs.getLong(LAST_PROCESS_RESTART_KEY, 0L)
            if (wallNow >= lastRestart && wallNow - lastRestart < PROCESS_RESTART_COOLDOWN_MS) {
                android.util.Log.w(
                    "OpenLessBackendWarmupActivity",
                    "stuck main window label but process restart is on cooldown; finishing instance only",
                )
                return false
            }
            runtimePrefs.edit().putLong(LAST_PROCESS_RESTART_KEY, wallNow).apply()
            android.util.Log.e(
                "OpenLessBackendWarmupActivity",
                "main window label permanently stuck; restarting process",
            )
            OpenLessProcessRestartStats(context, "stuckwindow").recordStart()
            val launchIntent = context.packageManager.getLaunchIntentForPackage(context.packageName)
                ?.apply { addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP) }
            if (launchIntent != null) {
                val pendingIntent = android.app.PendingIntent.getActivity(
                    context,
                    0,
                    launchIntent,
                    android.app.PendingIntent.FLAG_IMMUTABLE or android.app.PendingIntent.FLAG_CANCEL_CURRENT,
                )
                // Plain set(), not setExactAndAllowWhileIdle(): API 31+ gates
                // exact alarms behind the SCHEDULE_EXACT_ALARM permission,
                // which this app doesn't otherwise need — relaunch timing
                // within a second or two either way is fine here.
                val alarmManager = context.getSystemService(Context.ALARM_SERVICE) as android.app.AlarmManager
                alarmManager.set(
                    android.app.AlarmManager.ELAPSED_REALTIME,
                    android.os.SystemClock.elapsedRealtime() + 500L,
                    pendingIntent,
                )
            } else {
                android.util.Log.e("OpenLessBackendWarmupActivity", "no launch intent found; process will not auto-relaunch")
            }
            android.os.Process.killProcess(android.os.Process.myPid())
            return true
        }
    }
}
