package vista.module

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge

/// Native Android entry point. The client is plain Kotlin/Jetpack Compose; the
/// SwiftUI views in `client/ios` are the iOS client and are not transpiled here.
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // Streaming should not be interrupted by the display sleeping.
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setContent {
            VistaTheme {
                VistaAndroidRoot()
            }
        }
    }
}
