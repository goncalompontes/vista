package vista.module

import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver

@Composable
fun VistaAndroidRoot() {
    val context = LocalContext.current
    val activity = context as? ComponentActivity
    val client = remember { HostControlClient() }
    val inputFocus = remember { FocusRequester() }
    var model by remember { mutableStateOf(client.state) }
    var enteredSession by remember { mutableStateOf(false) }
    var library by remember { mutableStateOf(true) }
    var keyboard by remember { mutableStateOf(false) }
    var destructive by remember { mutableStateOf<HostAction?>(null) }
    val disconnect = { client.disconnect(); enteredSession = false; library = true; keyboard = false }

    DisposableEffect(client, activity) {
        client.onState = {
            model = it
            if (it.connected) enteredSession = true
        }
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_START -> client.setForeground(true)
                Lifecycle.Event.ON_STOP -> client.setForeground(false)
                else -> Unit
            }
        }
        activity?.lifecycle?.addObserver(observer)
        onDispose { activity?.lifecycle?.removeObserver(observer); client.dispose() }
    }
    DisposableEffect(model.connected, activity) {
        if (model.connected) activity?.window?.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        else activity?.window?.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        onDispose { activity?.window?.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
    }
    LaunchedEffect(model.controlEnabled, keyboard, library) {
        if (model.controlEnabled && !keyboard && !library) inputFocus.requestFocus()
    }
    BackHandler(enteredSession && !library) { client.setControl(false); library = true }

    Surface(modifier = Modifier.fillMaxSize(), color = VistaBackground) {
        Box(Modifier.fillMaxSize().systemBarsPadding().imePadding()
            .focusRequester(inputFocus).onPreviewKeyEvent { key ->
                if (!model.canControl || keyboard || library) false else {
                    val event = key.nativeKeyEvent
                    val usage = KeyMapping.hidUsage(event.keyCode)
                    if (usage == null || event.action !in listOf(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.ACTION_UP)) false
                    else client.sendKey(usage, event.action == android.view.KeyEvent.ACTION_DOWN, event.repeatCount > 0)
                }
            }.focusable()) {
            if (!enteredSession) PairingScreen(model, client::connect, disconnect)
            else Column(Modifier.fillMaxSize()) {
                SessionHeader(model, disconnect)
                if (library) SourceLibrary(model, { target -> client.select(target); library = false },
                    if (model.target != null) ({ library = false }) else null)
                else StreamScreen(client, model,
                    sources = { client.setControl(false); library = true },
                    keyboard = { client.releaseInput(); keyboard = true },
                    action = { if (it.id == "window.close") destructive = it else client.invoke(it) })
            }
        }
    }
    if (keyboard) TextEntryDialog(model.canControl, { client.sendText(it) }, { keyboard = false })
    destructive?.let { action ->
        AlertDialog(onDismissRequest = { destructive = null }, title = { Text("Close this host window?") },
            text = { Text("This closes ${model.targetLabel} on the shared host. Unsaved work may be lost.") },
            confirmButton = { TextButton(onClick = { client.invoke(action); destructive = null }) { Text("Close window") } },
            dismissButton = { TextButton(onClick = { destructive = null }) { Text("Cancel") } })
    }
}

