package vista.module

import org.junit.Assert.*
import org.junit.Test

class RemoteInputTest {
    private val token = InputToken(1, 2, 3)
    private val geometry = VideoGeometry.fit(400, 400, 1920, 1080, token)!!
    private val sent = mutableListOf<RemoteInput>()
    private val gesture = RemoteGesture { actual, events -> assertEquals(token, actual); sent += events }
    private fun down(x: Double = 200.0, y: Double = 200.0) = gesture.down(TouchPoint(x, y), geometry, token, 1.0)

    @Test fun letterboxPortraitLandscapeUltrawideAndNonfiniteCoordinatesAreRejected() {
        for ((w, h) in listOf(1080 to 1920, 1920 to 1080, 3440 to 1440, 800 to 600)) {
            val rect = VideoGeometry.fit(400, 400, w, h, token)!!
            assertEquals(SourcePoint(0.5, 0.5), rect.sourcePoint(200.0, 200.0, token))
            assertNull(rect.sourcePoint(rect.left - 1, 200.0, token))
            assertNull(rect.sourcePoint(200.0, rect.top - 1, token))
            assertNull(rect.sourcePoint(Double.NaN, 200.0, token))
            assertNull(rect.sourcePoint(200.0, Double.POSITIVE_INFINITY, token))
            assertNull(rect.sourcePoint(200.0, 200.0, token.copy(geometry = 4)))
        }
        assertNull(VideoGeometry.fit(0, 400, 1920, 1080, token))
        down(100.0, 10.0)
        gesture.up(TouchPoint(100.0, 10.0), token)
        assertTrue(sent.isEmpty())
    }
    @Test fun tapMovesBeforePressAndCannotClickUntilUp() {
        down()
        assertTrue(sent.isEmpty())
        gesture.up(TouchPoint(200.0, 200.0), token)
        assertEquals(listOf(RemoteInput.Move(SourcePoint(0.5, 0.5)), RemoteInput.Button(true), RemoteInput.Button(false)), sent)
    }
    @Test fun dragHoldsPrimaryAndReleasesWhenCancelled() {
        down()
        gesture.move(listOf(TouchPoint(240.0, 220.0)), token)
        assertTrue(sent[0] is RemoteInput.Move)
        assertEquals(RemoteInput.Button(true), sent[1])
        gesture.cancel()
        assertEquals(RemoteInput.Cancel, sent.last())
        sent.clear()
        gesture.up(TouchPoint(240.0, 220.0), token)
        assertTrue(sent.isEmpty())
    }
    @Test fun twoFingersScrollWithoutStrayClickAndAccumulateSmallMoves() {
        down()
        gesture.move(listOf(TouchPoint(180.0, 200.0), TouchPoint(220.0, 200.0)), token)
        for (i in 1..40) gesture.move(listOf(TouchPoint(180.0, 200.0-i), TouchPoint(220.0, 200.0-i)), token)
        gesture.up(TouchPoint(180.0, 160.0), token)
        assertFalse(sent.any { it is RemoteInput.Button })
        val scroll = sent.filterIsInstance<RemoteInput.Scroll>()
        assertEquals("begin", scroll.first().phase)
        assertEquals(1.0, scroll.filter { it.phase == "update" }.sumOf { it.dy }, 0.0)
        assertEquals("end", scroll.last().phase)
    }
    @Test fun longPressIsOneSecondaryClickAndPinchOrOldContextCannotClick() {
        down()
        assertTrue(gesture.longPress(token))
        gesture.up(TouchPoint(200.0, 200.0), token)
        assertEquals(listOf(RemoteInput.Button(true, "secondary"), RemoteInput.Button(false, "secondary")),
            sent.filterIsInstance<RemoteInput.Button>())
        sent.clear()
        down()
        gesture.move(listOf(TouchPoint(180.0, 200.0), TouchPoint(220.0, 200.0)), token)
        gesture.move(listOf(TouchPoint(100.0, 200.0), TouchPoint(300.0, 200.0)), token)
        gesture.up(TouchPoint(200.0, 200.0), token)
        assertFalse(sent.any { it is RemoteInput.Button })
        sent.clear()
        down()
        gesture.up(TouchPoint(200.0, 200.0), token.copy(selection = 99))
        assertEquals(listOf(RemoteInput.Cancel), sent) // Original token; transport rejects this stale cancellation.
    }
    @Test fun pendingAndScrollCancellationReleaseHeldModifiersWithoutClicking() {
        down()
        gesture.cancel()
        assertEquals(listOf(RemoteInput.Cancel), sent)
        sent.clear()
        down()
        gesture.move(listOf(TouchPoint(180.0, 200.0), TouchPoint(220.0, 200.0)), token)
        gesture.move(listOf(TouchPoint(180.0, 200.0), TouchPoint(220.0, 200.0), TouchPoint(240.0, 200.0)), token)
        assertEquals(RemoteInput.Cancel, sent.last())
        assertFalse(sent.any { it is RemoteInput.Button })
    }
    @Test fun queueGuardRejectsOldContextAndCancelClearsAllHeldInput() {
        val guard = InputGuard()
        assertTrue(guard.accept(RemoteInput.Key(0xe1, true), token, token))
        assertTrue(guard.accept(RemoteInput.Button(true), token, token))
        assertTrue(guard.holding)
        assertFalse(guard.accept(RemoteInput.Key(4, true), token, token.copy(ownership = 1)))
        assertFalse(guard.accept(RemoteInput.Text("late"), token, null))
        assertFalse(guard.accept(RemoteInput.Move(SourcePoint(Double.NaN, 0.5)), token, token))
        assertTrue(guard.accept(RemoteInput.Cancel, token, token))
        assertFalse(guard.holding)
        assertFalse(guard.accept(RemoteInput.Key(0xe1, false), token, token))
    }
    @Test fun wireUsesHostScrollAndCancelTypesAndBoundsUtf8Text() {
        val scroll = RemoteInput.Scroll(0.0, 2.0, "update", SourcePoint(0.5, 0.5)).wire(3)
        assertEquals("scroll", scroll.getString("type"))
        assertEquals("line", scroll.getString("unit"))
        assertEquals("input.cancel", RemoteInput.Cancel.wire(3).getString("type"))
        val guard = InputGuard()
        assertFalse(guard.accept(RemoteInput.Text("é".repeat(8193)), token, token))
        assertTrue(guard.accept(RemoteInput.Text("é".repeat(8192)), token, token))
    }
}
