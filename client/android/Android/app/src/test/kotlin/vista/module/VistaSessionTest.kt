package vista.module

import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.InputStream
import java.io.OutputStream
import java.io.PipedInputStream
import java.io.PipedOutputStream
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * Runs [VistaSession] against a scripted host that uses the same wire
 * primitives. This covers both handshake modes: the control connection
 * encrypts `session_open`, the media connection keeps the handshake plaintext
 * and encrypts access units.
 */
class VistaSessionTest {
    private val secretHex = "0707070707070707070707070707070707070707070707070707070707070707"
    private val secret = VistaWire.hexToBytes(secretHex)

    @Test
    fun controlHandshakeEncryptsSessionOpenAndReadsControlMessages() {
        val clientToHost = PipedOutputStream()
        val hostIn = PipedInputStream(clientToHost)
        val hostToClient = PipedOutputStream()
        val clientIn = PipedInputStream(hostToClient)
        val pool = Executors.newSingleThreadExecutor()
        try {
            val host = pool.submit {
                val scripted = ScriptedHost(hostIn, hostToClient, secret, encryptSessionOpen = true, expectInput = true)
                scripted.handshake()
                scripted.sendControl(JSONObject().put("type", "ping").put("nonce", 7))
            }
            val session = VistaSession(clientIn, clientToHost, secret, "vista-android")
            session.handshake(wantInput = true, encryptSessionOpen = true)
            val message = session.readSecure()
            assertEquals("ping", message.getString("type"))
            assertEquals(7, message.getLong("nonce"))
            host.get(5, TimeUnit.SECONDS)
        } finally {
            pool.shutdownNow()
        }
    }

    @Test
    fun mediaHandshakeStaysPlaintextAndReadsRecords() {
        val clientToHost = PipedOutputStream()
        val hostIn = PipedInputStream(clientToHost)
        val hostToClient = PipedOutputStream()
        val clientIn = PipedInputStream(hostToClient)
        val pool = Executors.newSingleThreadExecutor()
        try {
            val payload = byteArrayOf(0, 0, 0, 1, 0x65, 0x11, 0x22)
            val host = pool.submit {
                val scripted = ScriptedHost(hostIn, hostToClient, secret, encryptSessionOpen = false, expectInput = false)
                scripted.handshake()
                scripted.sendRecord(MediaRecord(1_500_000_000L, keyframe = true, discontinuity = false, bytes = payload))
            }
            val session = VistaSession(clientIn, clientToHost, secret, "vista-android-media")
            session.handshake(wantInput = false, encryptSessionOpen = false)
            val record = session.readRecord()
            assertEquals(1_500_000_000L, record.ptsNanos)
            assertTrue(record.keyframe)
            assertFalse(record.discontinuity)
            assertArrayEquals(payload, record.bytes)
            host.get(5, TimeUnit.SECONDS)
        } finally {
            pool.shutdownNow()
        }
    }

    @Test
    fun sessionOpenedPermissionsAreAuthoritativeAndMissingGrantsFailClosed() {
        for (permissions in listOf(JSONObject().put("video", true).put("input", false), null)) {
            val session = negotiatedSession(permissions = permissions)
            assertFalse(session.policyInput)
            assertEquals(permissions != null, session.policyVideo)
        }
    }

    @Test
    fun sessionOpenedCannotReplaceTheAuthenticatedSessionIdentity() {
        assertThrows(IllegalStateException::class.java) { negotiatedSession(wrongSession = true) }
    }

    private fun negotiatedSession(permissions: JSONObject? = JSONObject().put("input", true), wrongSession: Boolean = false): VistaSession {
        val clientToHost = PipedOutputStream()
        val hostIn = PipedInputStream(clientToHost)
        val hostToClient = PipedOutputStream()
        val clientIn = PipedInputStream(hostToClient)
        val pool = Executors.newSingleThreadExecutor()
        try {
            val host = pool.submit {
                ScriptedHost(hostIn, hostToClient, secret, encryptSessionOpen = true, expectInput = true,
                    openedPermissions = permissions, wrongSession = wrongSession).handshake()
            }
            val session = VistaSession(clientIn, clientToHost, secret, "vista-android")
            try { session.handshake(wantInput = true, encryptSessionOpen = true) }
            finally { host.get(5, TimeUnit.SECONDS) }
            return session
        } finally {
            pool.shutdownNow()
            clientIn.close()
            clientToHost.close()
            hostIn.close()
            hostToClient.close()
        }
    }