@Composable
private fun PairingScreen(model: ClientState, connect: (String, Int, String) -> Unit, disconnect: () -> Unit) {
    var host by rememberSaveable { mutableStateOf("") }
    var port by rememberSaveable { mutableStateOf("7443") }
    // The invitation secret is deliberately absent from saved state/storage.
    var secret by remember { mutableStateOf("") }
    var showSecret by remember { mutableStateOf(false) }
    val busy = model.connection != ConnectionState.Offline
    val valid = host.isNotBlank() && port.toIntOrNull() in 1..65535 && secret.matches(Regex("[0-9a-fA-F]{64}"))
    Box(Modifier.fillMaxSize().background(Brush.verticalGradient(listOf(Color(0xFF1A2440), VistaBackground))),
        contentAlignment = Alignment.Center) {
        Column(Modifier.widthIn(max = 480.dp).fillMaxWidth().verticalScroll(rememberScrollState()).padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(20.dp)) {
            Surface(color = VistaBlue, shape = RoundedCornerShape(22.dp), modifier = Modifier.size(64.dp)) {
                Box(contentAlignment = Alignment.Center) { Text("V", style = MaterialTheme.typography.displaySmall,
                    fontWeight = FontWeight.Bold, color = Color(0xFF152A52)) }
            }
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Your desktop,\nwithin reach.", style = MaterialTheme.typography.headlineLarge, fontWeight = FontWeight.SemiBold)
                Text("Connect Vista to your host and bring a workspace with you.", color = VistaMuted,
                    style = MaterialTheme.typography.bodyLarge)
            }
            Card(colors = CardDefaults.cardColors(containerColor = VistaPanel)) {
                Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    Text("Pair with a host", style = MaterialTheme.typography.titleLarge)
                    OutlinedTextField(host, { host = it }, label = { Text("Host address") }, singleLine = true,
                        placeholder = { Text("192.168.1.20") }, enabled = !busy, modifier = Modifier.fillMaxWidth(),
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri))
                    OutlinedTextField(port, { port = it }, label = { Text("Port") }, singleLine = true,
                        enabled = !busy, modifier = Modifier.fillMaxWidth(),
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number))
                    OutlinedTextField(secret, { secret = it.trim() }, label = { Text("Pairing secret") }, singleLine = true,
                        supportingText = { Text("The 64-character secret from your host invitation") },
                        visualTransformation = if (showSecret) VisualTransformation.None else PasswordVisualTransformation(),
                        trailingIcon = { TextButton(onClick = { showSecret = !showSecret }) { Text(if (showSecret) "Hide" else "Show") } },
                        enabled = !busy, modifier = Modifier.fillMaxWidth(),
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password))
                    Button(onClick = { connect(host.trim(), port.toInt(), secret) }, enabled = valid && !busy,
                        modifier = Modifier.fillMaxWidth().heightIn(min = 50.dp)) { Text("Connect to host") }
                    if (busy) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                            Text(model.message, style = MaterialTheme.typography.bodySmall, modifier = Modifier.weight(1f))
                        }
                        TextButton(onClick = disconnect, modifier = Modifier.fillMaxWidth()) { Text("Cancel connection") }
                    } else Text(model.message, color = VistaMuted, style = MaterialTheme.typography.bodySmall)
                }
            }
            Text("Use a host and network you trust. Keep the invitation secret private.", color = VistaMuted,
                style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
private fun SessionHeader(model: ClientState, disconnect: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text("Vista", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
            Text(model.host.ifBlank { "Your host" }, color = VistaMuted, style = MaterialTheme.typography.labelMedium)
        }
        TextButton(onClick = disconnect, modifier = Modifier.heightIn(min = 48.dp)) { Text("Disconnect") }
    }
    if (!model.connected) LinearProgressIndicator(Modifier.fillMaxWidth())
}

@Composable
private fun SourceLibrary(model: ClientState, select: (StreamTarget) -> Unit, resume: (() -> Unit)?) {
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(20.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            Text("Choose your workspace", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.SemiBold)
            Text("Open windows from your host, or view its desktop.", color = VistaMuted, modifier = Modifier.padding(top = 8.dp))
        }
        if (resume != null) item { OutlinedButton(onClick = resume, modifier = Modifier.fillMaxWidth()) { Text("Return to picture") } }
        item {
            SourceCard("Desktop", "The display currently shared by your host", "D", model.connected) { select(StreamTarget.Desktop) }
        }
        item { Text("ACTIVE WINDOWS", style = MaterialTheme.typography.labelMedium, color = VistaMuted,
            modifier = Modifier.padding(top = 16.dp, bottom = 4.dp)) }
        if (model.windows.isEmpty()) item {
            Text(if (model.connected) "No windows are listed by the host." else model.message,
                color = VistaMuted, modifier = Modifier.padding(vertical = 20.dp))
        }
        items(model.windows, key = { it.id }) { window ->
            SourceCard(window.title, window.appId ?: "Window on your host", window.title.take(1).uppercase(), model.connected) {
                select(StreamTarget("window", window.id))
            }
        }
    }
}

