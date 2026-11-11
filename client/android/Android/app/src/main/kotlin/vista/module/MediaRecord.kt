package vista.module

/** One authenticated H.264 access unit, mirroring `host/src/media_transport.rs`. */
data class MediaRecord(
    val ptsNanos: Long,
    val keyframe: Boolean,
    val discontinuity: Boolean,
    val bytes: ByteArray,
) {
    companion object {
        private const val FLAG_KEYFRAME = 0b0000_0001
        private const val FLAG_DISCONTINUITY = 0b0000_0010
        private const val KNOWN_FLAGS = FLAG_KEYFRAME or FLAG_DISCONTINUITY

        fun parse(plaintext: ByteArray): MediaRecord {
            check(plaintext.size >= VistaWire.MEDIA_HEADER_BYTES) { "truncated media record" }
            var pts = 0L
            for (index in 0 until 8) {
                pts = (pts shl 8) or (plaintext[index].toLong() and 0xff)
            }
            val flags = plaintext[8].toInt() and 0xff
            check(flags and KNOWN_FLAGS.inv() == 0) { "invalid media flags $flags" }
            val frame = plaintext.copyOfRange(VistaWire.MEDIA_HEADER_BYTES, plaintext.size)
            check(frame.isNotEmpty()) { "empty media frame" }
            check(frame.size <= VistaWire.MAX_MEDIA_FRAME_BYTES) { "media frame too large" }
            return MediaRecord(
                ptsNanos = pts,
                keyframe = flags and FLAG_KEYFRAME != 0,
                discontinuity = flags and FLAG_DISCONTINUITY != 0,
                bytes = frame,
            )
        }
    }
}

/** Annex-B helpers used by the decoder; no Android types, so JVM-testable. */
object AnnexB {
    private const val NAL_TYPE_MASK = 0x1f
    private const val NAL_SPS = 7
    private const val NAL_PPS = 8

    /** Range of every NAL unit payload, excluding its Annex-B start code. */
    fun nalUnits(data: ByteArray): List<IntRange> {
        val units = ArrayList<IntRange>()
        var nalStart = -1
        var index = 0
        while (index + 2 < data.size) {
            if (data[index] == 0.toByte() && data[index + 1] == 0.toByte()) {
                val startCodeBytes = when {
                    data[index + 2] == 1.toByte() -> 3
                    index + 3 < data.size && data[index + 2] == 0.toByte() && data[index + 3] == 1.toByte() -> 4
                    else -> 0
                }
                if (startCodeBytes > 0) {
                    if (nalStart >= 0) units.add(nalStart until index)
                    nalStart = index + startCodeBytes
                    index += startCodeBytes
                    continue
                }
            }
            index++
        }
        if (nalStart >= 0) units.add(nalStart until data.size)
        return units.filter { it.first < it.last }
    }

    /**
     * SPS/PPS NAL units with start codes, or null when the access unit does not
     * carry them. The host repeats parameter sets at every IDR.
     */
    fun parameterSets(data: ByteArray): ByteArray? {
        val units = nalUnits(data)
        val selected = units.filter { range ->
            val header = data[range.first].toInt() and NAL_TYPE_MASK
            header == NAL_SPS || header == NAL_PPS
        }
        if (selected.isEmpty()) return null
        val output = java.io.ByteArrayOutputStream()
        for (range in selected) {
            output.write(byteArrayOf(0, 0, 0, 1))
            output.write(data, range.first, range.last - range.first + 1)
        }
        return output.toByteArray()
    }

    fun isKeyframe(data: ByteArray): Boolean = nalUnits(data).any { range ->
        val header = data[range.first].toInt() and NAL_TYPE_MASK
        header == 5
    }
}
