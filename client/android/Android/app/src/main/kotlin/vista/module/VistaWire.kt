package vista.module

import java.io.InputStream
import java.io.OutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.Mac
import javax.crypto.spec.IvParameterSpec
import javax.crypto.spec.SecretKeySpec

/**
 * Wire primitives for the Vista v1 control channel.
 *
 * This mirrors `host/src/protocol.rs` and `host/src/transport.rs` and is kept
 * free of Android types so the JVM unit tests can check it against the shared
 * `docs/fixtures/pairing-v1.json` vector. Any change to the byte layout must
 * land in the Rust host, the Swift core and this file together.
 */
object VistaWire {
    const val PROTOCOL_MAJOR = 1
    const val PROTOCOL_MINOR = 0
    const val PROTOCOL_VERSION = "$PROTOCOL_MAJOR.$PROTOCOL_MINOR"

    const val PAIRING_SECRET_BYTES = 32
    const val SESSION_KEY_BYTES = 32
    const val MAX_CONTROL_FRAME_BYTES = 64 * 1024
    const val AEAD_TAG_BYTES = 16

    /** Mirrors `media_transport.rs`: 8-byte PTS, one flags byte, then the AU. */
    const val MEDIA_HEADER_BYTES = 9
    const val MAX_MEDIA_FRAME_BYTES = 16 * 1024 * 1024
    const val MAX_MEDIA_RECORD_BYTES = MEDIA_HEADER_BYTES + MAX_MEDIA_FRAME_BYTES + AEAD_TAG_BYTES

    const val DIRECTION_CLIENT_TO_HOST = 1
    const val DIRECTION_HOST_TO_CLIENT = 2

    private const val RECORD_LABEL = "vista-record-v1"
    private const val SESSION_LABEL = "vista-session-v1"
    private const val CLIENT_CONFIRM_LABEL = "client-confirm\n"
    private const val HOST_CONFIRM_LABEL = "host-confirm\n"

    /** Pairing transcript exactly as the host builds it in `protocol.rs`. */
    fun transcript(
        version: String,
        clientID: String,
        clientNonce: String,
        hostID: String,
        hostNonce: String,
        pairingID: String,
        policyBits: Int,
    ): ByteArray = (
        "vista-pairing-v1\n" +
            "version=$version\n" +
            "client_id=$clientID\n" +
            "client_nonce=$clientNonce\n" +
            "host_id=$hostID\n" +
            "host_nonce=$hostNonce\n" +
            "pairing_id=$pairingID\n" +
            "policy=$policyBits\n"
        ).toByteArray(Charsets.UTF_8)

    fun clientTag(secret: ByteArray, transcript: ByteArray): ByteArray =
        hmacSha256(secret, CLIENT_CONFIRM_LABEL.toByteArray(Charsets.UTF_8) + transcript)

    fun hostTag(secret: ByteArray, transcript: ByteArray, sessionID: String): ByteArray =
        hmacSha256(
            secret,
            HOST_CONFIRM_LABEL.toByteArray(Charsets.UTF_8) +
                transcript +
                "\nsession=".toByteArray(Charsets.UTF_8) +
                sessionID.toByteArray(Charsets.UTF_8),
        )

    /**
     * HKDF-SHA256 with salt = pairing id and the pairing transcript appended to
     * the info string; returns 64 bytes (client→host, then host→client).
     */
    fun deriveSessionKeys(
        secret: ByteArray,
        pairingID: String,
        transcript: ByteArray,
        sessionID: String,
    ): ByteArray {
        val salt = hmacSha256(pairingID.toByteArray(Charsets.UTF_8), secret)
        val info = (
            "$SESSION_LABEL\n" +
                "$pairingID\n" +
                "$sessionID\n"
            ).toByteArray(Charsets.UTF_8) + transcript
        val output = ByteArray(SESSION_KEY_BYTES * 2)
        var previous = ByteArray(0)
        var offset = 0
        for (counter in 1..2) {
            previous = hmacSha256(salt, previous + info + counter.toByte())
            previous.copyInto(output, offset)
            offset += previous.size
        }
        return output
    }

    fun nonce(counter: Long): ByteArray =
        ByteArray(12).also { ByteBuffer.wrap(it, 4, 8).order(ByteOrder.BIG_ENDIAN).putLong(counter) }

    fun additionalData(sessionID: String, direction: Int): ByteArray =
        RECORD_LABEL.toByteArray(Charsets.UTF_8) + sessionID.toByteArray(Charsets.UTF_8) + direction.toByte()

    fun seal(
        key: ByteArray,
        sessionID: String,
        direction: Int,
        counter: Long,
        plaintext: ByteArray,
    ): ByteArray {
        val cipher = Cipher.getInstance("ChaCha20-Poly1305")
        cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "ChaCha20"), IvParameterSpec(nonce(counter)))
        cipher.updateAAD(additionalData(sessionID, direction))
        return cipher.doFinal(plaintext)
    }

    fun open(
        key: ByteArray,
        sessionID: String,
        direction: Int,
        counter: Long,
        sealed: ByteArray,
    ): ByteArray {
        val cipher = Cipher.getInstance("ChaCha20-Poly1305")
        cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(key, "ChaCha20"), IvParameterSpec(nonce(counter)))
        cipher.updateAAD(additionalData(sessionID, direction))
        return cipher.doFinal(sealed)
    }

    fun writeFrame(output: OutputStream, payload: ByteArray) {
        output.write(ByteBuffer.allocate(4).order(ByteOrder.BIG_ENDIAN).putInt(payload.size).array())
        output.write(payload)
        output.flush()
    }

    fun readFrame(input: InputStream, maxBytes: Int = MAX_CONTROL_FRAME_BYTES + AEAD_TAG_BYTES): ByteArray {
        val header = input.readNBytes(4)
        check(header.size == 4) { "host closed the connection" }
        val length = ByteBuffer.wrap(header).order(ByteOrder.BIG_ENDIAN).int
        check(length in 1..maxBytes) { "invalid frame length" }
        val bytes = input.readNBytes(length)
        check(bytes.size == length) { "truncated frame" }
        return bytes
    }

    fun hmacSha256(key: ByteArray, value: ByteArray): ByteArray =
        Mac.getInstance("HmacSHA256").run {
            init(SecretKeySpec(key, "HmacSHA256"))
            doFinal(value)
        }

    fun randomHex(bytes: Int): String =
        ByteArray(bytes).also { SecureRandom().nextBytes(it) }.toHex()

    fun hexToBytes(value: String): ByteArray {
        check(value.length % 2 == 0) { "hex value must have an even length" }
        return ByteArray(value.length / 2) { index ->
            value.substring(index * 2, index * 2 + 2).toInt(16).toByte()
        }
    }

    fun permissionBits(video: Boolean, audio: Boolean, input: Boolean, clipboard: Boolean): Int =
        (if (video) 1 else 0) or
            (if (audio) 2 else 0) or
            (if (input) 4 else 0) or
            (if (clipboard) 8 else 0)
}

/** Lower-case hex used by the pairing tags and the shared JSON fixtures. */
fun ByteArray.toHex(): String = joinToString("") { byte -> "%02x".format(byte) }
