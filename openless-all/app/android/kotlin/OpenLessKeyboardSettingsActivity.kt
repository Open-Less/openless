package com.openless.app

import android.app.Activity
import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.SeekBar
import android.widget.Switch
import android.widget.TextView
import android.widget.Toast
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

/**
 * Full-screen native settings window opened by long-pressing the OpenLess
 * logo in the keyboard panels. Keyboard-only preferences that don't need the
 * full WebView app live here. Framework only for now (vibration intensity
 * and duration) — more rows get appended to `content` in buildContent() as
 * they're added.
 */
class OpenLessKeyboardSettingsActivity : Activity() {
    private val prefs by lazy { getSharedPreferences("openless_ime_ui", Context.MODE_PRIVATE) }

    private val englishUi by lazy {
        val locale = prefs.getString("locale", null) ?: resources.configuration.locales[0].toLanguageTag()
        !locale.startsWith("zh", ignoreCase = true)
    }

    private fun ui(zh: String, en: String) = if (englishUi) en else zh
    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    /** Same "which theme is the keyboard actually showing" logic as OpenLessImeService.isDarkTheme, so this screen matches whatever the user is looking at when they long-press the Logo to get here. */
    private val isDarkTheme: Boolean
        get() = when (prefs.getString("theme_mode", null)) {
            "light" -> false
            "dark" -> true
            else -> (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) != Configuration.UI_MODE_NIGHT_NO
        }

