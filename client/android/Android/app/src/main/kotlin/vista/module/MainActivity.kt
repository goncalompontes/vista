package vista.module

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.MaterialTheme

/// Native Android entry point. The client is plain Kotlin/Jetpack Compose; the
/// SwiftUI views in `client/ios` are the iOS client and are not transpiled here.
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            MaterialTheme {
                VistaAndroidRoot()
            }
        }
    }
}
