package vista.module

import android.content.Context
import android.graphics.Color
import android.view.Gravity
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.ViewConfiguration
import android.widget.FrameLayout
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView

@Composable
fun VideoSurface(client: HostControlClient, state: ClientState, modifier: Modifier = Modifier) {
    AndroidView(modifier = modifier, factory = { RemoteVideoView(it, client) },
        update = { it.update(state) })
}

/** The fitted SurfaceView and input adapter share one measured coordinate
 * system. Letterbox touches never reach the host, including after rotation.
 */
private class RemoteVideoView(context: Context, private val client: HostControlClient) : FrameLayout(context) {
    private val picture = SurfaceView(context)
    private val gesture = RemoteGesture(client::send)
    private var model = client.state
    private var geometry: VideoGeometry? = null
    private val longPress = Runnable {
        if (gesture.longPress(model.inputToken)) performHapticFeedback(android.view.HapticFeedbackConstants.LONG_PRESS)
    }

    init {
        setBackgroundColor(Color.BLACK)
        isFocusable = false
        isFocusableInTouchMode = false
        contentDescription = "Remote picture. Tap to click, drag to move, two fingers to scroll, hold for secondary click."
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_YES
        picture.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
        addView(picture, LayoutParams(1, 1, Gravity.CENTER))
        picture.holder.addCallback(object : SurfaceHolder.Callback {
            override fun surfaceCreated(holder: SurfaceHolder) { client.attachSurface(holder.surface) }
            override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) = Unit
            override fun surfaceDestroyed(holder: SurfaceHolder) { gesture.cancel(); client.detachSurface(holder.surface) }
        })
    }

    fun update(next: ClientState) {
        if (model.inputToken != next.inputToken) {
            gesture.cancel()
            removeCallbacks(longPress)
        }
        val resized = model.width != next.width || model.height != next.height
        model = next
        if (resized || geometry?.token != next.inputToken) fit()
        contentDescription = if (next.canControl)
            "Remote picture. Tap to click, drag to move, two fingers to scroll, hold for secondary click."
        else "Remote picture. View only."
    }

    private fun fit() {
        val token = model.inputToken ?: InputToken(model.connectionId, model.selection, model.geometryEpoch)
        val rect = VideoGeometry.fit(width, height, model.width, model.height, token)
        geometry = rect
        if (rect != null) {
            val desired = LayoutParams(rect.width.toInt(), rect.height.toInt(), Gravity.CENTER)
            if (picture.layoutParams.width != desired.width || picture.layoutParams.height != desired.height) picture.layoutParams = desired
        }
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        gesture.cancel()
        client.releaseInput()
        fit()
    }

    override fun onInterceptTouchEvent(event: MotionEvent): Boolean = true

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (!model.canControl) return false
        val token = model.inputToken
        val point = TouchPoint(event.x.toDouble(), event.y.toDouble())
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                parent.requestDisallowInterceptTouchEvent(true)
                gesture.down(point, geometry, token, resources.displayMetrics.density.toDouble())
                postDelayed(longPress, ViewConfiguration.getLongPressTimeout().toLong())
            }
            MotionEvent.ACTION_POINTER_DOWN, MotionEvent.ACTION_MOVE -> gesture.move(
                (0 until event.pointerCount).map { TouchPoint(event.getX(it).toDouble(), event.getY(it).toDouble()) }, token)
            MotionEvent.ACTION_POINTER_UP -> removeCallbacks(longPress)
            MotionEvent.ACTION_UP -> {
                removeCallbacks(longPress)
                gesture.up(point, token)
                performClick()
                parent.requestDisallowInterceptTouchEvent(false)
            }
            MotionEvent.ACTION_CANCEL -> {
                removeCallbacks(longPress)
                gesture.cancel()
                client.releaseInput()
                parent.requestDisallowInterceptTouchEvent(false)
            }
        }
        return true
    }

    override fun performClick(): Boolean { super.performClick(); return true }

    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        val token = model.inputToken ?: return super.onGenericMotionEvent(event)
        val point = geometry?.sourcePoint(event.x.toDouble(), event.y.toDouble(), token) ?: return false
        if (event.actionMasked == MotionEvent.ACTION_SCROLL) {
            client.send(token, listOf(RemoteInput.Move(point), RemoteInput.Scroll(
                event.getAxisValue(MotionEvent.AXIS_HSCROLL).toDouble(),
                -event.getAxisValue(MotionEvent.AXIS_VSCROLL).toDouble(), "update", point)))
            return true
        }
        return super.onGenericMotionEvent(event)
    }

    override fun onDetachedFromWindow() {
        removeCallbacks(longPress)
        gesture.cancel()
        client.releaseInput()
        super.onDetachedFromWindow()
    }
}