    private fun tone(dark: Int, light: Int): Int = if (isDarkTheme) dark else light

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Explicit rather than relying on targetSdk 36's implicit
        // edge-to-edge enforcement — guarantees the IME-inset listener in
        // buildContent() actually fires so the "云笔记提交" URL/Token fields
        // stay reachable once the keyboard covers part of the screen.
        WindowCompat.setDecorFitsSystemWindows(window, false)
        setContentView(buildContent())
        // Some OEM ROMs only wire up the WindowInsets/IME dispatch chain
        // correctly once a WindowInsetsControllerCompat has actually been
        // instantiated for this window — never used for show()/hide() here,
        // just created as the standard companion call to
        // setDecorFitsSystemWindows(false) above.
        WindowCompat.getInsetsController(window, window.decorView)
    }

    private fun saveMigrationSetting(write: () -> Unit) {
        if (!OpenLessMigrationState.settings(write)) {
            Toast.makeText(this, ui("正在恢复配置，请稍后重试", "Configuration restore is in progress; try again shortly"), Toast.LENGTH_SHORT).show()
            setContentView(buildContent())
        }
    }

    private fun buildContent(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(tone(Color.rgb(30, 30, 30), Color.rgb(245, 245, 247)))
        }
        val header = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, dp(8), dp(16), dp(8))
        }
        header.addView(
            TextView(this).apply {
                text = "←"
                textSize = 22f
                setTextColor(tone(Color.WHITE, Color.rgb(30, 30, 34)))
                gravity = Gravity.CENTER
                setPadding(dp(16), dp(8), dp(16), dp(8))
                isClickable = true
                setOnClickListener { finish() }
            },
        )
        header.addView(
            TextView(this).apply {
                text = ui("键盘设置", "Keyboard settings")
                textSize = 18f
                setTypeface(typeface, android.graphics.Typeface.BOLD)
                setTextColor(tone(Color.WHITE, Color.rgb(30, 30, 34)))
            },
        )
        root.addView(header, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        val scroll = ScrollView(this)
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(12), dp(20), dp(20))
        }
        scroll.addView(content, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        root.addView(scroll, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))

        // Height/raise are adjusted in their own bottom sheet (see
        // showKeyboardSizeDialog()), not docked under this form: the 1:1
        // footprint is as tall as the keyboard itself, so keeping it on this
        // page permanently left everything above it only a sliver to scroll
        // in. This row just names the current values and opens the sheet.
        content.addView(sectionLabel(ui("键盘外观", "Keyboard appearance")))
        val sizeSummary = TextView(this).apply {
            text = keyboardSizeSummary()
            textSize = 14f
            setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(110, 110, 115)))
        }
        val sizeRow = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, dp(6), 0, dp(6))
            isClickable = true
            setOnClickListener { showKeyboardSizeDialog { sizeSummary.text = keyboardSizeSummary() } }
        }
        sizeRow.addView(
            TextView(this).apply {
                text = ui("键盘高度", "Keyboard height")
                textSize = 15f
                setTextColor(tone(Color.rgb(220, 220, 220), Color.rgb(40, 40, 44)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        sizeRow.addView(sizeSummary)
        sizeRow.addView(
            TextView(this).apply {
                text = "›"
                textSize = 18f
                setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(110, 110, 115)))
                setPadding(dp(8), 0, 0, 0)
            },
        )
        content.addView(
            sizeRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            },
        )

        // Paired with onCreate()'s setDecorFitsSystemWindows(false): now that
        // the window draws edge-to-edge, this restores the padding the
        // system used to apply automatically (status bar / nav bar / cutouts)
        // AND — the actual point of going edge-to-edge here — adds the IME's
        // own height as bottom padding whenever it's taller than the nav bar,
        // so `scroll` (layout_weight=1) genuinely loses that much height
        // while the keyboard is up — targetSdk 36 otherwise neutralizes the
        // manifest's windowSoftInputMode="adjustResize" (the window no
        // longer physically shrinks on its own), which is exactly why the
        // 云笔记提交 URL/Token fields were unreachable once the keyboard
        // covered them. Shrinking the viewport alone isn't enough on its
        // own, though: if a field already had focus before the keyboard
        // finished animating in, nothing re-triggers ScrollView's normal
        // "bring the focused child into view" behavior on a pure padding
        // change (that behavior only fires at the moment focus is first
        // requested) — so once the inset is actually nonzero, explicitly
        // scroll whatever's currently focused into the new, smaller
        // viewport instead of leaving it wherever it happened to sit before.
        ViewCompat.setOnApplyWindowInsetsListener(root) { view, insets ->
            val systemBars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            val imeBottom = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
            view.setPadding(systemBars.left, systemBars.top, systemBars.right, maxOf(systemBars.bottom, imeBottom))
            if (imeBottom > 0) {
                val focused = view.findFocus()
                if (focused != null) {
                    scroll.post {
                        val rect = android.graphics.Rect()
                        focused.getDrawingRect(rect)
                        content.offsetDescendantRectToMyCoords(focused, rect)
                        scroll.smoothScrollTo(0, rect.bottom - scroll.height + dp(16))
                    }
                }
            }
            insets
        }

        // Gates the "进程重启统计（今天）" section further down (see its own
        // guard) — off by default, since those counters are only meaningful
        // for diagnosing a specific problem, not everyday reading. Read once
        // here, up top, rather than re-reading prefs at the exact point it's
        // used, so this row and the section it controls can never disagree
        // within a single render of this page.
        content.addView(sectionLabel(ui("主设置", "Main settings")))
        val debugFeaturesEnabled = prefs.getBoolean("key_debug_features_enabled", false)
        val debugRow = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        debugRow.addView(
            TextView(this).apply {
                text = ui("调试功能", "Debug features")
                textSize = 15f
                setTextColor(tone(Color.rgb(220, 220, 220), Color.rgb(40, 40, 44)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        debugRow.addView(
            Switch(this).apply {
                isChecked = debugFeaturesEnabled
                setOnCheckedChangeListener { _, checked ->
                    saveMigrationSetting { prefs.edit().putBoolean("key_debug_features_enabled", checked).apply() }
                    // The restart-stats section's own visibility is decided
                    // once, above, when this page was built — rebuild it so
                    // toggling here shows/hides it immediately instead of
                    // only taking effect the next time this page opens.
                    setContentView(buildContent())
                }
            },
        )
        content.addView(
            debugRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            },
        )

        content.addView(sectionLabel(ui("震动反馈", "Haptic feedback")))

        val enabledRow = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        enabledRow.addView(
            TextView(this).apply {
                text = ui("按键震动", "Key vibration")
                textSize = 15f
                setTextColor(tone(Color.rgb(220, 220, 220), Color.rgb(40, 40, 44)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        enabledRow.addView(
            Switch(this).apply {
                isChecked = prefs.getBoolean("key_haptic_enabled", true)
                setOnCheckedChangeListener { _, checked -> saveMigrationSetting { prefs.edit().putBoolean("key_haptic_enabled", checked).apply() } }
            },
        )
        content.addView(
            enabledRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            },
        )

        content.addView(sectionLabel(ui("拼音云候选", "Cloud Pinyin candidates")))
        content.addView(Switch(this).apply {
            text = ui("Google 云候选（发送当前拼音；密码和隐私输入禁用）", "Google cloud candidates (sends Pinyin; disabled in private fields)")
            isChecked = prefs.getBoolean("cloud_pinyin_enabled", false)
            setOnCheckedChangeListener { _, checked -> prefs.edit().putBoolean("cloud_pinyin_enabled", checked).apply() }
        })
        content.addView(Switch(this).apply {
            text = ui("每周更新公共词库（学习词仅保存在本机）", "Weekly public dictionaries (learned words stay on this device)")
            isChecked = prefs.getBoolean("public_dictionary_updates", true)
            setOnCheckedChangeListener { _, checked -> prefs.edit().putBoolean("public_dictionary_updates", checked).apply() }
        })
        content.addView(
            sectionLabel(ui("模糊音", "Fuzzy pinyin")),
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                topMargin = dp(14)
            },
        )
        val fuzzy = RimeFuzzyPinyin.selected(prefs).toMutableSet()
        for (group in RimeFuzzyPinyin.groups) {
            content.addView(Switch(this).apply {
                text = ui(group.labelZh, group.labelEn)
                isChecked = group.id in fuzzy
                setOnCheckedChangeListener { _, checked ->
                    if (checked) fuzzy += group.id else fuzzy -= group.id
                    RimeFuzzyPinyin.save(prefs, fuzzy)
                    scheduleFuzzyRedeploy()
                }
            })
        }
        content.addView(View(this), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(14)))
        content.addView(sectionLabel(ui("英文键盘", "English keyboard")))
        val englishSuggestionsRow = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        englishSuggestionsRow.addView(
            TextView(this).apply {
                text = ui("英文单词提示", "English word suggestions")
                textSize = 15f
                setTextColor(tone(Color.rgb(220, 220, 220), Color.rgb(40, 40, 44)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        englishSuggestionsRow.addView(
            Switch(this).apply {
                isChecked = prefs.getBoolean("english_suggestions_enabled", true)
                setOnCheckedChangeListener { _, checked -> saveMigrationSetting { prefs.edit().putBoolean("english_suggestions_enabled", checked).apply() } }
            },
        )
        content.addView(
            englishSuggestionsRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            },
        )

        // Amplitude's 255 ceiling is Android's own VibrationEffect max, not a
        // choice made here — the hardware/API can't go any stronger than
        // that regardless of what this slider allows. Duration's ceiling
        // started at 500ms, halved to 250, then halved again to 125 so the
        // same slider width covers a quarter of the original range, for
        // finer-grained adjustment.
        var currentAmplitude = prefs.getInt("key_haptic_amplitude", 55).coerceIn(1, 255)
        var currentDurationMs = prefs.getLong("key_haptic_duration_ms", 12L).toInt().coerceIn(1, 125)
        // No separate test button — letting go of either slider fires one
        // vibration with the values as they now stand, so adjusting and
        // feeling the result is a single motion.
        content.addView(
            sliderRow(
                label = ui("震动强度（系统上限）", "Intensity (platform ceiling)"),
                min = 1,
                max = 255,
                current = currentAmplitude,
                onChange = { value ->
                    currentAmplitude = value
                    saveMigrationSetting { prefs.edit().putInt("key_haptic_amplitude", value).apply() }
                },
                onRelease = { fireTestVibration(currentAmplitude, currentDurationMs) },
            ),
        )
        content.addView(
            sliderRow(
                label = ui("震动时长", "Duration"),
                min = 1,
                max = 125,
                current = currentDurationMs,
                onChange = { value ->
                    currentDurationMs = value
                    saveMigrationSetting { prefs.edit().putLong("key_haptic_duration_ms", value.toLong()).apply() }
                },
                onRelease = { fireTestVibration(currentAmplitude, currentDurationMs) },
            ),
        )

        content.addView(sectionLabel(ui("后台运行", "Background")))
        // OEM background-task killers (observed on-device: this app's own
        // settings Activity gets reclaimed by the system 16+ times/day even
        // with a foreground service running) largely ignore that
        // protection but do respect the standard "ignore battery
        // optimizations" exemption — offering a direct link to it here is
        // the most effective single thing a user can do about the restart
        // counts below. isIgnoringBatteryOptimizations() re-reads live each
        // time this screen builds, so returning here after granting it in
        // system settings shows the up-to-date state without extra wiring.
        val powerManager = getSystemService(Context.POWER_SERVICE) as android.os.PowerManager
        if (powerManager.isIgnoringBatteryOptimizations(packageName)) {
            content.addView(
                TextView(this).apply {
                    text = ui("已加入电池优化白名单", "Already exempt from battery optimization")
                    textSize = 13f
                    setTextColor(tone(Color.rgb(134, 239, 172), Color.rgb(21, 128, 61)))
                },
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                    bottomMargin = dp(14)
                },
            )
        } else {
            content.addView(
                TextView(this).apply {
                    text = ui(
                        "系统电量管理可能频繁回收键盘的后台进程，导致设置页偶尔黑屏或响应变慢。加入电池优化白名单可以减少这种情况——效果因系统而异。",
                        "The system's battery manager may repeatedly reclaim the keyboard's background process, occasionally causing a black settings screen or slow responses. Exempting it from battery optimization can reduce this — effectiveness varies by device.",
                    )
                    textSize = 13f
                    setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
                },
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                    bottomMargin = dp(8)
                },
            )
            content.addView(
                TextView(this).apply {
                    text = ui("去允许后台活动 →", "Allow background activity →")
                    textSize = 15f
                    setTypeface(typeface, android.graphics.Typeface.BOLD)
                    setTextColor(tone(Color.rgb(94, 234, 212), Color.rgb(15, 118, 110)))
                    isClickable = true
                    setOnClickListener {
                        val direct = android.content.Intent(android.provider.Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
                            data = android.net.Uri.parse("package:$packageName")
                        }
                        val fallback = android.content.Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS).apply {
                            data = android.net.Uri.parse("package:$packageName")
                        }
                        runCatching { startActivity(direct) }
                            // A handful of heavily customized OEM systems
                            // block or silently no-op this specific system
                            // intent — the general app-details screen at
                            // least lands the user in the right area, one
                            // tap further from the actual toggle, instead
                            // of nothing happening on tap.
                            .onFailure { runCatching { startActivity(fallback) } }
                    }
                },
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                    bottomMargin = dp(14)
                },
            )
        }

        val monospace = android.graphics.Typeface.MONOSPACE
        // Diagnostic-only — hidden unless "调试功能" above is on (see that
        // switch's own comment). These counters help debug a specific
        // problem; they're noise for everyday reading otherwise.
        if (debugFeaturesEnabled) {
            content.addView(sectionLabel(ui("进程重启统计（今天）", "Process restarts (today)")))
            // Short, purposefully un-translated keys (not meant to be pretty —
            // meant to be pasted into a screenshot and read back verbatim).
            // All reset to 0 whenever OpenLessBuildInfo.VERSION changes (see
            // OpenLessApplication.resetRestartStatsOnVersionBump()), so these
            // are always "since this build was installed", not lifetime totals:
            //   main/access  - raw restarts of the main / :accessibility process
            //   sticky       - OpenLessRuntimeService.onStartCommand() got a
            //                  null Intent: Android's own restart-after-death
            //                  signal for a START_STICKY service, the strongest
            //                  evidence the whole process was actually killed
            //   warmup       - OpenLessBackendWarmupActivity.ensureBackendReady()
            //                  found the backend not registered and launched
            //                  the warmup Activity
            //   mictap       - user tapped the mic and toggleDictation() found
            //                  the backend not ready (the user-visible symptom)
            //   actkill      - OpenLessBackendWarmupActivity.onDestroy() fired,
            //                  total across all reasons below (doesn't
            //                  necessarily mean the process itself died)
            //   actkill_config - onDestroy() from a configuration change
            //                    (rotation/density/locale) — expected, harmless
            //   actkill_finishing - isFinishing was true (unexpected; this
            //                       Activity never calls finish() on itself
            //                       deliberately)
            //   actkill_os     - none of the above: the only sub-category that
            //                    is actually the system reclaiming this task
            //   rtexit       - Tauri's RunEvent::Exit actually fired despite
            //                  ExitRequested being prevented (see
            //                  mobile_runtime.rs) — should stay at 0 if that fix
            //                  is holding
            //   unclean      - previous main-process session never reached
            //                  OpenLessImeService.onDestroy() (best-effort
            //                  crash/force-stop signal, can't tell those apart)
            // Chinese gloss for each key — just enough to read at a glance
            // without cross-referencing the doc comment above.
            val restartCategories = listOf(
                Triple(OpenLessProcessRestartStats.MAIN, "main", "主进程"),
                Triple(OpenLessProcessRestartStats.ACCESSIBILITY, "access", "无障碍进程"),
                Triple("sticky", "sticky", "系统杀后恢复"),
                Triple("warmup", "warmup", "后端唤醒"),
                Triple("mictap", "mictap", "点击时未就绪"),
                Triple("actkill", "actkill", "界面被回收(合计)"),
                Triple("actkill_config", "  ├config", "· 配置变化(无害)"),
                Triple("actkill_finishing", "  ├finish", "· isFinishing(异常)"),
                Triple("actkill_os", "  └os", "· 真正被系统回收"),
                Triple("rtexit", "rtexit", "后端异常退出"),
                Triple("unclean", "unclean", "上次异常退出"),
                Triple("heartbeat", "heartbeat", "心跳自愈"),
                Triple("stuckwindow", "stuckwindow", "设置窗口卡死自重启"),
            )
            for ((key, label, gloss) in restartCategories) {
                content.addView(
                    TextView(this).apply {
                        val count = OpenLessProcessRestartStats(this@OpenLessKeyboardSettingsActivity, key).today()
                        text = label.padEnd(10) + count.toString().padEnd(4) + gloss
                        textSize = 13f
                        typeface = monospace
                        setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
                    },
                )
            }
            content.addView(View(this), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(14)))
        }

        // "个人偏好数据": read-only counters for the two local, on-device-only
        // learning stores that make repeated input steadily rank better
        // (笔画/拼音 both benefit — see each row's own comment). No toggle
        // here — matches this section's existing convention of pure
        // display; the underlying preferences (strokeUsageEnabled etc.)
        // live in the app's own WebView settings, not this native page.
        content.addView(sectionLabel(ui("个人偏好数据", "Personal preference data")))
        // 笔画输入的个人调频数据：记录"这个笔画码你选过哪个字"，越用越靠前
        // 排序，不影响词库本身，也不会同步到云端。联想学习单独一个存储、单独
        // 计上限，两者不再互相挤占。
        val strokePicks = StrokeUserFrequency.strokeCodes(this)
        val associationPicks = StrokeUserFrequency.associations(this)
        for ((labelZh, labelEn, store) in listOf(
            Triple("笔画调频", "Stroke ranking", strokePicks),
            Triple("联想学习", "Association learning", associationPicks),
        )) {
            content.addView(
                TextView(this).apply {
                    text = ui(
                        "$labelZh：已记录 ${store.size()} / ${store.capacity()} 条",
                        "$labelEn: ${store.size()} / ${store.capacity()} entries recorded",
                    )
                    textSize = 14f
                    setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
                },
            )
        }
        // 拼音的选词调频和造词由 Rime 的用户词典完成；这里只读条数。
        pinyinDictionaryText = TextView(this).apply {
            text = ui("拼音用户词典：读取中…", "Pinyin user dictionary: loading…")
            textSize = 14f
            setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
        }
        content.addView(pinyinDictionaryText)
        refreshPinyinDictionaryCount()
        // 键盘自己的学习数据（上面三项）单独导出 / 导入，和 App 设置无关：
        // 设置走云同步，这里不进迁移文件，也不参与加密同步。
        val dataActions = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        dataActions.addView(dictionaryAction(ui("导出键盘数据 →", "Export keyboard data →")) { showKeyboardDataExportChoices() })
        dataActions.addView(dictionaryAction(ui("导入键盘数据 →", "Import keyboard data →")) { pickKeyboardDataImportSource() })
        content.addView(dataActions)
        content.addView(
            TextView(this).apply {
                text = ui(
                    "可选择导出本页的键盘设置（高度、震动等）、拼音用户词典、笔画调频、联想学习，以及云笔记地址 / Token（明文）。不含 App 设置（设置通过云同步）。文件里有你学过的词，请妥善保管。导入时可选择部分：设置会被覆盖，学习数据与本机合并（同一条取较高的次数），不会删除现有内容。",
                    "Choose what to export: this page's keyboard settings (size, haptics…), the pinyin user dictionary, stroke ranking, association learning, and the cloud notes URL / token (plaintext). App settings are not included; cloud sync carries them. The file holds words you have learned; keep it private. On import, pick the parts: settings are replaced, learning data merges into this device's (the higher count wins) and nothing is removed.",
                )
                textSize = 12f
                setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(110, 110, 115)))
                setPadding(0, 0, 0, dp(14))
            },
        )

        // 话筒右划进入"云笔记"：录音经 LLM 整理后的文字（跟正常上屏用的是
        // 同一条润色流程——见 OpenLessImeService.handleImeTextReady()）会以
        // JSON POST 到这里配置的地址，不插入任何输入框，也不在本机留存。
        // 两项都填了才会真的提交——留空时只会在状态栏提示去设置里补上，不会
        // 静默失败。
        content.addView(sectionLabel(ui("云笔记提交", "Cloud notes webhook")))
        content.addView(
            TextView(this).apply {
                text = ui(
                    "话筒右划进入「云笔记」模式：录音经过整理后的文字会提交到下面的地址，不插入输入框，也不保存在本机。",
                    "Swipe the mic right to enter Cloud notes mode: the polished transcript is POSTed to the address below instead of being inserted — nothing is kept on this device either.",
                )
                textSize = 12f
                setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(110, 110, 115)))
                setPadding(0, 0, 0, dp(10))
            },
        )
        content.addView(
            textFieldRow(
                label = ui("提交地址", "Submit URL"),
                hint = "https://example.com/capture_ingest.php",
                initial = prefs.getString("key_cloud_note_webhook_url", "") ?: "",
                onChange = { prefs.edit().putString("key_cloud_note_webhook_url", it).apply() },
            ),
        )
        content.addView(
            textFieldRow(
                label = "Token",
                hint = ui("输入法专用 Token", "IME-only token"),
                initial = prefs.getString("key_cloud_note_webhook_token", "") ?: "",
                isSecret = true,
                onChange = { prefs.edit().putString("key_cloud_note_webhook_token", it).apply() },
            ),
        )

        // "语音识别来源": how many dictations in history the cloud and the local
        // model each recognized (with the local standby racing the cloud
        // either can win), the latest one's kind highlighted.
        content.addView(sectionLabel(ui("语音识别来源", "Speech recognition source")))
        val asrUsageText = TextView(this).apply {
            text = ui("读取中…", "Loading…")
            textSize = 14f
            setTextColor(tone(Color.rgb(220, 220, 220), Color.rgb(50, 50, 55)))
        }
        content.addView(asrUsageText)
        Thread {
            val usage = runCatching { OpenLessNative.nativeAsrUsage() }.getOrNull()
            runOnUiThread { asrUsageText.text = asrUsageSummary(usage) }
        }.apply { name = "openless-asr-usage" }.start()

        // build_first_seen_wall_time is written by OpenLessApplication's
        // resetRestartStatsOnVersionBump() at the exact moment it last
        // zeroed the restart-cause counters below — i.e. "counting since
        // when" for whatever counts are on screen right now, so a
        // screenshot of this page carries both together.
        val buildFirstSeenAt = getSharedPreferences("openless_runtime", Context.MODE_PRIVATE)
            .getLong("build_first_seen_wall_time", 0L)
        val installedAtText = if (buildFirstSeenAt > 0L) {
            android.text.format.DateFormat.format("yyyy-MM-dd HH:mm", buildFirstSeenAt)
        } else {
            "?"
        }
        content.addView(
            TextView(this).apply {
                text = "${ui("构建版本", "Build")} ${OpenLessBuildInfo.VERSION}  ${ui("安装于", "installed")} $installedAtText"
                textSize = 11f
                typeface = monospace
                setTextColor(tone(Color.rgb(120, 120, 120), Color.rgb(150, 150, 155)))
            },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                topMargin = dp(16)
            },
        )

        return root
    }

    private var pinyinDictionaryText: TextView? = null

    private fun dictionaryAction(text: String, onClick: () -> Unit): View = TextView(this).apply {
        this.text = text
        textSize = 15f
        setTypeface(typeface, android.graphics.Typeface.BOLD)
        setTextColor(tone(Color.rgb(94, 234, 212), Color.rgb(15, 118, 110)))
        isClickable = true
        setPadding(0, dp(8), dp(20), dp(8))
        setOnClickListener { onClick() }
    }

    private fun toast(zh: String, en: String) {
        Toast.makeText(applicationContext, ui(zh, en), Toast.LENGTH_LONG).show()
    }

    /** Applies the saved fuzzy-pinyin groups; toggles made while one redeploy runs are folded into the next. */
    private fun scheduleFuzzyRedeploy() {
        if (!fuzzyRedeployPending.compareAndSet(false, true)) return
        toast("正在更新拼音方案…", "Updating the pinyin schema…")
        val app = applicationContext
        rimeWorker.execute {
            fuzzyRedeployPending.set(false)
            val result = runCatching { OpenLessRime.applyFuzzyPinyin(app, RimeFuzzyPinyin.selected(prefs)) }
            runOnUiThread {
                if (result.isSuccess) toast("模糊音已更新", "Fuzzy pinyin updated")
                else toast("模糊音更新失败：${result.exceptionOrNull()?.message}", "Fuzzy pinyin update failed: ${result.exceptionOrNull()?.message}")
            }
        }
    }

    /** Exports to a scratch file and counts it — the only way to read the dictionary while Rime owns it. */
    private fun refreshPinyinDictionaryCount() {
        val app = applicationContext
        rimeWorker.execute {
            val count = runCatching {
                val scratch = java.io.File(app.cacheDir, "count-pinyin_simp.userdb.txt")
                try {
                    if (OpenLessRime.exportUserDictionary(app, scratch)) RimeUserDictionaryFile.entryCount(scratch.readText()) else 0
                } finally { scratch.delete() }
            }.getOrNull()
            runOnUiThread {
                pinyinDictionaryText?.text = if (count == null) ui("拼音用户词典：读取失败", "Pinyin user dictionary: unavailable")
                else ui("拼音用户词典：$count 条", "Pinyin user dictionary: $count entries")
            }
        }
    }

    private var pendingExport: Set<KeyboardDataFile.Section> = KeyboardDataFile.defaultExport

    /** A checklist of [sections], [preset] checked; [onConfirm] gets the checked ones (never called with none). */
    private fun chooseSections(
        title: String,
        sections: List<KeyboardDataFile.Section>,
        preset: Set<KeyboardDataFile.Section>,
        label: (KeyboardDataFile.Section) -> String,
        action: String,
        onConfirm: (Set<KeyboardDataFile.Section>) -> Unit,
    ) {
        val checked = BooleanArray(sections.size) { sections[it] in preset }
        android.app.AlertDialog.Builder(this)
            .setTitle(title)
            .setMultiChoiceItems(sections.map(label).toTypedArray(), checked) { _, index, isChecked -> checked[index] = isChecked }
            .setPositiveButton(action) { _, _ ->
                val chosen = sections.filterIndexed { index, _ -> checked[index] }.toSet()
                if (chosen.isNotEmpty()) onConfirm(chosen)
            }
            .setNegativeButton(ui("取消", "Cancel"), null)
            .show()
    }

    private fun showKeyboardDataExportChoices() {
        chooseSections(
            ui("选择要导出的内容", "Choose what to export"),
            KeyboardDataFile.Section.entries,
            KeyboardDataFile.defaultExport,
            { ui(it.labelZh, it.labelEn) },
            ui("下一步", "Next"),
        ) { pendingExport = it; pickKeyboardDataExportTarget() }
    }

    private fun pickKeyboardDataExportTarget() {
        val timestamp = android.text.format.DateFormat.format("yyyyMMdd-HHmm", System.currentTimeMillis())
        val intent = android.content.Intent(android.content.Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(android.content.Intent.CATEGORY_OPENABLE)
            type = "application/json"
            putExtra(android.content.Intent.EXTRA_TITLE, "openless-keyboard-data-$timestamp.json")
        }
        runCatching { startActivityForResult(intent, REQUEST_KEYBOARD_DATA_EXPORT) }
            .onFailure { toast("找不到文件管理器", "No file manager available") }
    }

    private fun pickKeyboardDataImportSource() {
        val intent = android.content.Intent(android.content.Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(android.content.Intent.CATEGORY_OPENABLE)
            // Shared files often lose their JSON MIME type on the way.
            type = "*/*"
        }
        runCatching { startActivityForResult(intent, REQUEST_KEYBOARD_DATA_IMPORT) }
            .onFailure { toast("找不到文件管理器", "No file manager available") }
    }

    /** Lists the parts [contents] actually holds, with counts, all but the token checked. */
    private fun showKeyboardDataImportChoices(contents: KeyboardDataFile.Contents) {
        val sections = contents.present()
        if (sections.isEmpty()) {
            toast("文件里没有可导入的数据", "The file holds nothing to import")
            return
        }
        chooseSections(
            ui("选择要导入的内容", "Choose what to import"),
            sections,
            KeyboardDataFile.defaultImport,
            { ui("${it.labelZh}（${contents.count(it)} 项）", "${it.labelEn} (${contents.count(it)})") },
            ui("导入", "Import"),
        ) { chosen ->
            val app = applicationContext
            rimeWorker.execute {
                val result = runCatching { KeyboardDataFile.import(app, contents, chosen) }
                runOnUiThread {
                    result.onSuccess { toast("已导入：${chosen.joinToString("、") { it.labelZh }}", "Imported: ${chosen.joinToString(", ") { it.labelEn }}") }
                        .onFailure { toast("导入失败：${it.message}", "Import failed: ${it.message}") }
                    if (!isFinishing && !isDestroyed) setContentView(buildContent())
                }
            }
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: android.content.Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        val uri = data?.data?.toString()
        if (resultCode != RESULT_OK || uri == null) return
        val app = applicationContext
        when (requestCode) {
            REQUEST_KEYBOARD_DATA_EXPORT -> rimeWorker.execute {
                val result = runCatching {
                    val json = KeyboardDataFile.export(app, pendingExport)
                    check(OpenLessContentWriter.writeBytes(app, uri, json.toByteArray(Charsets.UTF_8))) { "write failed" }
                }
                runOnUiThread {
                    result.onSuccess { toast("键盘数据已导出", "Keyboard data exported") }
                        .onFailure { toast("导出失败：${it.message}", "Export failed: ${it.message}") }
                    refreshPinyinDictionaryCount()
                }
            }
            REQUEST_KEYBOARD_DATA_IMPORT -> rimeWorker.execute {
                val result = runCatching {
                    val bytes = OpenLessContentReader.readBytes(app, uri, KeyboardDataFile.MAX_BYTES)
                        ?: error(ui("读取失败或文件超过 48 MiB", "Read failed or file exceeds 48 MiB"))
                    runCatching { KeyboardDataFile.read(String(bytes, Charsets.UTF_8)) }
                        .getOrElse { error(ui("不是 OpenLess 键盘数据文件", "Not an OpenLess keyboard data file")) }
                }
                runOnUiThread {
                    result.onSuccess { if (!isFinishing && !isDestroyed) showKeyboardDataImportChoices(it) }
                        .onFailure { toast("导入失败：${it.message}", "Import failed: ${it.message}") }
                }
            }
        }
    }

    private companion object {
        const val REQUEST_KEYBOARD_DATA_EXPORT = 7101
        const val REQUEST_KEYBOARD_DATA_IMPORT = 7102
        // Outlives the Activity so a redeploy or import finishes even if the page closes; also serializes them.
        val rimeWorker: java.util.concurrent.ExecutorService = java.util.concurrent.Executors.newSingleThreadExecutor()
        val fuzzyRedeployPending = java.util.concurrent.atomic.AtomicBoolean(false)
    }

    /** Fires a one-shot vibration with the sliders' current (already-saved) values, so a change is felt immediately. */
    private fun fireTestVibration(amplitude: Int, durationMs: Int) {
        runCatching {
            val vibrator = if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.S) {
                (getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as android.os.VibratorManager).defaultVibrator
            } else {
                @Suppress("DEPRECATION")
                getSystemService(Context.VIBRATOR_SERVICE) as android.os.Vibrator
            }
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.O) {
                vibrator.vibrate(android.os.VibrationEffect.createOneShot(durationMs.toLong(), amplitude))
            } else {
                @Suppress("DEPRECATION")
                vibrator.vibrate(durationMs.toLong())
            }
        }
    }

    /** "云 N · 本 M" from nativeAsrUsage()'s "<local>,<cloud>,<latest>", the latest kind bold and colored. */
    private fun asrUsageSummary(usage: String?): CharSequence {
        val parts = usage?.split(',') ?: return ui("后台服务未就绪", "Backend not ready")
        val local = parts.getOrNull(0)?.toIntOrNull() ?: 0
        val cloud = parts.getOrNull(1)?.toIntOrNull() ?: 0
        if (local + cloud == 0) return ui("暂无听写记录", "No dictations yet")
        val cloudText = ui("云 $cloud", "Cloud $cloud")
        val localText = ui("本 $local", "Local $local")
        val text = android.text.SpannableString("$cloudText · $localText")
        val (start, end, color) = when (parts.getOrNull(2)) {
            "cloud" -> Triple(0, cloudText.length, tone(Color.rgb(96, 165, 250), Color.rgb(37, 99, 235)))
            "local" -> Triple(text.length - localText.length, text.length, tone(Color.rgb(74, 222, 128), Color.rgb(21, 128, 61)))
            else -> return text
        }
        text.setSpan(android.text.style.ForegroundColorSpan(color), start, end, android.text.Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        text.setSpan(android.text.style.StyleSpan(android.graphics.Typeface.BOLD), start, end, android.text.Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        return text
    }

    private fun sectionLabel(text: String): View = TextView(this).apply {
        this.text = text
        textSize = 12f
        setTextColor(tone(Color.rgb(150, 150, 150), Color.rgb(110, 110, 115)))
        setPadding(0, 0, 0, dp(8))
    }

    private fun keyboardSizeSummary(): String {
        val heightDp = OpenLessImeService.panelHeightDp(this)
        val raiseDp = OpenLessImeService.raiseHeightDp(this)
        return ui("按键区 ${heightDp}dp · 抬高 ${raiseDp}dp", "Keys ${heightDp}dp · raise ${raiseDp}dp")
    }

    /**
     * Bottom sheet for the key-area height and raise: two sliders over a
     * 1:1 footprint that sits flush with the bottom of the screen — exactly
     * where the real keyboard will. Values save as they change, same as
     * every other row on this page; onClosed lets the caller refresh
     * whatever it shows of them.
     */
    private fun showKeyboardSizeDialog(onClosed: () -> Unit) {
        var heightDp = OpenLessImeService.panelHeightDp(this)
        var raiseDp = OpenLessImeService.raiseHeightDp(this)
        val footprint = buildKeyboardFootprintPreview()
        fun save() {
            prefs.edit()
                .putInt(OpenLessImeService.PREF_KEYBOARD_HEIGHT_DP, heightDp)
                .putInt(OpenLessImeService.PREF_KEYBOARD_RAISE_DP, raiseDp)
                .apply()
            footprint.setSizes(heightDp, raiseDp)
        }
        val heightSlider = steppedSliderRow(
            label = ui("按键区高度", "Key area height"),
            min = OpenLessImeService.MIN_KEYBOARD_HEIGHT_DP,
            max = OpenLessImeService.MAX_KEYBOARD_HEIGHT_DP,
            current = heightDp,
            onChange = { value ->
                heightDp = value
                save()
            },
        )
        val raiseSlider = steppedSliderRow(
            label = ui("底部抬高", "Raise from bottom"),
            min = OpenLessImeService.MIN_KEYBOARD_RAISE_DP,
            max = OpenLessImeService.MAX_KEYBOARD_RAISE_DP,
            current = raiseDp,
            onChange = { value ->
                raiseDp = value
                save()
            },
        )

        val dialog = android.app.Dialog(this)
        val linkColor = tone(Color.rgb(94, 234, 212), Color.rgb(15, 118, 110))
        val titleRow = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, 0, 0, dp(12))
        }
        titleRow.addView(
            TextView(this).apply {
                text = ui("键盘高度", "Keyboard height")
                textSize = 17f
                setTypeface(typeface, android.graphics.Typeface.BOLD)
                setTextColor(tone(Color.WHITE, Color.rgb(30, 30, 34)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        titleRow.addView(
            TextView(this).apply {
                text = ui("恢复默认", "Reset")
                textSize = 14f
                setTextColor(linkColor)
                setPadding(dp(10), dp(6), dp(10), dp(6))
                isClickable = true
                setOnClickListener {
                    heightSlider.set(OpenLessImeService.DEFAULT_KEYBOARD_HEIGHT_DP)
                    raiseSlider.set(OpenLessImeService.DEFAULT_KEYBOARD_RAISE_DP)
                }
            },
        )
        titleRow.addView(
            TextView(this).apply {
                text = ui("完成", "Done")
                textSize = 14f
                setTypeface(typeface, android.graphics.Typeface.BOLD)
                setTextColor(linkColor)
                setPadding(dp(10), dp(6), 0, dp(6))
                isClickable = true
                setOnClickListener { dialog.dismiss() }
            },
        )
        val controls = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(8), dp(20), dp(2))
            background = android.graphics.drawable.GradientDrawable().apply {
                setColor(tone(Color.rgb(30, 30, 30), Color.rgb(245, 245, 247)))
                val radius = dp(16).toFloat()
                cornerRadii = floatArrayOf(radius, radius, radius, radius, 0f, 0f, 0f, 0f)
            }
            addView(
                View(this@OpenLessKeyboardSettingsActivity).apply {
                    background = android.graphics.drawable.GradientDrawable().apply {
                        setColor(tone(Color.rgb(90, 90, 90), Color.rgb(200, 200, 205)))
                        cornerRadius = dp(2).toFloat()
                    }
                },
                LinearLayout.LayoutParams(dp(36), dp(4)).apply {
                    gravity = Gravity.CENTER_HORIZONTAL
                    bottomMargin = dp(10)
                },
            )
            addView(titleRow)
            addView(heightSlider.view)
            addView(raiseSlider.view)
        }
        val sheet = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(controls, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            addView(footprint.root, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        footprint.setSizes(heightDp, raiseDp)

        dialog.requestWindowFeature(android.view.Window.FEATURE_NO_TITLE)
        dialog.setContentView(sheet)
        dialog.setOnDismissListener { onClosed() }
        dialog.window?.let { window ->
            // The stock dialog background carries its own inset and rounded
            // card; clearing it is what lets the sheet span the full width
            // and sit flush with the bottom edge.
            window.setBackgroundDrawable(android.graphics.drawable.ColorDrawable(Color.TRANSPARENT))
            window.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            window.setGravity(Gravity.BOTTOM)
            window.setWindowAnimations(android.R.style.Animation_InputMethod)
        }
        dialog.show()
    }

    private class SteppedSlider(val view: View, val set: (Int) -> Unit)

    /**
     * Slider row for the keyboard-size sheet: the value reads as a plain
     * "300 dp" on the right, and −/+ buttons give the single-dp nudges a
     * drag can't hit reliably. `set` moves the slider from code (恢复默认)
     * and reports through onChange exactly like a drag would.
     */
    private fun steppedSliderRow(label: String, min: Int, max: Int, current: Int, onChange: (Int) -> Unit): SteppedSlider {
        var value = current.coerceIn(min, max)
        val textColor = tone(Color.rgb(220, 220, 220), Color.rgb(40, 40, 44))
        val valueView = TextView(this).apply {
            text = "$value dp"
            textSize = 15f
            setTextColor(textColor)
        }
        val seekBar = SeekBar(this)
        seekBar.max = (max - min).coerceAtLeast(1)
        seekBar.progress = value - min
        fun update(next: Int, fromSeekBar: Boolean) {
            val clamped = next.coerceIn(min, max)
            if (clamped == value) return
            value = clamped
            valueView.text = "$value dp"
            if (!fromSeekBar) seekBar.progress = value - min
            onChange(value)
        }
        seekBar.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
            override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                if (fromUser) update(progress + min, fromSeekBar = true)
            }
            override fun onStartTrackingTouch(seekBar: SeekBar?) = Unit
            override fun onStopTrackingTouch(seekBar: SeekBar?) = Unit
        })
        fun stepButton(symbol: String, delta: Int): View = TextView(this).apply {
            text = symbol
            textSize = 18f
            gravity = Gravity.CENTER
            setTextColor(textColor)
            background = android.graphics.drawable.GradientDrawable().apply {
                shape = android.graphics.drawable.GradientDrawable.OVAL
                setColor(tone(Color.rgb(60, 60, 60), Color.rgb(225, 225, 228)))
            }
            isClickable = true
            setOnClickListener { update(value + delta, fromSeekBar = false) }
        }
        val header = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        header.addView(
            TextView(this).apply {
                text = label
                textSize = 14f
                setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        header.addView(valueView)
        val controls = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, dp(6), 0, 0)
        }
        controls.addView(stepButton("−", -1), LinearLayout.LayoutParams(dp(34), dp(34)))
        controls.addView(seekBar, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        controls.addView(stepButton("+", 1), LinearLayout.LayoutParams(dp(34), dp(34)))
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            }
            addView(header)
            addView(controls)
        }
        return SteppedSlider(row, set = { update(it, fromSeekBar = false) })
    }

    /**
     * Full-width 1:1 IME footprint at the bottom of the keyboard-size sheet.
     * Key-area block uses the stretch height; raise strip is empty lift
     * space below keys.
     */
    private fun buildKeyboardFootprintPreview(): KeyboardFootprintPreview {
        val caption = TextView(this).apply {
            textSize = 11f
            gravity = Gravity.CENTER
            setTextColor(tone(Color.rgb(160, 160, 160), Color.rgb(100, 100, 105)))
            setPadding(0, dp(6), 0, dp(4))
        }
        val keysBlock = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setBackgroundColor(tone(Color.rgb(48, 48, 48), Color.rgb(242, 242, 246)))
            setPadding(dp(12), dp(10), dp(12), dp(10))
            // Three fake key rows so stretch is visually obvious.
            repeat(3) { rowIndex ->
                val row = LinearLayout(this@OpenLessKeyboardSettingsActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = Gravity.CENTER
                }
                repeat(10) {
                    row.addView(
                        View(this@OpenLessKeyboardSettingsActivity).apply {
                            setBackgroundColor(tone(Color.rgb(70, 70, 70), Color.rgb(255, 255, 255)))
                        },
                        LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f).apply {
                            marginStart = dp(2)
                            marginEnd = dp(2)
                        },
                    )
                }
                addView(
                    row,
                    LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f).apply {
                        if (rowIndex > 0) topMargin = dp(6)
                    },
                )
            }
        }
        val raiseBlock = FrameLayout(this).apply {
            setBackgroundColor(tone(Color.rgb(36, 36, 36), Color.rgb(220, 220, 224)))
            addView(
                TextView(this@OpenLessKeyboardSettingsActivity).apply {
                    text = ui("抬高", "Raise")
                    textSize = 11f
                    gravity = Gravity.CENTER
                    setTextColor(tone(Color.rgb(140, 140, 140), Color.rgb(120, 120, 125)))
                },
                FrameLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT,
                ),
            )
        }
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(tone(Color.rgb(22, 22, 22), Color.rgb(230, 230, 234)))
            addView(caption, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            addView(keysBlock, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(OpenLessImeService.DEFAULT_KEYBOARD_HEIGHT_DP)))
            addView(raiseBlock, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0))
        }
        return KeyboardFootprintPreview(root, caption, keysBlock, raiseBlock)
    }

    private inner class KeyboardFootprintPreview(
        val root: LinearLayout,
        private val caption: TextView,
        private val keysBlock: View,
        private val raiseBlock: View,
    ) {
        fun setSizes(heightDp: Int, raiseDp: Int) {
            keysBlock.layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                dp(heightDp),
            )
            raiseBlock.layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                dp(raiseDp),
            )
            raiseBlock.visibility = if (raiseDp > 0) View.VISIBLE else View.GONE
            caption.text = ui(
                "1:1 预览 · 键盘共占屏幕底部 ${heightDp + raiseDp}dp",
                "1:1 preview · keyboard takes the bottom ${heightDp + raiseDp}dp",
            )
            root.requestLayout()
        }
    }

    /** One labeled slider row. Reusable as more settings rows get added here. */
    private fun sliderRow(label: String, min: Int, max: Int, current: Int, onChange: (Int) -> Unit, onRelease: (() -> Unit)? = null): View {
        // Computed before building the SeekBar itself, since inside that
        // view's own apply{} block an unqualified "max" would resolve to
        // SeekBar's own max property (shadowing this function's max: Int
        // parameter), not the value intended here.
        val range = (max - min).coerceAtLeast(1)
        val initialProgress = (current - min).coerceIn(0, range)
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            }
        }
        val labelView = TextView(this).apply {
            text = "$label · Max $max Set:$current"
            textSize = 14f
            setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
        }
        row.addView(labelView)
        row.addView(
            SeekBar(this).apply {
                this.max = range
                this.progress = initialProgress
                setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                    override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                        if (fromUser) {
                            val value = progress + min
                            labelView.text = "$label · Max $max Set:$value"
                            onChange(value)
                        }
                    }
                    override fun onStartTrackingTouch(seekBar: SeekBar?) = Unit
                    override fun onStopTrackingTouch(seekBar: SeekBar?) {
                        onRelease?.invoke()
                    }
                })
            },
        )
        return row
    }

    /** One labeled single-line text field, auto-saving on every keystroke (matches every other row on this page — no separate save button). Used by the "云笔记提交" section for its URL/token; reusable for any future free-text setting. */
    private fun textFieldRow(label: String, hint: String, initial: String, isSecret: Boolean = false, onChange: (String) -> Unit): View {
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            }
        }
        row.addView(
            TextView(this).apply {
                text = label
                textSize = 14f
                setTextColor(tone(Color.rgb(200, 200, 200), Color.rgb(70, 70, 75)))
                setPadding(0, 0, 0, dp(4))
            },
        )
        row.addView(
            EditText(this).apply {
                setText(initial)
                this.hint = hint
                textSize = 14f
                isSingleLine = true
                if (isSecret) {
                    inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD
                }
                setTextColor(tone(Color.WHITE, Color.rgb(30, 30, 34)))
                setHintTextColor(tone(Color.rgb(110, 110, 115), Color.rgb(170, 170, 175)))
                addTextChangedListener(object : android.text.TextWatcher {
                    override fun afterTextChanged(s: android.text.Editable?) = onChange(s?.toString().orEmpty())
                    override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
                    override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit
                })
            },
        )
        return row
    }
}
