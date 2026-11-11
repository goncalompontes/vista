package vista.module

import android.media.MediaCodec
import android.media.MediaFormat
import android.view.Surface
import java.nio.ByteBuffer

/**
 * H.264 decoder feeding a [Surface]. Records are submitted from the media
 * reader thread; surface attach/detach comes from the UI thread, so every
 * entry point is serialised on [lock].
 */
class MediaDecoder {
    private val lock = Any()
    private var codec: MediaCodec? = null
    private var surface: Surface? = null
    private var waitingForKeyframe = true
    private var needsCodecConfig = true
    private var lastPtsUs = -1L

    /** Last negotiated stream geometry, or null before the first record. */
    var width: Int = 0
        private set
    var height: Int = 0
        private set

    fun attachSurface(newSurface: Surface) {
        synchronized(lock) {
            releaseLocked()
            surface = newSurface
            configureLocked()
        }
    }

    fun detachSurface() {
        synchronized(lock) {
            releaseLocked()
            surface = null
        }
    }

    fun release() {
        synchronized(lock) {
            releaseLocked()
            surface = null
        }
    }

    fun submit(record: MediaRecord) {
        synchronized(lock) {
            val active = codec ?: return
            if (record.discontinuity) {
                active.flush()
                waitingForKeyframe = true
                needsCodecConfig = true
                lastPtsUs = -1L
            }
            if (waitingForKeyframe && !record.keyframe) return
            if (record.keyframe) waitingForKeyframe = false

            if (needsCodecConfig) {
                val parameterSets = AnnexB.parameterSets(record.bytes) ?: return
                queue(active, parameterSets, 0, MediaCodec.BUFFER_FLAG_CODEC_CONFIG)
                needsCodecConfig = false
            }
            val ptsUs = maxOf(record.ptsNanos / 1_000, lastPtsUs + 1)
            lastPtsUs = ptsUs
            queue(active, record.bytes, ptsUs, 0)
            drain(active)
        }
    }

    private fun configureLocked() {
        val target = surface ?: return
        val format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, 1920, 1080).apply {
            setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, 4 * 1024 * 1024)
        }
        val created = MediaCodec.createDecoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
        created.configure(format, target, null, 0)
        created.start()
        codec = created
        waitingForKeyframe = true
        needsCodecConfig = true
        lastPtsUs = -1L
    }

    private fun releaseLocked() {
        val active = codec ?: return
        codec = null
        try {
            active.stop()
        } catch (_: Throwable) {
        }
        try {
            active.release()
        } catch (_: Throwable) {
        }
    }

    private fun queue(active: MediaCodec, data: ByteArray, ptsUs: Long, flags: Int) {
        var attempts = 0
        while (true) {
            val index = active.dequeueInputBuffer(10_000)
            if (index >= 0) {
                val buffer: ByteBuffer = active.getInputBuffer(index) ?: return
                buffer.clear()
                buffer.put(data)
                active.queueInputBuffer(index, 0, data.size, ptsUs, flags)
                return
            }
            drain(active)
            if (++attempts > 500) return
        }
    }

    private fun drain(active: MediaCodec) {
        val info = MediaCodec.BufferInfo()
        while (true) {
            val index = active.dequeueOutputBuffer(info, 0)
            if (index >= 0) {
                active.releaseOutputBuffer(index, true)
                continue
            }
            if (index == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                val changed = active.outputFormat
                if (changed.containsKey(MediaFormat.KEY_WIDTH)) {
                    width = changed.getInteger(MediaFormat.KEY_WIDTH)
                }
                if (changed.containsKey(MediaFormat.KEY_HEIGHT)) {
                    height = changed.getInteger(MediaFormat.KEY_HEIGHT)
                }
                continue
            }
            return
        }
    }
}
