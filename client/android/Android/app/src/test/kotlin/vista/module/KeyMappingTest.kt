package vista.module

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class KeyMappingTest {
    @Test
    fun mapsLettersAndDigits() {
        assertEquals(0x04, KeyMapping.hidUsage(29)) // A
        assertEquals(0x1d, KeyMapping.hidUsage(54)) // Z
        assertEquals(0x1e, KeyMapping.hidUsage(8)) // 1
        assertEquals(0x25, KeyMapping.hidUsage(15)) // 8
        assertEquals(0x27, KeyMapping.hidUsage(7)) // 0
    }

    @Test
    fun mapsEditingAndModifiers() {
        assertEquals(0x28, KeyMapping.hidUsage(66)) // Enter
        assertEquals(0x29, KeyMapping.hidUsage(111)) // Escape
        assertEquals(0x2a, KeyMapping.hidUsage(67)) // Backspace
        assertEquals(0x2c, KeyMapping.hidUsage(62)) // Space
        assertEquals(0x50, KeyMapping.hidUsage(21)) // Left arrow
        assertEquals(0xe1, KeyMapping.hidUsage(59)) // Left shift
        assertEquals(0xe0, KeyMapping.hidUsage(113)) // Left control
    }

    @Test
    fun rejectsUnknownKeys() {
        assertNull(KeyMapping.hidUsage(3)) // Home button
        assertNull(KeyMapping.hidUsage(9999))
    }
}
