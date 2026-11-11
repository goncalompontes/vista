package vista.module

import android.graphics.Point
import android.os.SystemClock
import android.view.MotionEvent
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.Configurator
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.semantics.SemanticsActions
import org.junit.runner.RunWith
import java.io.File

/** Opt-in real-host UI/input exercise. The host invitation stays in a file on
 * the test emulator, never in command-line arguments, snapshots or test logs.
 * Assert desktop effects separately in the disposable target's event log.
 */
@RunWith(AndroidJUnit4::class)
class LiveHostSmokeTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation).also { Configurator.getInstance().waitForIdleTimeout = 0 }
    @get:Rule val compose = createEmptyComposeRule()
    private fun SemanticsNodeInteraction.activate() {
        assertIsEnabled()
        performSemanticsAction(SemanticsActions.OnClick) { click -> assertTrue(click()) }
    }
    private fun find(text: String): SemanticsNodeInteraction {
        compose.waitUntil(30_000) { compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
        return compose.onNodeWithText(text)
    }
    private fun source(description: String): SemanticsNodeInteraction {
        compose.waitUntil(30_000) { compose.onAllNodesWithContentDescription(description).fetchSemanticsNodes().isNotEmpty() }
        return compose.onNodeWithContentDescription(description)
    }
    private fun live() {
        find("Live · View only").assertIsDisplayed()
        // Let the replacement native surface and Compose overlay settle, then
        // recheck. A transient old label cannot satisfy a recovery assertion.
        SystemClock.sleep(700)
        find("Live · View only").assertIsDisplayed()
        compose.waitUntil(10_000) { visiblePicture() }
    }
    private fun visiblePicture(): Boolean {
        if (android.os.Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
        val bounds = device.findObject(By.descStartsWith("Remote picture."))?.visibleBounds ?: return false
        val bitmap = instrumentation.uiAutomation.takeScreenshot() ?: return false
        try {
            var colored = 0
            var samples = 0
            // Skip the top status label and bars; a rendered black surface
            // cannot pass just because the Compose overlay says Live.
            for (y in bounds.top + bounds.height() / 4 until bounds.bottom - bounds.height() / 4 step 16) {
                for (x in bounds.left + bounds.width() / 4 until bounds.right - bounds.width() / 4 step 16) {
                    if (x !in 0 until bitmap.width || y !in 0 until bitmap.height) continue
                    val pixel = bitmap.getPixel(x, y)
                    if (android.graphics.Color.red(pixel) + android.graphics.Color.green(pixel) + android.graphics.Color.blue(pixel) > 45) colored++
                    samples++
                }
            }
            return samples > 0 && colored > samples / 4
        } finally { bitmap.recycle() }
    }
    private fun screenshot(name: String) {
        compose.waitForIdle()
        SystemClock.sleep(100)
        val directory = instrumentation.targetContext.getExternalFilesDir("verification")!!
        directory.mkdirs()
        assertTrue(device.takeScreenshot(File(directory, "$name.png")))
    }
    private fun settleRotation() {
        // UiDevice reports display rotation before Compose has replaced the
        // old layout/surface. Do not click coordinates from that old layout.
        SystemClock.sleep(500)
        instrumentation.waitForIdleSync()
        if (android.os.Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
    }

    @Test fun pairingLayoutAndValidation() {
        ActivityScenario.launch(MainActivity::class.java).use {
            find("Connect to host")
            find("Connect to host").assertIsNotEnabled()
            screenshot("pairing-portrait")
            device.setOrientationLeft()
            settleRotation()
            find("Pair with a host")
            screenshot("pairing-landscape")
            device.setOrientationNatural()
            settleRotation()
        }
    }

    @Test fun realHostInputLifecycleAndRotation() {
        val arguments = InstrumentationRegistry.getArguments()
        assumeTrue("Opt in with -e live true and -e invitation <emulator file>", arguments.getString("live") == "true")
        val invite = JSONObject(File(requireNotNull(arguments.getString("invitation"))).readText())
        val windowId = arguments.getString("window")?.toLong()
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            device.setOrientationNatural()
            settleRotation()
            find("Connect to host")
            find("Host address").performTextReplacement("10.0.2.2")
            find("Port").performTextReplacement("7443")
            find("Pairing secret").performTextReplacement(invite.getString("secret_hex"))
            find("Connect to host").performScrollTo().activate()
            find("Choose your workspace")
            screenshot("library")
            val source = if (windowId == null) "Open Desktop" else "Open Vista input verification"
            source(source).activate()
            live()
            screenshot("stream-view-only")
            if (arguments.getString("input") != "false") {
                source("Enable host control").activate()
                find("Live · Control enabled").assertIsDisplayed()
                compose.waitForIdle()
                if (android.os.Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
                val picture = requireNotNull(device.findObject(By.descStartsWith("Remote picture."))).visibleBounds
                val center = Point(picture.centerX(), picture.centerY())
                // One-finger drag, then long press/secondary click, all inside video.
                device.swipe(center.x - 80, center.y, center.x + 80, center.y + 10, 30)
                device.swipe(center.x, center.y, center.x, center.y, 160)
                twoFingerScroll(center)
                device.pressKeyCode(android.view.KeyEvent.KEYCODE_SHIFT_LEFT)
                screenshot("stream-control")
                // Hold a modifier before backgrounding, then verify it is
                // released promptly in the native target log.
                find("Shift").activate()
                scenario.moveToState(androidx.lifecycle.Lifecycle.State.CREATED)
                scenario.moveToState(androidx.lifecycle.Lifecycle.State.RESUMED)
                live()
                find("View only").assertIsDisplayed()
                source("Enable host control").activate()
                find("Live · Control enabled").assertIsDisplayed()
                find("Keyboard").activate()
                find("Text for host").performTextReplacement("vista mvp verified 20261111")
                find("Send to host").activate()
                find("Done").activate()
                // Wait for the local dialog/window transition before using
                // any coordinates from the stream controls again.
                compose.waitUntil(10_000) { compose.onAllNodesWithText("Send text").fetchSemanticsNodes().isEmpty() }
            }
            scenario.moveToState(androidx.lifecycle.Lifecycle.State.CREATED)
            scenario.moveToState(androidx.lifecycle.Lifecycle.State.RESUMED)
            live()
            find("View only").assertIsDisplayed()
            device.setOrientationLeft()
            settleRotation()
            live()
            screenshot("stream-landscape")
            device.setOrientationNatural()
            settleRotation()
            live()
            screenshot("stream-portrait")
            find("Sources").activate()
            find("Choose your workspace")
            source("Open Desktop").activate()
            live()
            screenshot("stream-switched-desktop")
            find("Sources").activate()
            find("Choose your workspace")
            source(source).activate()
            live()
            if (arguments.getString("restart") == "true") {
                File(instrumentation.targetContext.filesDir, "restart-request").writeText("restart")
                compose.waitUntil(30_000) { compose.onAllNodes(hasText("Retrying", substring = true)).fetchSemanticsNodes().isNotEmpty() }
                live()
                screenshot("stream-reconnected")
            }
            find("Disconnect").activate()
            find("Connect to host")
        }
    }

    private fun twoFingerScroll(center: Point) {
        val downTime = SystemClock.uptimeMillis()
        fun event(action: Int, offset: Float, count: Int = 2) {
            val properties = Array(count) { index -> MotionEvent.PointerProperties().apply { id = index; toolType = MotionEvent.TOOL_TYPE_FINGER } }
            val coords = Array(count) { index -> MotionEvent.PointerCoords().apply {
                x = center.x + if (index == 0) -35f else 35f
                y = center.y - offset; pressure = 1f; size = 1f
            } }
            val motion = MotionEvent.obtain(downTime, SystemClock.uptimeMillis(), action, count, properties, coords,
                0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_TOUCHSCREEN, 0)
            assertTrue(instrumentation.uiAutomation.injectInputEvent(motion, true))
            motion.recycle()
            SystemClock.sleep(20)
        }
        event(MotionEvent.ACTION_DOWN, 0f, 1)
        event(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 0f)
        for (step in 1..30) event(MotionEvent.ACTION_MOVE, step * 4f)
        event(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 120f)
        event(MotionEvent.ACTION_UP, 120f, 1)
    }
}
