package vista.module

import org.json.JSONObject
import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.truncate

sealed interface RemoteInput {
    data class Move(val point: SourcePoint) : RemoteInput
    data class Button(val pressed: Boolean, val button: String = "primary") : RemoteInput
    data class Scroll(val dx: Double, val dy: Double, val phase: String, val anchor: SourcePoint) : RemoteInput
    data class Key(val usage: Int, val pressed: Boolean, val repeat: Boolean = false) : RemoteInput
    data class Text(val text: String) : RemoteInput
    data object Cancel : RemoteInput

    fun wire(epoch: Long): JSONObject = when (this) {
        is Move -> JSONObject().put("type", "pointer.absolute").put("x", point.x)
            .put("y", point.y).put("geometry_epoch", epoch)
        is Button -> JSONObject().put("type", "pointer.button").put("button", button).put("pressed", pressed)
        is Scroll -> JSONObject().put("type", "scroll").put("dx", dx).put("dy", dy)
            .put("unit", "line").put("phase", phase).put("momentum", false)
            .put("anchor_x", anchor.x).put("anchor_y", anchor.y)
        is Key -> JSONObject().put("type", "key.physical").put("usage", usage)
            .put("pressed", pressed).put("repeat", repeat)
        is Text -> JSONObject().put("type", "text.commit").put("text", text)
        Cancel -> JSONObject().put("type", "input.cancel")
    }
}

/** Commands keep the token from recognition, never the context at queue drain. */
class InputGuard {
    private val keys = mutableSetOf<Int>()
    private val buttons = mutableSetOf<String>()
    val holding: Boolean get() = keys.isNotEmpty() || buttons.isNotEmpty()

    fun accept(command: RemoteInput, original: InputToken, current: InputToken?): Boolean {
        if (original != current) return false
        return when (command) {
            is RemoteInput.Move -> command.point.x.isFinite() && command.point.y.isFinite() &&
                command.point.x in 0.0..1.0 && command.point.y in 0.0..1.0
            is RemoteInput.Button -> {
                if (command.button !in listOf("primary", "secondary")) false else {
                    if (command.pressed) buttons.add(command.button) else buttons.remove(command.button)
                    true
                }
            }
            is RemoteInput.Key -> {
                if (command.usage <= 0 || command.usage > 0xffff || (command.repeat && !command.pressed)) false
                else if (!command.pressed && command.usage !in keys) false
                else { if (command.pressed) keys.add(command.usage) else keys.remove(command.usage); true }
            }
            is RemoteInput.Text -> command.text.isNotEmpty() && command.text.toByteArray().size <= 16 * 1024
            is RemoteInput.Scroll -> command.dx.isFinite() && command.dy.isFinite() &&
                abs(command.dx) <= 10_000 && abs(command.dy) <= 10_000 &&
                command.anchor.x in 0.0..1.0 && command.anchor.y in 0.0..1.0
            RemoteInput.Cancel -> { clear(); true }
        }
    }
    fun clear() { keys.clear(); buttons.clear() }
}

data class TouchPoint(val x: Double, val y: Double)

/** Direct-touch recognizer with one owner for the full gesture. A tap is
 * deferred until UP; adding a second finger therefore cannot leak a click.
 * Long press is secondary click, one-finger movement is a held-button drag,
 * two-finger translation is scroll. Pinch/3+ fingers cancel remote ownership.
 */
class RemoteGesture(private val emit: (InputToken, List<RemoteInput>) -> Unit) {
    private enum class Mode { Idle, Pending, Drag, Scroll, Consumed }
    private var mode = Mode.Idle
    private var geometry: VideoGeometry? = null
    private var origin = TouchPoint(0.0, 0.0)
    private var last = origin
    private var density = 1.0
    private var span = 0.0
    private var scrollX = 0.0
    private var scrollY = 0.0
    private var anchor = SourcePoint(0.5, 0.5)

    fun down(point: TouchPoint, content: VideoGeometry?, current: InputToken?, density: Double) {
        cancel()
        val mapped = content?.sourcePoint(point.x, point.y, current) ?: return
        geometry = content
        origin = point
        last = point
        anchor = mapped
        this.density = density.coerceAtLeast(0.5)
        mode = Mode.Pending
    }

    fun move(points: List<TouchPoint>, current: InputToken?) {
        val content = geometry ?: return
        if (current != content.token || points.size > 2 || points.isEmpty()) { cancel(); return }
        if (mode == Mode.Idle || mode == Mode.Consumed) return
        val center = TouchPoint(points.sumOf { it.x } / points.size, points.sumOf { it.y } / points.size)
        val mapped = content.sourcePoint(center.x, center.y, current) ?: run { cancel(); return }
        if (points.size == 2) {
            val distance = hypot(points[0].x - points[1].x, points[0].y - points[1].y)
            if (mode != Mode.Scroll) {
                if (mode == Mode.Drag) send(RemoteInput.Button(false))
                mode = Mode.Scroll
                span = distance
                last = center
                anchor = mapped
                scrollX = 0.0
                scrollY = 0.0
                send(RemoteInput.Move(anchor), RemoteInput.Scroll(0.0, 0.0, "begin", anchor))
                return
            }
            if (abs(distance - span) > maxOf(20 * density, span * 0.2)) { cancel(); return }
            scrollX += (last.x - center.x) / (32 * density)
            scrollY += (last.y - center.y) / (32 * density)
            val dx = truncate(scrollX)
            val dy = truncate(scrollY)
            if (dx != 0.0 || dy != 0.0) {
                send(RemoteInput.Scroll(dx, dy, "update", anchor))
                scrollX -= dx
                scrollY -= dy
            }
        } else if (mode != Mode.Scroll) {
            if (mode == Mode.Pending && hypot(center.x - origin.x, center.y - origin.y) > 8 * density) {
                val start = content.sourcePoint(origin.x, origin.y, current) ?: return
                send(RemoteInput.Move(start), RemoteInput.Button(true))
                mode = Mode.Drag
            }
            if (mode == Mode.Drag) send(RemoteInput.Move(mapped))
        }
        last = center
    }

    fun up(point: TouchPoint, current: InputToken?) {
        val content = geometry ?: return
        val mapped = content.sourcePoint(point.x, point.y, current) ?: run { cancel(); return }
        when (mode) {
            Mode.Pending -> send(RemoteInput.Move(mapped), RemoteInput.Button(true), RemoteInput.Button(false))
            Mode.Drag -> send(RemoteInput.Move(mapped), RemoteInput.Button(false))
            Mode.Scroll -> send(RemoteInput.Scroll(0.0, 0.0, "end", anchor))
            else -> Unit
        }
        clear()
    }

    fun longPress(current: InputToken?): Boolean {
        val content = geometry ?: return false
        if (mode != Mode.Pending || content.token != current) return false
        send(RemoteInput.Move(anchor), RemoteInput.Button(true, "secondary"), RemoteInput.Button(false, "secondary"))
        mode = Mode.Consumed
        return true
    }

    fun cancel() {
        if (mode == Mode.Scroll) send(RemoteInput.Scroll(0.0, 0.0, "cancel", anchor))
        if (mode != Mode.Idle) send(RemoteInput.Cancel)
        clear()
    }

    private fun send(vararg events: RemoteInput) { geometry?.let { emit(it.token, events.toList()) } }
    private fun clear() { mode = Mode.Idle; geometry = null }
}
