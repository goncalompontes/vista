package vista.module

import android.os.Handler
import android.util.Log
import android.os.Looper
import android.view.Surface
import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.Executors

data class HostWindow(val id: Long, val title: String, val appId: String?)
data class HostAction(val id: String, val label: String, val selectable: Boolean, val reason: String?)

private data class MediaOfferInfo(val port: Int, val pairingId: String, val expiresAt: Long)

/**
 * Control and media connections to one Vista host session.
 *
 * The control connection pairs, selects a stream and serves catalog/capability
 * updates. `media.offer` starts a second authenticated connection that carries
 * AEAD-framed H.264 access units into [decoder].
 */
class HostControlClient {
    private val executor = Executors.newSingleThreadExecutor()
    private val commandExecutor = Executors.newSingleThreadExecutor()
    private val mediaExecutor = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())

    val decoder = MediaDecoder()

    private var socket: Socket? = null
    private var mediaSocket: Socket? = null
    private var session: VistaSession? = null
    private var mediaSession: VistaSession? = null
    @Volatile private var sessionActive = false
    @Volatile private var mediaActive = false
    @Volatile private var sessionId = ""
    @Volatile private var geometryEpoch = 0L
    @Volatile private var inputAllowed = false
    private var hostAddress = ""
    private var secret = ByteArray(0)
    private var capabilityConnectionEpoch = ""
    private var capabilityGrantEpoch = ""
    private var capabilityTargetId = ""
    private var capabilityTargetGeneration = ""
    private var capabilityRevision = 0L

    var onState: ((String) -> Unit)? = null
    var onCatalog: ((List<HostWindow>) -> Unit)? = null
    var onCapabilities: ((List<HostAction>) -> Unit)? = null
    var onActionResult: ((String) -> Unit)? = null
    var onConnected: ((Boolean) -> Unit)? = null
    var onMediaActive: ((Boolean) -> Unit)? = null
    var onStreamTarget: ((String) -> Unit)? = null
    var onStreamError: ((String) -> Unit)? = null
    var onVideoStats: ((Long) -> Unit)? = null

    fun connect(host: String, port: Int, secretHex: String) {
        close()
        executor.execute {
            try {
                publish("Connecting…")
                secret = VistaWire.hexToBytes(secretHex)
                require(secret.size == VistaWire.PAIRING_SECRET_BYTES) {
                    "Pairing secret must be 64 hex characters"
                }
                hostAddress = host
                val sock = Socket()
                sock.connect(InetSocketAddress(host, port), 12_000)
                socket = sock
                val control = VistaSession(
                    BufferedInputStream(sock.getInputStream()),
                    BufferedOutputStream(sock.getOutputStream()),
                    secret,
                    "vista-android",
                )
                session = control
                control.trace = { Log.d("VistaWire", "control: $it") }
                control.handshake(wantInput = true, encryptSessionOpen = true)
                sessionId = control.sessionId
                sessionActive = true
                inputAllowed = control.policyInput
                publish("Connected to $host" + if (inputAllowed) " (input enabled)" else "")
                publishConnected(true)
                select(JSONObject().put("kind", "desktop"))
                while (sessionActive) {
                    handle(control.readSecure())
                }
            } catch (error: Throwable) {
                Log.e("VistaWire", "control connection failed", error)
                if (sessionActive) {
                    publish("Connection failed: ${error.message ?: error.javaClass.simpleName}")
                }
            } finally {
                sessionActive = false
                session = null
                publishConnected(false)
                closeSocket(socket)
                socket = null
                publishCatalog(emptyList())
                publishCapabilities(emptyList())
            }
        }
    }

    fun selectDesktop() {
        select(JSONObject().put("kind", "desktop"))
    }

    fun selectWindow(windowId: Long) {
        select(JSONObject().put("kind", "window").put("id", windowId))
    }

    fun selectWorkspace(workspaceId: Long) {
        select(JSONObject().put("kind", "workspace").put("id", workspaceId))
    }

    /** True when the host granted portable input for this session. */
    fun canSendInput(): Boolean = inputAllowed

    fun attachSurface(surface: Surface) {
        decoder.attachSurface(surface)
    }

    /** Normalized pointer position within the streamed frame. */
    fun sendPointer(x: Double, y: Double) {
        if (!inputAllowed) return
        commandExecutor.execute {
            sendInputEvent(
                JSONObject()
                    .put("type", "pointer.absolute")
                    .put("x", x.coerceIn(0.0, 1.0))
                    .put("y", y.coerceIn(0.0, 1.0))
                    .put("geometry_epoch", geometryEpoch),
            )
        }
    }

    fun sendPointerButton(pressed: Boolean) {
        if (!inputAllowed) return
        commandExecutor.execute {
            sendInputEvent(
                JSONObject()
                    .put("type", "pointer.button")
                    .put("button", "primary")
                    .put("pressed", pressed),
            )
        }
    }

    fun sendText(text: String) {
        if (!inputAllowed || text.isEmpty()) return
        commandExecutor.execute {
            sendInputEvent(JSONObject().put("type", "text.commit").put("text", text))
        }
    }

    private fun sendInputEvent(event: JSONObject) {
        if (!sessionActive) return
        try {
            session?.sendSecure(
                JSONObject()
                    .put("type", "input.event")
                    .put("v", 1)
                    .put("session_id", sessionId)
                    .put("event", event),
            )
        } catch (error: Throwable) {
            publish("Input failed: ${error.message ?: "transport error"}")
        }
    }

    fun detachSurface() {
        decoder.detachSurface()
    }

    fun invoke(action: HostAction) {
        commandExecutor.execute {
            if (!sessionActive || !action.selectable) return@execute
            try {
                session?.sendSecure(
                    JSONObject()
                        .put("type", "action.invoke")
                        .put("v", 1)
                        .put("id", "android-${System.nanoTime()}")
                        .put("connection_epoch", capabilityConnectionEpoch)
                        .put("session_id", sessionId)
                        .put("grant_epoch", capabilityGrantEpoch)
                        .put("target_id", capabilityTargetId)
                        .put("target_generation", capabilityTargetGeneration)
                        .put("capability_revision", capabilityRevision)
                        .put("action", action.id)
                        .put("schema_version", 1)
                        .put("parameters", JSONObject().apply {
                            if (action.id == "window.maximize.set") {
                                put("value", action.label == "Maximize")
                            }
                        }),
                )
            } catch (error: Throwable) {
                publish("Action failed: ${error.message ?: "transport error"}")
            }
        }
    }

    fun close() {
        sessionActive = false
        mediaActive = false
        session = null
        mediaSession = null
        closeSocket(socket)
        closeSocket(mediaSocket)
        socket = null
        mediaSocket = null
        publishConnected(false)
        publishMediaActive(false)
        publishCatalog(emptyList())
        publishCapabilities(emptyList())
    }

    private fun select(target: JSONObject) {
        commandExecutor.execute {
            if (!sessionActive) return@execute
            try {
                session?.sendSecure(
                    JSONObject()
                        .put("type", "stream.select")
                        .put("v", 1)
                        .put("session_id", sessionId)
                        .put("target", target),
                )
            } catch (error: Throwable) {
                publish("Stream selection failed: ${error.message ?: "transport error"}")
            }
        }
    }

    private fun handle(message: JSONObject) {
        when (message.getString("type")) {
            "capabilities.snapshot" -> {
                capabilityConnectionEpoch = message.getString("connection_epoch")
                capabilityGrantEpoch = message.getString("grant_epoch")
                capabilityTargetId = message.getString("target_id")
                capabilityTargetGeneration = message.getString("target_generation")
                capabilityRevision = message.getLong("capability_revision")
                publishCapabilities(parseActions(message.getJSONArray("actions")))
            }
            "catalog.snapshot" -> publishCatalog(parseCatalog(message.getJSONArray("entries")))
            "action.result" -> publishActionResult(message)
            "media.offer" -> {
                val offer = MediaOfferInfo(
                    port = message.getInt("port"),
                    pairingId = message.optString("pairing_id"),
                    expiresAt = message.optLong("expires_at"),
                )
                publish("Video endpoint on port ${offer.port}")
                startMedia(offer)
            }
            "stream.started" -> {
                geometryEpoch = message.optLong("geometry_epoch", 1)
                val width = message.optInt("width", 0)
                val height = message.optInt("height", 0)
                val encoder = message.optString("encoder", "h264")
                val target = message.optJSONObject("target")
                val label = when (target?.optString("kind")) {
                    "window" -> "window ${target.optLong("id")}"
                    "workspace" -> "workspace ${target.optLong("id")}"
                    else -> target?.optString("kind") ?: "stream"
                }
                publish("Streaming $label · $width×$height ($encoder)")
                onStreamTarget?.let { callback -> main.post { callback(label) } }
            }
            "stream.stopped" -> {
                stopMedia()
                publish("Stream stopped: ${message.optString("reason", "host stopped the stream")}")
            }
            "stream.error" -> {
                stopMedia()
                val text = "${message.optString("code", "stream_error")}: ${message.optString("message")}"
                publish(text)
                onStreamError?.let { callback -> main.post { callback(text) } }
            }
            "pong" -> Unit
            "session_close" -> {
                publish("Host closed the session: ${message.optString("reason")}")
                sessionActive = false
            }
            "error" -> throw IllegalStateException(
                message.optString("message", message.optString("code", "host error")),
            )
        }
    }

    private fun startMedia(offer: MediaOfferInfo) {
        stopMedia()
        mediaExecutor.execute {
            try {
                val sock = Socket()
                sock.connect(InetSocketAddress(hostAddress, offer.port), 12_000)
                mediaSocket = sock
                val media = VistaSession(
                    BufferedInputStream(sock.getInputStream()),
                    BufferedOutputStream(sock.getOutputStream()),
                    secret,
                    "vista-android-media",
                )
                mediaSession = media
                media.trace = { Log.d("VistaWire", "media: $it") }
                media.handshake(wantInput = false, encryptSessionOpen = false)
                mediaActive = true
                publishMediaActive(true)
                publish("Receiving video…")
                media.sendKeyframeRequest()
                var frames = 0L
                while (mediaActive && sessionActive) {
                    val record = media.readRecord()
                    decoder.submit(record)
                    frames++
                    if (frames % 30L == 0L) {
                        publishVideoStats(frames)
                    }
                }
            } catch (error: Throwable) {
                Log.e("VistaWire", "media connection failed", error)
                if (mediaActive) {
                    publish("Media ended: ${error.message ?: error.javaClass.simpleName}")
                }
            } finally {
                mediaActive = false
                mediaSession = null
                publishMediaActive(false)
                closeSocket(mediaSocket)
                mediaSocket = null
            }
        }
    }

    private fun stopMedia() {
        mediaActive = false
        closeSocket(mediaSocket)
        mediaSocket = null
        mediaSession = null
        publishMediaActive(false)
    }

    private fun parseCatalog(entries: JSONArray): List<HostWindow> = buildList {
        for (index in 0 until entries.length()) {
            val entry = entries.getJSONObject(index)
            add(
                HostWindow(
                    entry.getLong("window_id"),
                    entry.optString("title", "Window ${entry.getLong("window_id")}"),
                    entry.optString("app_id").takeIf { it.isNotEmpty() },
                ),
            )
        }
    }

    private fun parseActions(actions: JSONArray): List<HostAction> = buildList {
        for (index in 0 until actions.length()) {
            val action = actions.getJSONObject(index)
            val presentation = action.optJSONObject("presentation")
            add(
                HostAction(
                    action.getString("id"),
                    presentation?.optString("label", action.getString("id")) ?: action.getString("id"),
                    action.optBoolean("supported") &&
                        action.optBoolean("enabled") &&
                        action.optString("authorization") == "granted",
                    action.optString("reason").takeIf { it.isNotEmpty() },
                ),
            )
        }
    }

    private fun publish(value: String) = main.post { onState?.invoke(value) }
    private fun publishConnected(value: Boolean) = main.post { onConnected?.invoke(value) }
    private fun publishMediaActive(value: Boolean) = main.post { onMediaActive?.invoke(value) }
    private fun publishCatalog(value: List<HostWindow>) = main.post { onCatalog?.invoke(value) }
    private fun publishCapabilities(value: List<HostAction>) = main.post { onCapabilities?.invoke(value) }
    private fun publishActionResult(value: JSONObject) =
        main.post { onActionResult?.invoke(value.optString("status", "unknown")) }
    private fun publishVideoStats(frames: Long) = main.post { onVideoStats?.invoke(frames) }

    private fun closeSocket(target: Socket?) {
        try {
            target?.close()
        } catch (_: Throwable) {
        }
    }
}
