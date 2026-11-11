package vista.module

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer

class MediaWireTest {
    @Test
    fun parsesTimestampFlagsAndPayload() {
        val payload = byteArrayOf(0, 0, 0, 1, 0x65, 0x01, 0x02)
        val plaintext = ByteArrayOutputStream().apply {
            write(ByteBuffer.allocate(8).putLong(9_000_000_000L).array())
            write(0b0000_0011)
            write(payload)
        }.toByteArray()
        val record = MediaRecord.parse(plaintext)
        assertTrue(record.ptsNanos == 9_000_000_000L)
        assertTrue(record.keyframe)
        assertTrue(record.discontinuity)
        assertArrayEquals(payload, record.bytes)
    }

    @Test
    fun rejectsUnknownFlagsEmptyFramesAndTruncatedHeaders() {
        val header = ByteBuffer.allocate(8).putLong(1L).array()
        assertThrows(IllegalStateException::class.java) {
            MediaRecord.parse(header + byteArrayOf(0b0000_0100, 1))
        }
        assertThrows(IllegalStateException::class.java) {
            MediaRecord.parse(header + byteArrayOf(0))
        }
        assertThrows(IllegalStateException::class.java) {
            MediaRecord.parse(byteArrayOf(0, 1, 2))
        }
    }

    @Test
    fun extractsSpsAndPpsAndDetectsKeyframes() {
        val sps = byteArrayOf(0x67, 0x42, 0x00, 0x1f)
        val pps = byteArrayOf(0x68, 0xce.toByte(), 0x3c, 0x80.toByte())
        val idr = byteArrayOf(0x65, 0x88.toByte(), 0x84.toByte())
        val nonIdr = byteArrayOf(0x41, 0x9a.toByte())
        val accessUnit = startCode() + sps + startCode() + pps + startCode() + idr

        val parameterSets = AnnexB.parameterSets(accessUnit)
        requireNotNull(parameterSets)
        val expected = startCode() + sps + startCode() + pps
        assertArrayEquals(expected, parameterSets)
        assertTrue(AnnexB.isKeyframe(accessUnit))
        assertFalse(AnnexB.isKeyframe(startCode() + nonIdr))
        assertNull(AnnexB.parameterSets(startCode() + nonIdr))
    }

    private fun startCode(): ByteArray = byteArrayOf(0, 0, 0, 1)
}
