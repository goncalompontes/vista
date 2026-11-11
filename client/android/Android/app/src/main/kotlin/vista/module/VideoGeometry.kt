package vista.module

data class SourcePoint(val x: Double, val y: Double)

/** The actual fitted video rectangle, including letterbox/pillarbox rejection.
 * Equivalent to VideoInputGeometry from PR #85; no upstream source copied.
 */
data class VideoGeometry(val left: Double, val top: Double, val width: Double, val height: Double,
                         val token: InputToken) {
    fun sourcePoint(x: Double, y: Double, current: InputToken?): SourcePoint? {
        if (current != token || !x.isFinite() || !y.isFinite() || width <= 0 || height <= 0) return null
        val px = (x - left) / width
        val py = (y - top) / height
        return if (px in 0.0..1.0 && py in 0.0..1.0) SourcePoint(px, py) else null
    }
    companion object {
        fun fit(viewWidth: Int, viewHeight: Int, sourceWidth: Int, sourceHeight: Int,
                token: InputToken): VideoGeometry? {
            if (minOf(viewWidth, viewHeight, sourceWidth, sourceHeight) <= 0) return null
            val scale = minOf(viewWidth.toDouble() / sourceWidth, viewHeight.toDouble() / sourceHeight)
            val width = sourceWidth * scale
            val height = sourceHeight * scale
            return VideoGeometry((viewWidth - width) / 2, (viewHeight - height) / 2, width, height, token)
        }
    }
}
