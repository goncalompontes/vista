package vista.module

import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Surface
import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong

/** Authenticated v1 adapters. The pure reducer owns UI state; every queued
 * command, media frame and callback is fenced by its original connection.
 */
class HostControlClient {
    private val lock = Any()
    private val worker = Executors.newSingleThreadExecutor()
    private val commands = Executors.newSingleThreadExecutor()
    private val mediaWorker = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())
    private val generations = AtomicLong()
    private var run = 0L
    private var stopped = true
    private var disposed = false
    private var active: Control? = null
    private var connecting: Socket? = null
    private var mediaSocket: Socket? = null
    private var mediaSession: VistaSession? = null
    private var desired: StreamTarget? = null
    private var capability: JSONObject? = null
    private val input = InputGuard()
    val decoder = MediaDecoder()
    @Volatile var state = ClientState()
        private set
    var onState: ((ClientState) -> Unit)? = null

    init {
        decoder.onFrameRendered = { tag ->
            synchronized(lock) {
                if (state.playback != PlaybackState.Live)
                    event(tag.connection, ClientEvent.Frame(tag.selection, tag.geometry))
            }
        }
    }

    private data class Control(val id: Long, val socket: Socket, val session: VistaSession)

    fun connect(host: String, port: Int, secretHex: String) {
        val secret = runCatching { VistaWire.hexToBytes(secretHex) }.getOrNull()
        if (host.isBlank() || port !in 1..65535 || secret?.size != 32) {
            synchronized(lock) { publish(state.copy(message = "Enter a host, valid port and 64-character hexadecimal secret.")) }
            return
        }
        disconnect()
        val currentRun: Long
        synchronized(lock) {
            if (disposed) return
            stopped = false
            desired = null
            currentRun = ++run
            publish(ClientState(host = host, connection = ConnectionState.Connecting, message = "Connecting…"))
        }
        worker.execute {
            var retry = 500L
            while (synchronized(lock) { !stopped && run == currentRun }) {
                val id = generations.incrementAndGet()
                synchronized(lock) {
                    if (stopped || run != currentRun) return@execute
                    publish(ClientState(connectionId = id, host = host, foreground = state.foreground,
                        connection = if (retry == 500L) ConnectionState.Connecting else ConnectionState.Reconnecting,
                        message = "Connecting to $host…"))
                }
                var ctx: Control? = null
                try {
                    val sock = Socket()
                    synchronized(lock) {
                        if (stopped || run != currentRun) { sock.close(); return@execute }
                        connecting = sock
                    }
                    sock.tcpNoDelay = true
                    sock.connect(InetSocketAddress(host, port), 12_000)
                    sock.soTimeout = 15_000
                    val session = VistaSession(BufferedInputStream(sock.getInputStream()),
                        BufferedOutputStream(sock.getOutputStream()), secret, "vista-android")
                    session.trace = { Log.d("VistaWire", "control: $it") }
                    session.handshake(wantInput = true, encryptSessionOpen = true)
                    sock.soTimeout = 0
                    ctx = Control(id, sock, session)
                    synchronized(lock) {
                        if (stopped || run != currentRun) { sock.close(); return@execute }
                        connecting = null
                        active = ctx
                        event(id, ClientEvent.Authenticated(session.policyInput))
                        desired?.let { target ->
                            event(id, ClientEvent.Selecting(target))
                            sendSelection(ctx, target)
                        }
                    }
                    retry = 500L
                    while (synchronized(lock) { active === ctx && !stopped && run == currentRun }) {
                        handle(ctx, session.readSecure(), host, secret)
                    }
                } catch (error: Exception) {
                    synchronized(lock) {
                        if (!stopped && run == currentRun) {
                            event(id, ClientEvent.Lost("Connection lost. Retrying…"))
                            Log.i("VistaWire", "control ended: ${error.javaClass.simpleName}")
                        }
                    }
                } finally {
                    synchronized(lock) {
                        if (active === ctx) active = null
                        closeSocket(ctx?.socket)
                        closeSocket(connecting)
                        connecting = null
                        stopMedia()
                        input.clear()
                        capability = null
                    }
                }
                if (synchronized(lock) { stopped || run != currentRun }) break
                Thread.sleep(retry)
                retry = minOf(retry * 2, 5_000L)
            }
            secret.fill(0)
        }
    }

    fun select(target: StreamTarget) {
        synchronized(lock) {
            val ctx = active ?: return
            if (!state.connected || target == state.target) return
            cancelLocked(ctx)
            val replacing = state.target != null
            desired = target
            capability = null
            event(ctx.id, ClientEvent.Selecting(target))
            if (replacing) {
                // v1 media records carry no target generation. A fresh pair of
                // sockets prevents old buffered pictures becoming the new target.
                event(ctx.id, ClientEvent.Lost("Opening source…"))
                closeSocket(ctx.socket)
                stopMedia()
            } else commands.execute {
                synchronized(lock) {
                    if (active === ctx && state.target == target) sendSelection(ctx, target)
                }
            }
        }
    }

    fun setControl(enabled: Boolean) = synchronized(lock) {
        if (!enabled) active?.let { cancelLocked(it) }
        event(state.connectionId, ClientEvent.Control(enabled))
    }

    fun setForeground(foreground: Boolean) = synchronized(lock) {
        if (!foreground) active?.let { cancelLocked(it) }
        event(state.connectionId, ClientEvent.Foreground(foreground))
    }

    fun releaseInput() = synchronized(lock) {
        active?.let { cancelLocked(it) }
        event(state.connectionId, ClientEvent.Release)
    }

    fun send(token: InputToken, events: List<RemoteInput>) {
        val ctx = synchronized(lock) { active?.takeIf { it.id == token.connection && state.inputToken == token } } ?: return
        commands.execute {
            synchronized(lock) {
                if (active !== ctx) return@synchronized
                try {
                    for (command in events) {
                        if (!input.accept(command, token, state.inputToken)) continue
                        ctx.session.sendSecure(JSONObject().put("type", "input.event").put("v", 1)
                            .put("session_id", ctx.session.sessionId).put("event", command.wire(token.geometry)))
                        if (command == RemoteInput.Cancel) event(ctx.id, ClientEvent.Release)
                        Log.d("VistaInput", "send ${command.wire(token.geometry).optString("type")}")
                    }
                } catch (_: Exception) {
                    event(ctx.id, ClientEvent.Lost("Input connection lost. Retrying…"))
                    closeSocket(ctx.socket)
                }
            }
        }
    }

    fun sendKey(usage: Int, pressed: Boolean, repeat: Boolean = false): Boolean {
        val token = state.inputToken ?: return false
        send(token, listOf(RemoteInput.Key(usage, pressed, repeat)))
        return true
    }

    fun sendText(text: String): Boolean {
        val token = state.inputToken ?: return false
        if (text.isEmpty() || text.toByteArray().size > 16 * 1024) return false
        send(token, listOf(RemoteInput.Text(text)))
        return true
    }

    fun invoke(action: HostAction) {
        val ctx: Control
        val snapshot: JSONObject
        val selection: Long
        synchronized(lock) {
            ctx = active ?: return
            snapshot = capability ?: return
            selection = state.selection
            if (!state.connected || !state.foreground || !state.actions.contains(action) || !action.selectable) return
        }
        commands.execute {
            synchronized(lock) {
                if (active !== ctx || capability !== snapshot || selection != state.selection ||
                    !state.foreground || !state.actions.contains(action)) return@synchronized
                try {
                    ctx.session.sendSecure(JSONObject().put("type", "action.invoke").put("v", 1)
                        .put("id", "android-${System.nanoTime()}")
                        .put("connection_epoch", snapshot.getString("connection_epoch"))
                        .put("session_id", ctx.session.sessionId)
                        .put("grant_epoch", snapshot.getString("grant_epoch"))
                        .put("target_id", snapshot.getString("target_id"))
                        .put("target_generation", snapshot.getString("target_generation"))
                        .put("capability_revision", snapshot.getLong("capability_revision"))
                        .put("action", action.id).put("schema_version", 1)
                        .put("parameters", JSONObject().apply {
                            if (action.id == "window.maximize.set") put("value", action.label == "Maximize")
                        }))
                } catch (_: Exception) { closeSocket(ctx.socket) }
            }
        }
    }

    fun attachSurface(surface: Surface) {
        decoder.attachSurface(surface)
        synchronized(lock) { requestKeyframe() }
    }
    fun detachSurface(surface: Surface) {
        if (decoder.detachSurface(surface)) {
            releaseInput()
            synchronized(lock) { event(state.connectionId, ClientEvent.Recovering(state.selection)) }
        }
    }

    fun disconnect() = synchronized(lock) {
        stopped = true
        run++
        active?.let { cancelLocked(it); closeSocket(it.socket) }
        active = null
        closeSocket(connecting)
        connecting = null
        stopMedia()
        input.clear()
        capability = null
        desired = null
        publish(ClientState(connectionId = generations.incrementAndGet(), foreground = state.foreground,
            message = "Disconnected"))
    }

    fun dispose() {
        synchronized(lock) { disposed = true }
        disconnect()
        worker.shutdownNow()
        commands.shutdownNow()
        mediaWorker.shutdownNow()
        decoder.release()
        onState = null
    }

    private fun cancelLocked(ctx: Control) {
        // Main-thread lifecycle/layout callbacks only invalidate local state.
        // Network cleanup runs on the ordered sender; the host also releases
        // the lease if the socket closes before this cancellation is written.
        input.clear()
        if (!state.streamReady) return
        commands.execute {
            synchronized(lock) {
                if (active !== ctx) return@synchronized
                try {
                    ctx.session.sendSecure(JSONObject().put("type", "input.event").put("v", 1)
                        .put("session_id", ctx.session.sessionId).put("event", RemoteInput.Cancel.wire(0)))
                } catch (_: Exception) { closeSocket(ctx.socket) }
            }
        }
    }

    private fun sendSelection(ctx: Control, target: StreamTarget) {
        val wireTarget = JSONObject().put("kind", target.kind)
        target.id?.let { wireTarget.put("id", it) }
        ctx.session.sendSecure(JSONObject().put("type", "stream.select").put("v", 1)
            .put("session_id", ctx.session.sessionId).put("target", wireTarget))
    }

    private fun handle(ctx: Control, message: JSONObject, host: String, secret: ByteArray) = synchronized(lock) {
        if (active !== ctx || message.optString("session_id", ctx.session.sessionId) != ctx.session.sessionId) return@synchronized
        when (message.getString("type")) {
            "catalog.snapshot" -> event(ctx.id, ClientEvent.Catalog(message.getLong("revision"),
                parseCatalog(message.getJSONArray("entries"))))
            "capabilities.snapshot" -> {
                val target = state.target
                val targetId = when (target?.kind) {
                    "window" -> "w${target.id}"
                    "workspace" -> "ws${target.id}"
                    "desktop" -> "desktop"
                    else -> null
                }
                if (message.optString("target_id") == targetId && message.getLong("capability_revision") > state.capabilityRevision) {
                    capability = message
                    event(ctx.id, ClientEvent.Capabilities(message.getLong("capability_revision"), parseActions(message.getJSONArray("actions"))))
                }
            }
            "media.offer" -> startMedia(ctx, host, secret, message.getInt("port"))
            "stream.started" -> {
                cancelLocked(ctx)
                val target = message.getJSONObject("target")
                val selected = StreamTarget(target.getString("kind"), if (target.has("id")) target.getLong("id") else null)
                event(ctx.id, ClientEvent.Geometry(selected, message.getLong("geometry_epoch"),
                    message.getInt("width"), message.getInt("height"),
                    message.optString("input_scope", "unknown"), message.optString("capture_scope", "unknown")))
                if (state.streamReady && state.target == selected) {
                    decoder.resetForNewStream(state.width, state.height, InputToken(ctx.id, state.selection, state.geometryEpoch))
                    requestKeyframe()
                }
            }
            "stream.stopped", "stream.error" -> {
                cancelLocked(ctx)
                event(ctx.id, ClientEvent.Stopped(message.optString("message", message.optString("reason", "Source stopped"))))
            }
            "action.result" -> event(ctx.id, ClientEvent.Message("Action: ${message.optString("status", "unknown")}"))
            "session_close", "error" -> {
                cancelLocked(ctx)
                event(ctx.id, ClientEvent.Lost("Host ended the session. Retrying…"))
                closeSocket(ctx.socket)
            }
        }
    }

    private fun startMedia(ctx: Control, host: String, secret: ByteArray, port: Int) {
        stopMedia()
        mediaWorker.execute {
            val sock = Socket()
            try {
                synchronized(lock) { if (active !== ctx) return@execute; mediaSocket = sock }
                sock.connect(InetSocketAddress(host, port), 12_000)
                sock.soTimeout = 15_000
                val media = VistaSession(BufferedInputStream(sock.getInputStream()),
                    BufferedOutputStream(sock.getOutputStream()), secret, "vista-android-media")
                media.trace = { Log.d("VistaWire", "media: $it") }
                media.handshake(wantInput = false, encryptSessionOpen = false)
                sock.soTimeout = 0
                synchronized(lock) {
                    if (active !== ctx) return@execute
                    mediaSession = media
                    requestKeyframe()
                }
                while (true) {
                    val record = media.readRecord()
                    val tag = synchronized(lock) {
                        if (active !== ctx) return@execute
                        if (state.streamReady) InputToken(ctx.id, state.selection, state.geometryEpoch) else null
                    } ?: continue
                    if (record.discontinuity) synchronized(lock) {
                        if (active === ctx && state.selection == tag.selection && state.geometryEpoch == tag.geometry) {
                            // Disabling local control must not leave host keys/buttons held.
                            cancelLocked(ctx)
                            event(ctx.id, ClientEvent.Recovering(tag.selection))
                        }
                    }
                    decoder.submit(record, tag)
                }
            } catch (error: Exception) {
                synchronized(lock) {
                    if (active === ctx) {
                        event(ctx.id, ClientEvent.Recovering(state.selection))
                        closeSocket(ctx.socket) // Re-pair both channels, never strand a frozen media socket.
                    }
                }
                Log.i("VistaWire", "media ended: ${error.javaClass.simpleName}")
            } finally {
                closeSocket(sock)
                synchronized(lock) { if (mediaSocket === sock) { mediaSocket = null; mediaSession = null } }
            }
        }
    }

    private fun requestKeyframe() {
        val media = mediaSession ?: return
        val ctx = active ?: return
        commands.execute {
            synchronized(lock) {
                if (mediaSession !== media || active !== ctx) return@synchronized
                try { media.sendKeyframeRequest() }
                catch (_: Exception) { closeSocket(ctx.socket) }
            }
        }
    }
    private fun stopMedia() { closeSocket(mediaSocket); mediaSocket = null; mediaSession = null }
    private fun event(id: Long, event: ClientEvent) {
        val next = ClientReducer.reduce(state, id, event)
        publish(next)
    }
    private fun publish(next: ClientState) {
        if (state == next) return
        state = next
        main.post { if (state === next) onState?.invoke(next) }
    }
    private fun closeSocket(socket: Socket?) { runCatching { socket?.close() } }

    private fun parseCatalog(entries: JSONArray): List<HostWindow> = (0 until entries.length()).map { index ->
        val entry = entries.getJSONObject(index)
        HostWindow(entry.getLong("window_id"), entry.optString("title", "Untitled window"),
            entry.optString("app_id").takeIf { it.isNotEmpty() })
    }
    private fun parseActions(actions: JSONArray): List<HostAction> = (0 until actions.length()).map { index ->
        val action = actions.getJSONObject(index)
        HostAction(action.getString("id"), action.optJSONObject("presentation")?.optString("label") ?: action.getString("id"),
            action.optBoolean("supported") && action.optBoolean("enabled") && action.optString("authorization") == "granted",
            action.optString("reason").takeIf { it.isNotEmpty() })
    }.filter { it.id in setOf("window.focus", "window.close", "window.maximize.set", "host.workspace.next") }
}
