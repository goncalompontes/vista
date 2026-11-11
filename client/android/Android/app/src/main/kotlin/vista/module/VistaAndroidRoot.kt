package vista.module

import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView

@Composable
fun VistaAndroidRoot() {
    val client = remember { HostControlClient() }
    var host by remember { mutableStateOf("") }
    var port by remember { mutableStateOf("7443") }
    var secret by remember { mutableStateOf("") }
    var status by remember { mutableStateOf("Not connected") }
    var windows by remember { mutableStateOf(emptyList<HostWindow>()) }
    var actions by remember { mutableStateOf(emptyList<HostAction>()) }
    var connected by remember { mutableStateOf(false) }
    var mediaActive by remember { mutableStateOf(false) }
    var frames by remember { mutableStateOf(0L) }
    var target by remember { mutableStateOf("desktop") }
    var textInput by remember { mutableStateOf("") }

    DisposableEffect(client) {
        client.onState = { status = it }
        client.onCatalog = { windows = it }
        client.onCapabilities = { actions = it }
        client.onActionResult = { status = "Action result: $it" }
        client.onConnected = { connected = it }
        client.onMediaActive = { mediaActive = it }
        client.onVideoStats = { frames = it }
        client.onStreamTarget = { target = it }
        client.onStreamError = { status = it }
        onDispose { client.close() }
    }

    if (!connected) {
        Column(
            modifier = Modifier.fillMaxSize().padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Vista", style = MaterialTheme.typography.headlineMedium)
            Text("Connect to a Vista host", style = MaterialTheme.typography.titleMedium)
            OutlinedTextField(host, { host = it }, label = { Text("Host address") }, modifier = Modifier.fillMaxWidth())
            OutlinedTextField(port, { port = it }, label = { Text("Port") }, modifier = Modifier.fillMaxWidth())
            OutlinedTextField(
                secret,
                { secret = it },
                label = { Text("64-character pairing secret") },
                modifier = Modifier.fillMaxWidth(),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(
                    onClick = { client.connect(host.trim(), port.toIntOrNull() ?: 0, secret.trim()) },
                    enabled = host.isNotBlank() && secret.length == 64,
                ) {
                    Text("Pair")
                }
                TextButton(onClick = { client.close(); status = "Disconnected" }) { Text("Disconnect") }
            }
            Text(status, style = MaterialTheme.typography.bodySmall)
            if (windows.isNotEmpty()) {
                Text("Host windows", style = MaterialTheme.typography.titleSmall)
                LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    items(windows, key = { it.id }) { window ->
                        Card(modifier = Modifier.fillMaxWidth()) {
                            Text(
                                window.title,
                                modifier = Modifier.fillMaxWidth().clickable { client.selectWindow(window.id) }.padding(12.dp),
                            )
                        }
                    }
                }
            }
        }
        return
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Box(
            modifier = Modifier.fillMaxWidth().weight(1f).background(Color.Black),
            contentAlignment = Alignment.Center,
        ) {
            val aspectRatio = if (client.decoder.width > 0 && client.decoder.height > 0) {
                client.decoder.width.toFloat() / client.decoder.height
            } else {
                16f / 9f
            }
            VideoSurface(client, Modifier.fillMaxSize().aspectRatio(aspectRatio))
            if (!mediaActive) {
                Text("Waiting for video…", color = Color.White)
            }
            Text(
                text = if (mediaActive) "Live · $frames frames" else "No media",
                color = Color.White,
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.align(Alignment.TopStart).padding(8.dp),
            )
        }
        Column(modifier = Modifier.fillMaxWidth().padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(status, style = MaterialTheme.typography.bodySmall)
            Text("Target: $target", style = MaterialTheme.typography.labelSmall)
            Text(
                "Touch: " + if (client.canSendInput()) "enabled" else "unavailable",
                style = MaterialTheme.typography.labelSmall,
            )
            if (client.canSendInput()) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    OutlinedTextField(
                        textInput,
                        { textInput = it },
                        label = { Text("Send text to the host") },
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(
                        onClick = {
                            client.sendText(textInput)
                            textInput = ""
                        },
                        enabled = textInput.isNotEmpty(),
                    ) {
                        Text("Send")
                    }
                }
            }
            LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                item {
                    Button(onClick = { client.selectDesktop() }) { Text("Desktop") }
                }
                items(windows, key = { it.id }) { window ->
                    TextButton(onClick = { client.selectWindow(window.id) }) {
                        Text(window.title.take(24))
                    }
                }
            }
            if (actions.isNotEmpty()) {
                LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    items(actions, key = { it.id }) { action ->
                        TextButton(
                            onClick = { client.invoke(action) },
                            enabled = action.selectable,
                        ) {
                            Text(action.label + (action.reason?.let { " · $it" } ?: ""))
                        }
                    }
                }
            }
            TextButton(onClick = { client.close(); status = "Disconnected" }) { Text("Disconnect") }
        }
    }
}

@Composable
private fun VideoSurface(client: HostControlClient, modifier: Modifier = Modifier) {
    AndroidView(
        modifier = modifier,
        factory = { context ->
            SurfaceView(context).also { view ->
                view.setOnTouchListener { touched, event ->
                    val normalizedX = (event.x / touched.width.coerceAtLeast(1)).toDouble()
                    val normalizedY = (event.y / touched.height.coerceAtLeast(1)).toDouble()
                    when (event.actionMasked) {
                        MotionEvent.ACTION_DOWN -> {
                            client.sendPointerButton(true)
                            client.sendPointer(normalizedX, normalizedY)
                        }
                        MotionEvent.ACTION_MOVE -> client.sendPointer(normalizedX, normalizedY)
                        MotionEvent.ACTION_UP -> {
                            client.sendPointer(normalizedX, normalizedY)
                            client.sendPointerButton(false)
                        }
                        MotionEvent.ACTION_CANCEL -> client.sendPointerButton(false)
                        else -> return@setOnTouchListener false
                    }
                    true
                }
                view.holder.addCallback(
                    object : SurfaceHolder.Callback {
                        override fun surfaceCreated(holder: SurfaceHolder) {
                            client.attachSurface(holder.surface)
                        }

                        override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) = Unit

                        override fun surfaceDestroyed(holder: SurfaceHolder) {
                            client.detachSurface()
                        }
                    },
                )
            }
        },
    )
}
