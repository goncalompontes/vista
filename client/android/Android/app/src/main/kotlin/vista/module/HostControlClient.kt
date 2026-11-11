package vista.module

import android.os.Handler
import android.os.Looper
import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.Executors

data class HostWindow(val id: Long, val title: String, val appId: String?)
data class HostAction(val id: String, val label: String, val selectable: Boolean, val reason: String?)

class HostControlClient {
    private val executor = Executors.newSingleThreadExecutor()
    private val commandExecutor = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())
    private var socket: Socket? = null
    private var input: BufferedInputStream? = null
    private var output: BufferedOutputStream? = null
    private var clientSequence = 0L
    private var hostSequence = 0L
    private var sessionId = ""
    private var sessionKeyOut = ByteArray(0)
    private var sessionKeyIn = ByteArray(0)
    private var outCounter = 0L
    private var inCounter = 0L
    private var sessionActive = false
    private var capabilityConnectionEpoch = ""
    private var capabilityGrantEpoch = ""
    private var capabilityTargetId = ""
    private var capabilityTargetGeneration = ""
    private var capabilityRevision = 0L

    var onState: ((String) -> Unit)? = null
    var onCatalog: ((List<HostWindow>) -> Unit)? = null
    var onCapabilities: ((List<HostAction>) -> Unit)? = null
    var onActionResult: ((String) -> Unit)? = null

    fun connect(host: String, port: Int, secretHex: String) {
        close()
        executor.execute {
            try {
                publish("Connecting…")
                val secret = VistaWire.hexToBytes(secretHex)
                require(secret.size == VistaWire.PAIRING_SECRET_BYTES) { "Pairing secret must be 64 hex characters" }
                val clientNonce = VistaWire.randomHex(VistaWire.PAIRING_SECRET_BYTES)
                val sock = Socket()
                sock.connect(InetSocketAddress(host, port), 12_000)
                socket = sock
                input = BufferedInputStream(sock.getInputStream())
                output = BufferedOutputStream(sock.getOutputStream())
                sendPlain(
                    JSONObject()
                        .put("seq", clientSequence++)
                        .put("type", "client_hello")
                        .put("versions", JSONObject().put("min", version()).put("max", version()))
                        .put("client_id", "vista-android")
                        .put("client_nonce", clientNonce)
                )
                val hello = readPlain()
                check(hello.getString("type") == "server_hello") { "host did not send server hello" }
                checkHostSequence(hello)
                val hostId = hello.getString("host_id")
                val hostNonce = hello.getString("host_nonce")
                val pairingId = hello.getString("pairing_id")
                val policy = hello.getJSONObject("policy")
                val policyBits = VistaWire.permissionBits(
                    policy.optBoolean("video"),
                    policy.optBoolean("audio"),
                    policy.optBoolean("input"),
                    policy.optBoolean("clipboard"),
                )
                val transcript = VistaWire.transcript(
                    version = VistaWire.PROTOCOL_VERSION,
                    clientID = "vista-android",
                    clientNonce = clientNonce,
                    hostID = hostId,
                    hostNonce = hostNonce,
                    pairingID = pairingId,
                    policyBits = policyBits,
                )
                sendPlain(
                    JSONObject()
                        .put("seq", clientSequence++)
                        .put("type", "pairing_request")
                        .put("pairing_id", pairingId)
                        .put("tag", VistaWire.clientTag(secret, transcript).toHex())
                )
                val accepted = readPlain()
                checkHostSequence(accepted)
                check(accepted.getString("type") == "pairing_accepted") { "pairing rejected" }
                sessionId = accepted.getString("session_id")
                val expectedHostTag = VistaWire.hostTag(secret, transcript, sessionId)
                check(expectedHostTag.contentEquals(VistaWire.hexToBytes(accepted.getString("tag")))) {
                    "host authentication failed"
                }
                val keys = VistaWire.deriveSessionKeys(secret, pairingId, transcript, sessionId)
                sessionKeyOut = keys.copyOfRange(0, VistaWire.SESSION_KEY_BYTES)
                sessionKeyIn = keys.copyOfRange(VistaWire.SESSION_KEY_BYTES, VistaWire.SESSION_KEY_BYTES * 2)
                sendSecure(
                    JSONObject()
                        .put("seq", clientSequence++)
                        .put("type", "session_open")
                        .put("session_id", sessionId)
                        .put(
                            "permissions",
                            JSONObject()
                                .put("video", true)
                                .put("audio", false)
                                .put("input", false)
                                .put("clipboard", false),
                        )
                )
                val opened = readSecure()
                check(opened.getString("type") == "session_opened") { "session was not opened" }
                sessionActive = true
                publish("Connected to ${hello.optString("host_name", host)}")
                while (sessionActive) {
                    val message = readSecure()
                    when (message.getString("type")) {
                        "capabilities.snapshot" -> {
                            capabilityConnectionEpoch = message.getString("connection_epoch")
                            capabilityGrantEpoch = message.getString("grant_epoch")
                            capabilityTargetId = message.getString("target_id")
                            capabilityTargetGeneration = message.getString("target_generation")
                            capabilityRevision = message.getLong("capability_revision")
                            val actions = parseActions(message.getJSONArray("actions"))
                            publishCapabilities(actions)
                        }
                        "catalog.snapshot" -> publishCatalog(parseCatalog(message.getJSONArray("entries")))
                        "action.result" -> publishActionResult(message)
                        "pong" -> Unit
                        "error", "session_close" -> throw IllegalStateException(message.optString("message", message.optString("reason", "host closed")))
                    }
                }
            } catch (error: Throwable) {
                publish("Connection failed: ${error.message ?: error.javaClass.simpleName}")
                publishCatalog(emptyList())
                publishCapabilities(emptyList())
                closeSocket()
            }
        }
    }

    fun invoke(action: HostAction) {
        commandExecutor.execute {
            if (!sessionActive || !action.selectable) return@execute
            try {
                sendSecure(
                    JSONObject()
                        .put("seq", clientSequence++)
                        .put("type", "action.invoke")
                        .put("v", 1)
                        .put("id", "android-${clientSequence}")
                        .put("connection_epoch", capabilityConnectionEpoch)
                        .put("session_id", sessionId)
                        .put("grant_epoch", capabilityGrantEpoch)
                        .put("target_id", capabilityTargetId)
                        .put("target_generation", capabilityTargetGeneration)
                        .put("capability_revision", capabilityRevision)
                        .put("action", action.id)
                        .put("schema_version", 1)
                        .put("parameters", JSONObject().apply {
                            if (action.id == "window.maximize.set") put("value", action.label == "Maximize")
                        })
                )
            } catch (error: Throwable) {
                publish("Action failed: ${error.message ?: "transport error"}")
            }
        }
    }

    fun close() {
        sessionActive = false
        commandExecutor.execute { closeSocket() }
        publishCatalog(emptyList())
        publishCapabilities(emptyList())
    }

    private fun sendPlain(message: JSONObject) = writeFrame(message.toString().toByteArray())

    private fun readPlain(): JSONObject = JSONObject(String(readFrame(), Charsets.UTF_8))

    private fun sendSecure(message: JSONObject) {
        val sealed = VistaWire.seal(
            key = sessionKeyOut,
            sessionID = sessionId,
            direction = VistaWire.DIRECTION_CLIENT_TO_HOST,
            counter = outCounter++,
            plaintext = message.toString().toByteArray(),
        )
        writeFrame(sealed)
    }

    private fun readSecure(): JSONObject {
        val plaintext = VistaWire.open(
            key = sessionKeyIn,
            sessionID = sessionId,
            direction = VistaWire.DIRECTION_HOST_TO_CLIENT,
            counter = inCounter++,
            sealed = readFrame(),
        )
        return JSONObject(String(plaintext, Charsets.UTF_8)).also { checkHostSequence(it) }
    }

    private fun writeFrame(bytes: ByteArray) {
        VistaWire.writeFrame(output ?: error("not connected"), bytes)
    }

    private fun readFrame(): ByteArray = VistaWire.readFrame(input ?: error("not connected"))

    private fun checkHostSequence(message: JSONObject) {
        val received = message.getLong("seq")
        check(received == hostSequence) { "host sequence expected $hostSequence, got $received" }
        hostSequence++
    }

    private fun parseCatalog(entries: JSONArray): List<HostWindow> = buildList {
        for (index in 0 until entries.length()) {
            val entry = entries.getJSONObject(index)
            add(HostWindow(entry.getLong("window_id"), entry.optString("title", "Window ${entry.getLong("window_id")}"), entry.optString("app_id").takeIf { it.isNotEmpty() }))
        }
    }

    private fun parseActions(actions: JSONArray): List<HostAction> = buildList {
        for (index in 0 until actions.length()) {
            val action = actions.getJSONObject(index)
            val presentation = action.optJSONObject("presentation")
            add(HostAction(action.getString("id"), presentation?.optString("label", action.getString("id")) ?: action.getString("id"), action.optBoolean("supported") && action.optBoolean("enabled") && action.optString("authorization") == "granted", action.optString("reason").takeIf { it.isNotEmpty() }))
        }
    }

    private fun publish(value: String) = main.post { onState?.invoke(value) }
    private fun publishCatalog(value: List<HostWindow>) = main.post { onCatalog?.invoke(value) }
    private fun publishCapabilities(value: List<HostAction>) = main.post { onCapabilities?.invoke(value) }
    private fun publishActionResult(value: JSONObject) = main.post { onActionResult?.invoke(value.optString("status", "unknown")) }
    private fun closeSocket() { try { socket?.close() } catch (_: Throwable) {} ; socket = null; input = null; output = null }

    companion object {
        private fun version() = JSONObject()
            .put("major", VistaWire.PROTOCOL_MAJOR)
            .put("minor", VistaWire.PROTOCOL_MINOR)
    }
}
