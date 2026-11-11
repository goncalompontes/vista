package vista.module

import android.os.Handler
import android.os.Looper
import android.media.MediaCodec
import android.media.MediaFormat
import android.view.Surface
import android.util.Log
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
    private var token: InputToken? = null
    var onFrameRendered: ((InputToken) -> Unit)? = null

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

    fun detachSurface(oldSurface: Surface): Boolean {
        synchronized(lock) {
            if (surface !== oldSurface) return false
            releaseLocked()
            surface = null
            return true
        }
    }

    fun release() {
        synchronized(lock) {
            releaseLocked()
            surface = null
        }
    }

    /** Flush any previous stream so the next IDR re-sends codec configuration. */
    fun resetForNewStream(width: Int, height: Int, token: InputToken) {
        synchronized(lock) {
            releaseLocked()
            this.width = width
            this.height = height
            this.token = token
            configureLocked()
            waitingForKeyframe = true
            needsCodecConfig = true
            lastPtsUs = -1L
        }
    }

    fun submit(record: MediaRecord, token: InputToken): Int {
        synchronized(lock) {
            if (this.token != token) return 0
            val active = codec ?: run { if (record.keyframe) Log.d("VistaVideo", "IDR waiting for surface"); return 0 }
            if (record.keyframe) Log.d("VistaVideo", "IDR received; config=$needsCodecConfig")
            if (record.discontinuity) {
                active.flush()
                waitingForKeyframe = true
                needsCodecConfig = true
                lastPtsUs = -1L
            }
            if (waitingForKeyframe && !record.keyframe) return 0
            if (record.keyframe) waitingForKeyframe = false

            if (needsCodecConfig) {
                val parameterSets = AnnexB.parameterSets(record.bytes) ?: return 0
                queue(active, parameterSets, 0, MediaCodec.BUFFER_FLAG_CODEC_CONFIG)
                needsCodecConfig = false
            }
            val ptsUs = maxOf(record.ptsNanos / 1_000, lastPtsUs + 1)
            lastPtsUs = ptsUs
            queue(active, record.bytes, ptsUs, 0)
            return drain(active)
        }
    }

    private fun configureLocked() {
        val target = surface ?: return
        if (width <= 0 || height <= 0) return
        val format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, width, height).apply {
            setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, 4 * 1024 * 1024)
        }
        val created = MediaCodec.createDecoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
        Log.d("VistaVideo", "configure ${width}x$height surface=${target.isValid}")
        created.configure(format, target, null, 0)
        val stream = token
        created.setOnFrameRenderedListener({ _, _, _ ->
            val current = synchronized(lock) { codec === created && surface === target }
            if (current && stream != null) onFrameRendered?.invoke(stream)
        }, Handler(Looper.getMainLooper()))
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

    private fun drain(active: MediaCodec): Int {
        var decoded = 0
        val info = MediaCodec.BufferInfo()
        while (true) {
            val index = active.dequeueOutputBuffer(info, 0)
            if (index >= 0) {
                // Surface decoders may return size=0 for a decoded graphic
                // buffer. Only the render listener establishes live playback.
                if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG == 0) decoded++
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
            return decoded
        }
    }
}
