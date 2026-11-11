package vista.module

/**
 * Minimal Android keycode -> HID usage page 0x07 mapping. The host resolves
 * these usages to Linux evdev codes and rejects anything it does not know, so
 * unmapped keys stay local instead of guessing.
 */
object KeyMapping {
    fun hidUsage(androidKeyCode: Int): Int? = when {
        androidKeyCode in 29..54 -> 0x04 + (androidKeyCode - 29) // A..Z
        androidKeyCode in 8..16 -> 0x1e + (androidKeyCode - 8) // 1..9
        androidKeyCode == 7 -> 0x27 // 0
        androidKeyCode == 66 -> 0x28 // Enter
        androidKeyCode == 111 -> 0x29 // Escape
        androidKeyCode == 67 -> 0x2a // Backspace
        androidKeyCode == 61 -> 0x2b // Tab
        androidKeyCode == 62 -> 0x2c // Space
        androidKeyCode == 69 -> 0x2d // Minus
        androidKeyCode == 70 -> 0x2e // Equals
        androidKeyCode == 71 -> 0x2f // Left bracket
        androidKeyCode == 72 -> 0x30 // Right bracket
        androidKeyCode == 73 -> 0x31 // Backslash
        androidKeyCode == 74 -> 0x33 // Semicolon
        androidKeyCode == 75 -> 0x34 // Apostrophe
        androidKeyCode == 68 -> 0x35 // Grave
        androidKeyCode == 55 -> 0x36 // Comma
        androidKeyCode == 56 -> 0x37 // Period
        androidKeyCode == 76 -> 0x38 // Slash
        androidKeyCode == 115 -> 0x39 // Caps lock
        androidKeyCode in 131..142 -> 0x3a + (androidKeyCode - 131) // F1..F12
        androidKeyCode == 122 -> 0x4a // Home
        androidKeyCode == 123 -> 0x4d // End
        androidKeyCode == 112 -> 0x4c // Forward delete
        androidKeyCode == 22 -> 0x4f // Right
        androidKeyCode == 21 -> 0x50 // Left
        androidKeyCode == 20 -> 0x51 // Down
        androidKeyCode == 19 -> 0x52 // Up
        androidKeyCode == 113 -> 0xe0 // Left control
        androidKeyCode == 59 -> 0xe1 // Left shift
        androidKeyCode == 57 -> 0xe2 // Left alt
        androidKeyCode == 114 -> 0xe4 // Right control
        androidKeyCode == 60 -> 0xe5 // Right shift
        androidKeyCode == 58 -> 0xe6 // Right alt
        else -> null
    }
}