@Composable
private fun SourceCard(title: String, subtitle: String, badge: String, enabled: Boolean, click: () -> Unit) {
    Card(Modifier.fillMaxWidth().semantics { contentDescription = "Open $title" }.clickable(enabled = enabled, onClick = click),
        colors = CardDefaults.cardColors(containerColor = VistaPanel)) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Surface(color = Color(0xFF303D5A), shape = RoundedCornerShape(12.dp), modifier = Modifier.size(48.dp)) {
                Box(contentAlignment = Alignment.Center) { Text(badge, color = VistaBlue, style = MaterialTheme.typography.titleLarge) }
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(title.ifBlank { "Untitled window" }, maxLines = 2, overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.titleMedium)
                Text(subtitle, maxLines = 1, overflow = TextOverflow.Ellipsis, color = VistaMuted,
                    style = MaterialTheme.typography.bodySmall)
            }
            Text("Open", color = if (enabled) VistaBlue else VistaMuted, style = MaterialTheme.typography.labelLarge)
        }
    }
}

@Composable
private fun StreamScreen(client: HostControlClient, model: ClientState, sources: () -> Unit,
                         keyboard: () -> Unit, action: (HostAction) -> Unit) {
    // Keep the native video view at the same composition position across
    // orientation changes. Replacing a Row with a Column can leave a rendered
    // but detached SurfaceView on the emulator.
    BoxWithConstraints(Modifier.fillMaxSize().padding(8.dp)) {
        val wide = maxWidth > 700.dp || maxWidth > maxHeight
        val pictureWidth = if (wide) (maxWidth - 292.dp).coerceAtLeast(1.dp) else maxWidth
        val pictureHeight = if (wide) maxHeight else maxHeight * 0.4f
        val controlsX = if (wide) pictureWidth + 12.dp else 0.dp
        val controlsY = if (wide) 0.dp else pictureHeight + 8.dp
        val controlsWidth = if (wide) 280.dp else maxWidth
        val controlsHeight = if (wide) maxHeight else (maxHeight - controlsY).coerceAtLeast(1.dp)
        Picture(client, model, Modifier.width(pictureWidth).height(pictureHeight))
        Column(Modifier.offset(x = controlsX, y = controlsY).width(controlsWidth).height(controlsHeight)
            .verticalScroll(rememberScrollState()).padding(horizontal = if (wide) 0.dp else 8.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            StreamControls(client, model, sources, keyboard, action)
        }
    }
}

@Composable
private fun Picture(client: HostControlClient, model: ClientState, modifier: Modifier) {
    Surface(modifier, color = Color.Black, shape = RoundedCornerShape(16.dp)) {
        Box(Modifier.fillMaxSize()) {
            VideoSurface(client, model, Modifier.fillMaxSize())
            if (model.playback != PlaybackState.Live) {
                Column(Modifier.align(Alignment.Center).background(Color.Black.copy(alpha = 0.75f), RoundedCornerShape(16.dp)).padding(20.dp),
                    horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    if (model.connection != ConnectionState.Offline && model.playback != PlaybackState.None)
                        CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                    Text(model.videoLabel, color = Color.White, style = MaterialTheme.typography.titleSmall)
                    if (!model.connected) Text(model.message, color = VistaMuted, style = MaterialTheme.typography.bodySmall)
                }
            }
            Surface(Modifier.align(Alignment.TopStart).padding(10.dp), color = Color(0xDD16181C), shape = RoundedCornerShape(8.dp)) {
                Text(model.videoLabel, Modifier.padding(horizontal = 10.dp, vertical = 6.dp), color = Color.White,
                    style = MaterialTheme.typography.labelMedium)
            }
        }
    }
}

@Composable
private fun StreamControls(client: HostControlClient, model: ClientState, sources: () -> Unit,
                           keyboard: () -> Unit, action: (HostAction) -> Unit) {
    Text(model.targetLabel, style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
    if (model.captureScope == "display") Text("Your host is sharing its display, including other visible windows.",
        color = VistaMuted, style = MaterialTheme.typography.bodySmall)
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedButton(onClick = sources, modifier = Modifier.weight(1f)) { Text("Sources") }
        OutlinedButton(onClick = keyboard, enabled = model.canControl, modifier = Modifier.weight(1f)) { Text("Keyboard") }
    }
    LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        items(model.windows, key = { it.id }) { window ->
            FilterChip(selected = model.target == StreamTarget("window", window.id),
                onClick = { client.select(StreamTarget("window", window.id)) }, enabled = model.connected,
                label = { Text(window.title.take(20)) })
        }
    }
    Surface(color = VistaPanel, shape = RoundedCornerShape(16.dp)) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(if (model.canControl) "Control enabled" else "View only", style = MaterialTheme.typography.titleSmall)
                    Text(if (model.inputGranted) model.scopeLabel else "The host has not granted input access.",
                        style = MaterialTheme.typography.bodySmall, color = VistaMuted)
                }
                Switch(checked = model.controlEnabled, onCheckedChange = client::setControl,
                    enabled = model.inputGranted && model.playback == PlaybackState.Live && model.foreground,
                    modifier = Modifier.padding(start = 6.dp).semantics { contentDescription = "Enable host control" })
            }
            Text("Tap to click · Drag to move · Two fingers to scroll · Hold for right click",
                color = VistaMuted, style = MaterialTheme.typography.bodySmall)
        }
    }
    KeyStrip(client, model)
    if (model.actions.isNotEmpty()) {
        Text("HOST ACTIONS", color = VistaMuted, style = MaterialTheme.typography.labelSmall)
        LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            items(model.actions, key = { it.id }) { hostAction ->
                OutlinedButton(onClick = { action(hostAction) }, enabled = model.connected && hostAction.selectable) {
                    Text(hostAction.label)
                }
            }
        }
        model.actions.firstOrNull { !it.selectable && it.reason != null }?.reason?.let {
            Text(it, color = VistaMuted, style = MaterialTheme.typography.bodySmall)
        }
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(if (model.connected && model.playback == PlaybackState.Live) "${model.width} × ${model.height} · Connected" else model.message,
            style = MaterialTheme.typography.labelSmall, color = VistaMuted, modifier = Modifier.weight(1f))
        TextButton(onClick = { client.releaseInput(); client.setControl(false) }, enabled = model.canControl) { Text("Release input") }
    }
}

