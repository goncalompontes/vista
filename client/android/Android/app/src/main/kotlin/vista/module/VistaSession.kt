package vista.module

import org.json.JSONObject
import java.io.InputStream
import java.io.OutputStream

/**
 * One authenticated Vista v1 session over an already-connected socket.
 *
 * The control connection encrypts `session_open` after `pairing_accepted`
 * (the host enables its record ciphers at that point). The media connection
 * uses the same handshake messages in plaintext and only encrypts media
 * records, matching `media_stream::client_handshake` in the host.
 */
class VistaSession(
    private val input: InputStream,
    private val output: OutputStream,
    private val secret: ByteArray,
    private val clientID: String,
) {
    var sessionId: String = ""
        private set

    /** Permissions the host actually granted in `server_hello`. */
    var policyVideo: Boolean = false
        private set
    var policyInput: Boolean = false
        private set

    private var clientToHost = ByteArray(0)
    private var hostToClient = ByteArray(0)
    private var clientSequence = 0L
    private var hostSequence = 0L
    private var outCounter = 0L
    private var inCounter = 0L
    private val sendLock = Any()

    /** Optional diagnostic sink; the Android client logs frame types here. */
    var trace: ((String) -> Unit)? = null

    fun handshake(wantInput: Boolean, encryptSessionOpen: Boolean) {
        require(secret.size == VistaWire.PAIRING_SECRET_BYTES) { "pairing secret must be 32 bytes" }
        val clientNonce = VistaWire.randomHex(VistaWire.PAIRING_SECRET_BYTES)
        sendPlain(
            JSONObject()
                .put("type", "client_hello")
                .put("versions", JSONObject().put("min", versionObject()).put("max", versionObject()))
                .put("client_id", clientID)
                .put("client_nonce", clientNonce),
        )
        val hello = readPlain()
        check(hello.getString("type") == "server_hello") { "host did not send server hello" }
        val hostId = hello.getString("host_id")
        val hostNonce = hello.getString("host_nonce")
        val pairingId = hello.getString("pairing_id")
        val policy = hello.getJSONObject("policy")
        policyVideo = policy.optBoolean("video")
        policyInput = policy.optBoolean("input")
        val grantedInput = wantInput && policyInput
        val policyBits = VistaWire.permissionBits(
            policyVideo,
            policy.optBoolean("audio"),
            policyInput,
            policy.optBoolean("clipboard"),
        )
        val transcript = VistaWire.transcript(
            version = version(),
            clientID = clientID,
            clientNonce = clientNonce,
            hostID = hostId,
            hostNonce = hostNonce,
            pairingID = pairingId,
            policyBits = policyBits,
        )
        sendPlain(
            JSONObject()
                .put("type", "pairing_request")
                .put("pairing_id", pairingId)
                .put("tag", VistaWire.clientTag(secret, transcript).toHex()),
        )
        val accepted = readPlain()
        check(accepted.getString("type") == "pairing_accepted") { "pairing rejected" }
        sessionId = accepted.getString("session_id")
        val expectedHostTag = VistaWire.hostTag(secret, transcript, sessionId)
        check(expectedHostTag.contentEquals(VistaWire.hexToBytes(accepted.getString("tag")))) {
            "host authentication failed"
        }
        val keys = VistaWire.deriveSessionKeys(secret, pairingId, transcript, sessionId)
        clientToHost = keys.copyOfRange(0, VistaWire.SESSION_KEY_BYTES)
        hostToClient = keys.copyOfRange(VistaWire.SESSION_KEY_BYTES, VistaWire.SESSION_KEY_BYTES * 2)

        val open = JSONObject()
            .put("type", "session_open")
            .put("session_id", sessionId)
            .put(
                "permissions",
                JSONObject()
                    .put("video", policyVideo)
                    .put("audio", false)
                    .put("input", grantedInput)
                    .put("clipboard", false),
            )
        if (encryptSessionOpen) sendSecure(open) else sendPlain(open)
        val opened = if (encryptSessionOpen) readSecure() else readPlain()
        check(opened.getString("type") == "session_opened") { "session was not opened" }
    }

    fun sendSecure(message: JSONObject) {
        synchronized(sendLock) {
            val sealed = VistaWire.seal(
                key = clientToHost,
                sessionID = sessionId,
                direction = VistaWire.DIRECTION_CLIENT_TO_HOST,
                counter = outCounter++,
                plaintext = message.put("seq", clientSequence++).toString().toByteArray(),
            )
            VistaWire.writeFrame(output, sealed)
            trace?.invoke("send secure ${message.optString("type")} seq=${clientSequence - 1}")
        }
    }

    fun readSecure(): JSONObject {
        val sealed = VistaWire.readFrame(input)
        val plaintext = VistaWire.open(
            key = hostToClient,
            sessionID = sessionId,
            direction = VistaWire.DIRECTION_HOST_TO_CLIENT,
            counter = inCounter++,
            sealed = sealed,
        )
        return JSONObject(String(plaintext, Charsets.UTF_8)).also {
            checkHostSequence(it)
            trace?.invoke("recv secure ${it.optString("type")} seq=${it.optLong("seq")}")
        }
    }

    /** Read one authenticated media record on the media connection. */
    fun readRecord(): MediaRecord {
        val sealed = VistaWire.readFrame(input, VistaWire.MAX_MEDIA_RECORD_BYTES)
        val plaintext = VistaWire.open(
            key = hostToClient,
            sessionID = sessionId,
            direction = VistaWire.DIRECTION_HOST_TO_CLIENT,
            counter = inCounter++,
            sealed = sealed,
        )
        return MediaRecord.parse(plaintext)
    }

    fun sendKeyframeRequest() {
        sendSecure(JSONObject().put("type", "keyframe_request").put("session_id", sessionId))
    }

    private fun sendPlain(message: JSONObject) {
        synchronized(sendLock) {
            val payload = message.put("seq", clientSequence++).toString().toByteArray()
            trace?.invoke("send plain ${message.optString("type")} bytes=${payload.size}")
            VistaWire.writeFrame(output, payload)
        }
    }

    private fun readPlain(): JSONObject =
        JSONObject(String(VistaWire.readFrame(input), Charsets.UTF_8)).also {
            checkHostSequence(it)
            trace?.invoke("recv plain ${it.optString("type")}")
        }

    private fun checkHostSequence(message: JSONObject) {
        val received = message.getLong("seq")
        check(received == hostSequence) { "host sequence expected $hostSequence, got $received" }
        hostSequence++
    }

    private fun version(): String = VistaWire.PROTOCOL_VERSION

    private fun versionObject(): JSONObject = JSONObject()
        .put("major", VistaWire.PROTOCOL_MAJOR)
        .put("minor", VistaWire.PROTOCOL_MINOR)
}
