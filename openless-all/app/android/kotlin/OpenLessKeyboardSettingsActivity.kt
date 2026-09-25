package com.openless.app

import android.app.Activity
import android.content.Context
import android.graphics.Color
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.SeekBar
import android.widget.Switch
import android.widget.TextView

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

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildContent())
    }

    private fun buildContent(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.rgb(30, 30, 30))
        }

        val header = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, dp(8), dp(16), dp(8))
        }
        header.addView(
            TextView(this).apply {
                text = "←"
                textSize = 22f
                setTextColor(Color.WHITE)
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
                setTextColor(Color.WHITE)
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

        content.addView(sectionLabel(ui("震动反馈", "Haptic feedback")))

        val enabledRow = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL }
        enabledRow.addView(
            TextView(this).apply {
                text = ui("按键震动", "Key vibration")
                textSize = 15f
                setTextColor(Color.rgb(220, 220, 220))
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
        )
        enabledRow.addView(
            Switch(this).apply {
                isChecked = prefs.getBoolean("key_haptic_enabled", true)
                setOnCheckedChangeListener { _, checked -> prefs.edit().putBoolean("key_haptic_enabled", checked).apply() }
            },
        )
        content.addView(
            enabledRow,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                bottomMargin = dp(14)
            },
        )

        // Amplitude's 255 ceiling is Android's own VibrationEffect max, not a
        // choice made here — the hardware/API can't go any stronger than
        // that regardless of what this slider allows. Duration's ceiling is
        // ours, so it's the one raised for a more noticeable pulse.
        var currentAmplitude = prefs.getInt("key_haptic_amplitude", 55).coerceIn(1, 255)
        var currentDurationMs = prefs.getLong("key_haptic_duration_ms", 12L).toInt().coerceIn(1, 500)
        // No separate test button — letting go of either slider fires one
        // vibration with the values as they now stand, so adjusting and
        // feeling the result is a single motion.
        content.addView(
            sliderRow(
                label = ui("震动强度（255 已是系统上限）", "Intensity (255 is the platform max)"),
                min = 1,
                max = 255,
                current = currentAmplitude,
                onChange = { value ->
                    currentAmplitude = value
                    prefs.edit().putInt("key_haptic_amplitude", value).apply()
                },
                onRelease = { fireTestVibration(currentAmplitude, currentDurationMs) },
            ),
        )
        content.addView(
            sliderRow(
                label = ui("震动时长", "Duration"),
                min = 1,
                max = 500,
                current = currentDurationMs,
                onChange = { value ->
                    currentDurationMs = value
                    prefs.edit().putLong("key_haptic_duration_ms", value.toLong()).apply()
                },
                onRelease = { fireTestVibration(currentAmplitude, currentDurationMs) },
            ),
        )

        content.addView(sectionLabel(ui("个人偏好数据", "Personal preference data")))
        val personalFrequency = StrokeUserFrequency(this)
        content.addView(
            TextView(this).apply {
                text = ui(
                    "已记录 ${personalFrequency.size()} / ${personalFrequency.capacity()} 条",
                    "${personalFrequency.size()} / ${personalFrequency.capacity()} entries recorded",
                )
                textSize = 14f
                setTextColor(Color.rgb(200, 200, 200))
            },
        )

        return root
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

    private fun sectionLabel(text: String): View = TextView(this).apply {
        this.text = text
        textSize = 12f
        setTextColor(Color.rgb(150, 150, 150))
        setPadding(0, 0, 0, dp(8))
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
        row.addView(
            TextView(this).apply {
                text = label
                textSize = 14f
                setTextColor(Color.rgb(200, 200, 200))
            },
        )
        row.addView(
            SeekBar(this).apply {
                this.max = range
                this.progress = initialProgress
                setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                    override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                        if (fromUser) onChange(progress + min)
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
}