@Composable
private fun KeyStrip(client: HostControlClient, model: ClientState) {
    var held by remember(model.inputToken) { mutableStateOf(setOf<Int>()) }
    LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        items(listOf("Esc" to 0x29, "Tab" to 0x2b, "Ctrl" to 0xe0, "Alt" to 0xe2, "Shift" to 0xe1,
            "←" to 0x50, "↑" to 0x52, "↓" to 0x51, "→" to 0x4f)) { (label, usage) ->
            val modifier = usage >= 0xe0
            FilterChip(selected = usage in held, enabled = model.canControl,
                onClick = {
                    if (modifier) {
                        val pressed = usage !in held
                        client.sendKey(usage, pressed)
                        held = if (pressed) held + usage else held - usage
                    } else { client.sendKey(usage, true); client.sendKey(usage, false) }
                }, label = { Text(label) })
        }
    }
}

@Composable
private fun TextEntryDialog(enabled: Boolean, send: (String) -> Boolean, close: () -> Unit) {
    var text by remember { mutableStateOf("") }
    var message by remember { mutableStateOf<String?>(null) }
    AlertDialog(onDismissRequest = close, title = { Text("Send text") }, text = {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Type here, then send the committed text to the selected host.", color = VistaMuted)
            OutlinedTextField(text, { text = it }, label = { Text("Text for host") }, enabled = enabled,
                modifier = Modifier.fillMaxWidth(), minLines = 2, maxLines = 5)
            if (!enabled) Text("Enable control to send text.", color = VistaMuted)
            message?.let { Text(it, color = VistaMuted) }
        }
    }, confirmButton = {
        TextButton(enabled = enabled && text.isNotEmpty(), onClick = {
            if (send(text)) { text = ""; message = "Text sent. Check the host for the result." }
            else message = "Text could not be sent. Check control access and use at most 16 KiB."
        }) { Text("Send to host") }
    }, dismissButton = { TextButton(onClick = close) { Text("Done") } })
}
