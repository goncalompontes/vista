package vista.module

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

@Composable
fun VistaAndroidRoot() {
    val client = remember { HostControlClient() }
    var host by remember { mutableStateOf("") }
    var port by remember { mutableStateOf("7443") }
    var secret by remember { mutableStateOf("") }
    var status by remember { mutableStateOf("Not connected") }
    var windows by remember { mutableStateOf(emptyList<HostWindow>()) }
    var actions by remember { mutableStateOf(emptyList<HostAction>()) }

    DisposableEffect(client) {
        client.onState = { status = it }
        client.onCatalog = { windows = it }
        client.onCapabilities = { actions = it }
        client.onActionResult = { status = "Action result: $it" }
        onDispose { client.close() }
    }

    Column(
        modifier = Modifier.fillMaxSize().padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Text("Vista", style = MaterialTheme.typography.headlineMedium)
        Text("Connect to a Vista host", style = MaterialTheme.typography.titleMedium)
        OutlinedTextField(host, { host = it }, label = { Text("Host address") }, modifier = Modifier.fillMaxWidth())
        OutlinedTextField(port, { port = it }, label = { Text("Port") }, modifier = Modifier.fillMaxWidth())
        OutlinedTextField(secret, { secret = it }, label = { Text("64-character pairing secret") }, modifier = Modifier.fillMaxWidth())
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = { client.connect(host, port.toIntOrNull() ?: 0, secret) }, enabled = host.isNotBlank() && secret.length == 64) {
                Text("Pair")
            }
            TextButton(onClick = { client.close(); status = "Disconnected" }) { Text("Disconnect") }
        }
        Text(status, style = MaterialTheme.typography.bodySmall)
        if (windows.isNotEmpty()) {
            Text("Windows", style = MaterialTheme.typography.titleMedium)
            LazyColumn(contentPadding = PaddingValues(bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                items(windows) { window ->
                    Card(modifier = Modifier.fillMaxWidth()) {
                        Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Text(window.title, style = MaterialTheme.typography.titleSmall)
                            Text(window.appId ?: "Window ${window.id}", style = MaterialTheme.typography.bodySmall)
                        }
                    }
                }
            }
        }
        if (actions.isNotEmpty()) {
            Text("Host actions", style = MaterialTheme.typography.titleMedium)
            actions.forEach { action ->
                Button(onClick = { client.invoke(action) }, enabled = action.selectable, modifier = Modifier.fillMaxWidth()) {
                    Text(if (action.selectable) action.label else "${action.label} (${action.reason ?: "unavailable"})")
                }
            }
        }
    }
}