    private class ScriptedHost(
        private val input: InputStream,
        private val output: OutputStream,
        private val secret: ByteArray,
        private val encryptSessionOpen: Boolean,
        private val expectInput: Boolean,
        private val openedPermissions: JSONObject? = JSONObject().put("video", true).put("input", expectInput),
        private val wrongSession: Boolean = false,
    ) {
        private var hostOutSequence = 0L
        private var inCounter = 0L
        private var outCounter = 0L
        private lateinit var sessionId: String
        private lateinit var clientToHost: ByteArray
        private lateinit var hostToClient: ByteArray

        fun handshake() {
            val hello = readPlain()
            assertEquals(1, hello.getJSONObject("versions").getJSONObject("min").getInt("major"))
            assertEquals(0, hello.getJSONObject("versions").getJSONObject("min").getInt("minor"))
            val clientID = hello.getString("client_id")
            val clientNonce = hello.getString("client_nonce")
            val hostNonce = VistaWire.randomHex(32)
            val pairingId = VistaWire.randomHex(16)
            writePlain(
                JSONObject()
                    .put("type", "server_hello")
                    .put("host_id", "scripted-host")
                    .put("host_nonce", hostNonce)
                    .put("pairing_id", pairingId)
                    .put("policy", JSONObject().put("video", true).put("audio", false).put("input", true).put("clipboard", false)),
            )
            val request = readPlain()
            val transcript = VistaWire.transcript(
                "1.0", clientID, clientNonce, "scripted-host", hostNonce, pairingId, 1 or 4,
            )
            assertEquals(VistaWire.clientTag(secret, transcript).toHex(), request.getString("tag"))
            sessionId = VistaWire.randomHex(16)
            writePlain(
                JSONObject()
                    .put("type", "pairing_accepted")
                    .put("session_id", sessionId)
                    .put("tag", VistaWire.hostTag(secret, transcript, sessionId).toHex()),
            )
            val keys = VistaWire.deriveSessionKeys(secret, pairingId, transcript, sessionId)
            clientToHost = keys.copyOfRange(0, 32)
            hostToClient = keys.copyOfRange(32, 64)
            val open = if (encryptSessionOpen) readSecure() else readPlain()
            assertEquals("session_open", open.getString("type"))
            assertEquals(expectInput, open.getJSONObject("permissions").getBoolean("input"))
            val opened = JSONObject().put("type", "session_opened")
                .put("session_id", if (wrongSession) "different-session" else sessionId)
            openedPermissions?.let { opened.put("permissions", it) }
            if (encryptSessionOpen) writeSecure(opened) else writePlain(opened)
        }

        fun sendControl(message: JSONObject) {
            writeSecure(message)
        }

        fun sendRecord(record: MediaRecord) {
            val plaintext = java.io.ByteArrayOutputStream().apply {
                write(java.nio.ByteBuffer.allocate(8).putLong(record.ptsNanos).array())
                write((if (record.keyframe) 1 else 0) or (if (record.discontinuity) 2 else 0))
                write(record.bytes)
            }.toByteArray()
            VistaWire.writeFrame(
                output,
                VistaWire.seal(
                    hostToClient,
                    sessionId,
                    VistaWire.DIRECTION_HOST_TO_CLIENT,
                    outCounter++,
                    plaintext,
                ),
            )
        }

        private fun readPlain(): JSONObject =
            JSONObject(String(VistaWire.readFrame(input), Charsets.UTF_8))

        private fun writePlain(message: JSONObject) {
            VistaWire.writeFrame(output, message.put("seq", hostOutSequence++).toString().toByteArray())
        }

        private fun readSecure(): JSONObject {
            val sealed = VistaWire.readFrame(input)
            val plaintext = VistaWire.open(
                clientToHost,
                sessionId,
                VistaWire.DIRECTION_CLIENT_TO_HOST,
                inCounter++,
                sealed,
            )
            return JSONObject(String(plaintext, Charsets.UTF_8))
        }

        private fun writeSecure(message: JSONObject) {
            VistaWire.writeFrame(
                output,
                VistaWire.seal(
                    hostToClient,
                    sessionId,
                    VistaWire.DIRECTION_HOST_TO_CLIENT,
                    outCounter++,
                    message.put("seq", hostOutSequence++).toString().toByteArray(),
                ),
            )
        }
    }
}
