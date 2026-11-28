package com.openless.app

import android.content.Context
import android.content.ContextWrapper
import android.content.SharedPreferences
import android.graphics.drawable.ColorDrawable
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Stable root, bounded draft geometry, and real Service presentation without onCreate/JNI. */
@RunWith(AndroidJUnit4::class)
class ImeSurfaceHostInstrumentedTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private fun onMain(block: () -> Unit) = instrumentation.runOnMainSync(block)
    private fun dp(value: Int) = (value * context.resources.displayMetrics.density).toInt()

    private class PreferredView(context: Context, private val preferred: Int) : View(context) {
        override fun onMeasure(width: Int, height: Int) {
            val bound = MeasureSpec.getSize(height)
            val actual = when (MeasureSpec.getMode(height)) {
                MeasureSpec.EXACTLY -> bound
                MeasureSpec.AT_MOST -> minOf(bound, preferred)
                else -> preferred
            }
            setMeasuredDimension(MeasureSpec.getSize(width), actual)
        }
    }

    private fun measure(host: View, height: Int, mode: Int = View.MeasureSpec.AT_MOST) {
        host.measure(View.MeasureSpec.makeMeasureSpec(dp(360), View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(height, mode))
        host.layout(0, 0, host.measuredWidth, host.measuredHeight)
    }

    @Test fun presentationChangesKeepTheSameRoot() {
        var stable = false
        var oldDetached = false
        onMain {
            val host = ImeSurfaceHost(context)
            val first = PreferredView(context, dp(300))
            host.render(first, null)
            measure(host, dp(900))
            val root = host
            val second = PreferredView(context, dp(300))
            host.render(second, null)
            measure(host, dp(900))
            stable = root === host && second.parent === host
            oldDetached = first.parent == null
        }
        assertTrue(stable)
        assertTrue(oldDetached)
    }

    @Test fun exactAcknowledgementDoesNotRepeatedlyShrinkTheKeyboard() {
        var before = 0
        var after = 0
        onMain {
            val host = ImeSurfaceHost(context)
            host.render(PreferredView(context, dp(300)), null)
            measure(host, dp(900))
            before = host.measuredHeight
            repeat(4) { measure(host, host.measuredHeight, View.MeasureSpec.EXACTLY) }
            after = host.measuredHeight
        }
        assertEquals(dp(300), before)
        assertEquals(before, after)
    }

    @Test fun constrainedDraftScrollsAboveUsableKeysWithoutOverlap() {
        var bounded = false
        var noOverlap = false
        var retained = false
        onMain {
            val host = ImeSurfaceHost(context)
            val keys = PreferredView(context, dp(300))
            val draft = PreferredView(context, dp(500))
            host.render(keys, draft)
            measure(host, dp(360))
            val scroll = host.findViewWithTag<View>("ime-draft-scroll")
            bounded = host.measuredHeight <= dp(360) && keys.measuredHeight >= dp(220)
            noOverlap = keys.top >= scroll.bottom && keys.bottom <= host.measuredHeight
            retained = draft.parent === scroll && draft.measuredHeight >= scroll.measuredHeight
        }
        assertTrue(bounded)
        assertTrue(noOverlap)
        assertTrue(retained)
    }

    @Test fun closingDraftRemovesItsChromeButKeepsKeysAndRoot() {
        var gone = false
        var keysRemain = false
        onMain {
            val host = ImeSurfaceHost(context)
            val keys = PreferredView(context, dp(300))
            host.render(keys, PreferredView(context, dp(140)))
            measure(host, dp(900))
            host.render(keys, null)
            measure(host, dp(900))
            gone = host.findViewWithTag<View>("ime-draft-scroll").visibility == View.GONE
            keysRemain = keys.parent === host && keys.top == 0 && host.measuredHeight == dp(300)
        }
        assertTrue(gone)
        assertTrue(keysRemain)
    }

    private class IsolatedContext(base: Context) : ContextWrapper(base) {
        override fun getSharedPreferences(name: String?, mode: Int): SharedPreferences =
            super.getSharedPreferences("ime_surface_test__${name ?: "null"}", mode)
    }

    private fun newService(): OpenLessImeService {
        val isolated = IsolatedContext(context)
        isolated.getSharedPreferences("openless_ime_ui", 0).edit()
            .clear().putBoolean("key_haptic_enabled", false).putInt("keyboard_height_dp", 300)
            .putString("theme_mode", "dark").commit()
        val service = OpenLessImeService()
        ContextWrapper::class.java.getDeclaredMethod("attachBaseContext", Context::class.java)
            .apply { isAccessible = true }.invoke(service, isolated)
        return service
    }

    private fun field(service: OpenLessImeService, name: String): Any? =
        OpenLessImeService::class.java.getDeclaredField(name).apply { isAccessible = true }.get(service)
    private fun setField(service: OpenLessImeService, name: String, value: Any?) {
        OpenLessImeService::class.java.getDeclaredField(name).apply { isAccessible = true }.set(service, value)
    }

    @Test fun serviceThemeRefreshDoesNotReplaceTheFrameworkRoot() {
        var same = false
        var changed = false
        onMain {
            val service = newService()
            val root = service.onCreateInputView()
            val first = field(service, "keyboardSurface") as ViewGroup
            service.getSharedPreferences("openless_ime_ui", 0).edit().putString("theme_mode", "light").commit()
            service.refreshInputView()
            val second = field(service, "keyboardSurface") as ViewGroup
            same = field(service, "surfaceHost") === root && second.parent === root
            changed = first !== second &&
                (second.getChildAt(0).background as? ColorDrawable)?.color == ImePalette.resolve(false).surface
        }
        assertTrue(same)
        assertTrue(changed)
    }

    @Test fun serviceDraftOpenClosePreservesRootAndEditorContent() {
        var same = false
        var content = ""
        var start = -1
        var end = -1
        var draftGone = false
        onMain {
            val service = newService()
            val root = service.onCreateInputView()
            val editor = EditText(context).apply { setText("unchanged draft"); setSelection(2, 7) }
            setField(service, "draftEdit", editor)
            setField(service, "draftToken", 1L)
            service.refreshInputView()
            content = editor.text.toString()
            start = editor.selectionStart
            end = editor.selectionEnd
            same = field(service, "surfaceHost") === root
            // Use only the presentation path; never submit text to any host or backend.
            setField(service, "draftEdit", null)
            setField(service, "draftToken", 2L)
            service.refreshInputView()
            draftGone = root.findViewWithTag<View>("ime-draft-scroll").visibility == View.GONE
            same = same && field(service, "surfaceHost") === root
        }
        assertTrue(same)
        assertEquals("unchanged draft", content)
        assertEquals(2, start)
        assertEquals(7, end)
        assertTrue(draftGone)
    }

    @Test fun previewUsesActualTypingKeysAndSuppressesFakeAccessibilityActions() {
        var actualKeys = 0
        var hidden = false
        onMain {
            val preview = ImeKeyboardPreview(context, ImePalette.resolve(false), false)
            fun count(view: View) {
                if ((view.tag as? String)?.startsWith("ime-key-") == true) actualKeys++
                if (view is ViewGroup) for (i in 0 until view.childCount) count(view.getChildAt(i))
            }
            count(preview)
            hidden = preview.importantForAccessibility == View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        }
        assertEquals(ImeKeyboardLayouts.rows(ImeKeyboardLayer.LETTERS, ImeTypingLanguage.PINYIN, ImeLetterCase.LOWER, "换行").sumOf { it.size }, actualKeys)
        assertTrue(hidden)
    }

    @Test fun previewTouchDoesNotPressInteractiveDescendants() {
        var consumed = false
        var pressed = true
        onMain {
            val preview = ImeKeyboardPreview(context, ImePalette.resolve(false), false)
            measure(preview, dp(300), View.MeasureSpec.EXACTLY)
            val key = preview.findViewWithTag<View>("ime-key-letters_r1_c1")
            val rect = android.graphics.Rect(0, 0, key.width, key.height)
            preview.offsetDescendantRectToMyCoords(key, rect)
            val now = android.os.SystemClock.uptimeMillis()
            for (action in listOf(android.view.MotionEvent.ACTION_DOWN, android.view.MotionEvent.ACTION_UP)) {
                val event = android.view.MotionEvent.obtain(now, now, action, rect.exactCenterX(), rect.exactCenterY(), 0)
                consumed = preview.dispatchTouchEvent(event)
                event.recycle()
            }
            pressed = key.isPressed
        }
        assertTrue(consumed)
        assertFalse(pressed)
    }

    @Test fun previewFitsAllSupportedHeightChoices() {
        val heights = mutableListOf<Int>()
        onMain {
            val preview = ImeKeyboardPreview(context, ImePalette.resolve(true), true)
            for (height in listOf(220, 300, 420)) {
                measure(preview, dp(height), View.MeasureSpec.EXACTLY)
                heights += preview.measuredHeight
            }
        }
        assertEquals(listOf(dp(220), dp(300), dp(420)), heights)
    }
}
