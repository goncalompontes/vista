package vista.module

import android.os.Handler
import android.os.Looper
import org.json.JSONArray
import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.SecureRandom
import java.util.concurrent.Executors
import javax.crypto.Cipher
import javax.crypto.Mac
import javax.crypto.spec.IvParameterSpec
import javax.crypto.spec.SecretKeySpec

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
                val secret = hex(secretHex)
                require(secret.size == 32) { "Pairing secret must be 64 hex characters" }
                val clientNonce = randomHex(32)
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
                val policyBits = permissionBits(policy)
                val transcript = "vista-pairing-v1\nversion=1.0\nclient_id=vista-android\nclient_nonce=$clientNonce\nhost_id=$hostId\nhost_nonce=$hostNonce\npairing_id=$pairingId\npolicy=$policyBits\n"
                sendPlain(
                    JSONObject()
                        .put("seq", clientSequence++)
                        .put("type", "pairing_request")
                        .put("pairing_id", pairingId)
                        .put("tag", hmac(secret, "client-confirm\n".toByteArray() + transcript.toByteArray()).toHex())
                )
                val accepted = readPlain()
                checkHostSequence(accepted)
                check(accepted.getString("type") == "pairing_accepted") { "pairing rejected" }
                sessionId = accepted.getString("session_id")
                val hostTag = hmac(
                    secret,
                    "host-confirm\n".toByteArray() + transcript.toByteArray() + "\nsession=".toByteArray() + sessionId.toByteArray(),
                )
                check(hostTag.contentEquals(hex(accepted.getString("tag")))) { "host authentication failed" }
                val keys = deriveKeys(secret, pairingId, transcript.toByteArray(), sessionId)
                sessionKeyOut = keys.copyOfRange(0, 32)
                sessionKeyIn = keys.copyOfRange(32, 64)
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
        val plaintext = message.toString().toByteArray()
        val nonce = nonce(outCounter++)
        val cipher = Cipher.getInstance("ChaCha20-Poly1305")
        cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(sessionKeyOut, "ChaCha20"), IvParameterSpec(nonce))
        cipher.updateAAD(aad(1))
        writeFrame(cipher.doFinal(plaintext))
    }

    private fun readSecure(): JSONObject {
        val sealed = readFrame()
        val nonce = nonce(inCounter++)
        val cipher = Cipher.getInstance("ChaCha20-Poly1305")
        cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(sessionKeyIn, "ChaCha20"), IvParameterSpec(nonce))
        cipher.updateAAD(aad(2))
        return JSONObject(String(cipher.doFinal(sealed), Charsets.UTF_8)).also { checkHostSequence(it) }
    }

    private fun writeFrame(bytes: ByteArray) {
        val out = output ?: error("not connected")
        out.write(ByteBuffer.allocate(4).order(ByteOrder.BIG_ENDIAN).putInt(bytes.size).array())
        out.write(bytes)
        out.flush()
    }

    private fun readFrame(): ByteArray {
        val source = input ?: error("not connected")
        val header = source.readNBytes(4)
        check(header.size == 4) { "host closed the connection" }
        val length = ByteBuffer.wrap(header).order(ByteOrder.BIG_ENDIAN).int
        check(length in 1..(64 * 1024 + 16)) { "invalid frame length" }
        val bytes = source.readNBytes(length)
        check(bytes.size == length) { "truncated frame" }
        return bytes
    }

    private fun checkHostSequence(message: JSONObject) {
        val received = message.getLong("seq")
        check(received == hostSequence) { "host sequence expected $hostSequence, got $received" }
        hostSequence++
    }

    private fun aad(direction: Int): ByteArray = "vista-record-v1".toByteArray() + sessionId.toByteArray() + direction.toByte()
    private fun nonce(counter: Long): ByteArray = ByteArray(12).also { ByteBuffer.wrap(it, 4, 8).order(ByteOrder.BIG_ENDIAN).putLong(counter) }

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
        private fun version() = JSONObject().put("major", 1).put("minor", 0)
        private fun permissionBits(policy: JSONObject): Int = (if (policy.optBoolean("video")) 1 else 0) or (if (policy.optBoolean("audio")) 2 else 0) or (if (policy.optBoolean("input")) 4 else 0) or (if (policy.optBoolean("clipboard")) 8 else 0)
        private fun randomHex(bytes: Int): String = ByteArray(bytes).also { SecureRandom().nextBytes(it) }.toHex()
        private fun hex(value: String): ByteArray { check(value.length % 2 == 0) ; return value.chunked(2).map { it.toInt(16).toByte() }.toByteArray() }
        private fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }
        private fun hmac(key: ByteArray, value: ByteArray): ByteArray = Mac.getInstance("HmacSHA256").run { init(SecretKeySpec(key, algorithm)); doFinal(value) }
        private fun deriveKeys(secret: ByteArray, pairingId: String, transcript: ByteArray, sessionId: String): ByteArray {
            val salt = hmac(pairingId.toByteArray(), secret)
            val info = "vista-session-v1\n$pairingId\n$sessionId\n".toByteArray() + transcript
            var previous = ByteArray(0)
            val output = ByteArray(64)
            var offset = 0
            for (counter in 1..2) {
                previous = hmac(salt, previous + info + counter.toByte())
                previous.copyInto(output, offset)
                offset += previous.size
            }
            return output
        }
    }
}
