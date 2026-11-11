package vista.module

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// Sill's picture-first dark surfaces and restrained blue emphasis, expressed
// with Android Material components. This is an independent implementation.
val VistaBlue = Color(0xFF8CAEFF)
val VistaBackground = Color(0xFF101216)
val VistaPanel = Color(0xFF1B1E25)
val VistaMuted = Color(0xFFA5ADBE)

@Composable
fun VistaTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = darkColorScheme(
        primary = VistaBlue, onPrimary = Color(0xFF152A52),
        background = VistaBackground, onBackground = Color(0xFFE8EBF3),
        surface = VistaPanel, onSurface = Color(0xFFE8EBF3),
        surfaceVariant = Color(0xFF272C37), onSurfaceVariant = VistaMuted,
        outline = Color(0xFF414958), secondary = Color(0xFF9DD9C8),
    ), content = content)
}
