package vista.module

import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.file.Files
import java.nio.file.Path

/**
 * Checks the Android wire core against the same cross-language vector the Rust
 * host and Swift `VistaCore` use (`docs/fixtures/pairing-v1.json`), so the third
 * client cannot silently drift from the protocol.
 */
class VistaWireTest {
    private val vector: JSONObject by lazy { JSONObject(String(Files.readAllBytes(findFixture()), Charsets.UTF_8)) }

    private fun findFixture(): Path {
        var directory: Path? = Path.of(System.getProperty("user.dir")).toAbsolutePath()
        while (directory != null) {
            val candidate = directory.resolve("docs/fixtures/pairing-v1.json")
            if (Files.isRegularFile(candidate)) return candidate
            directory = directory.parent
        }
        error("docs/fixtures/pairing-v1.json not found above ${System.getProperty("user.dir")}")
    }

    private fun hex(json: JSONObject, key: String): ByteArray = VistaWire.hexToBytes(json.getString(key))

    @Test
    fun pairingTranscriptMatchesSharedVector() {
        val transcript = VistaWire.transcript(
            version = "${vector.getJSONObject("selected").getInt("major")}.${vector.getJSONObject("selected").getInt("minor")}",
            clientID = vector.getString("client_id"),
            clientNonce = vector.getString("client_nonce"),
            hostID = vector.getString("host_id"),
            hostNonce = vector.getString("host_nonce"),
            pairingID = vector.getString("pairing_id"),
            policyBits = VistaWire.permissionBits(
                video = vector.getJSONObject("policy").getBoolean("video"),
                audio = vector.getJSONObject("policy").getBoolean("audio"),
                input = vector.getJSONObject("policy").getBoolean("input"),
                clipboard = vector.getJSONObject("policy").getBoolean("clipboard"),
            ),
        )
        assertEquals(vector.getString("transcript_hex"), transcript.toHex())
    }

    @Test
    fun pairingTagsMatchSharedVector() {
        val secret = hex(vector, "secret_hex")
        val transcript = hex(vector, "transcript_hex")
        assertEquals(vector.getString("client_tag"), VistaWire.clientTag(secret, transcript).toHex())
        assertEquals(
            vector.getString("host_tag"),
            VistaWire.hostTag(secret, transcript, vector.getString("session_id")).toHex(),
        )
    }

    @Test
    fun sessionKeysMatchSharedVector() {
        val keys = VistaWire.deriveSessionKeys(
            secret = hex(vector, "secret_hex"),
            pairingID = vector.getString("pairing_id"),
            transcript = hex(vector, "transcript_hex"),
            sessionID = vector.getString("session_id"),
        )
        assertEquals(VistaWire.SESSION_KEY_BYTES * 2, keys.size)
        assertEquals(vector.getString("client_to_host"), keys.copyOfRange(0, 32).toHex())
        assertEquals(vector.getString("host_to_client"), keys.copyOfRange(32, 64).toHex())
    }

    @Test
    fun recordCiphertextMatchesSharedVector() {
        val record = vector.getJSONObject("record")
        val sealed = VistaWire.seal(
            key = VistaWire.hexToBytes(record.getString("key_hex")),
            sessionID = record.getString("session_id"),
            direction = direction(record.getString("direction")),
            counter = record.getLong("counter"),
            plaintext = VistaWire.hexToBytes(record.getString("plaintext_hex")),
        )
        assertEquals(record.getString("ciphertext_hex"), sealed.toHex())
    }

    @Test
    fun recordRoundTripsAndRejectsTampering() {
        val key = ByteArray(32) { 0x21 }
        val plaintext = "vista".toByteArray()
        val sealed = VistaWire.seal(key, "session", VistaWire.DIRECTION_CLIENT_TO_HOST, 7, plaintext)
        assertArrayEquals(
            plaintext,
            VistaWire.open(key, "session", VistaWire.DIRECTION_CLIENT_TO_HOST, 7, sealed),
        )

        sealed[0] = (sealed[0].toInt() xor 0x01).toByte()
        assertThrows(javax.crypto.AEADBadTagException::class.java) {
            VistaWire.open(key, "session", VistaWire.DIRECTION_CLIENT_TO_HOST, 7, sealed)
        }
    }

    @Test
    fun frameRoundTripsAndRejectsOversizedLengths() {
        val output = ByteArrayOutputStream()
        val payload = byteArrayOf(1, 2, 3, 4)
        VistaWire.writeFrame(output, payload)
        assertArrayEquals(payload, VistaWire.readFrame(ByteArrayInputStream(output.toByteArray())))

        val oversized = ByteBuffer.allocate(4).order(ByteOrder.BIG_ENDIAN)
            .putInt(VistaWire.MAX_CONTROL_FRAME_BYTES + VistaWire.AEAD_TAG_BYTES + 1)
            .array()
        assertThrows(IllegalStateException::class.java) {
            VistaWire.readFrame(ByteArrayInputStream(oversized))
        }
    }

    private fun direction(value: String): Int = when (value) {
        "client_to_host" -> VistaWire.DIRECTION_CLIENT_TO_HOST
        "host_to_client" -> VistaWire.DIRECTION_HOST_TO_CLIENT
        else -> error("unknown direction $value")
    }

    @Test
    fun framingAcceptsShortReadsAndRejectsTruncatedHeadersAndBodies() {
        val framed = byteArrayOf(0, 0, 0, 3, 10, 20, 30)
        val shortReads = object : ByteArrayInputStream(framed) {
            override fun read(buffer: ByteArray, offset: Int, length: Int): Int =
                super.read(buffer, offset, minOf(length, 1))
        }
        assertArrayEquals(byteArrayOf(10, 20, 30), VistaWire.readFrame(shortReads))
        for (length in 0 until framed.size) {
            assertThrows(IllegalStateException::class.java) {
                VistaWire.readFrame(ByteArrayInputStream(framed.copyOf(length)))
            }
        }
    }
}
