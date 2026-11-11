package vista.module

/** Native adapter for the state invariants in VistaCore/ClientReducer.swift.
 * Transport authentication, decoded video and permission to inject input are
 * independent. Every asynchronous effect retains its originating context.
 */
enum class ConnectionState { Offline, Connecting, Connected, Reconnecting }
enum class PlaybackState { None, Waiting, Live, Recovering, Frozen }

data class HostWindow(val id: Long, val title: String, val appId: String?)
data class HostAction(val id: String, val label: String, val selectable: Boolean, val reason: String?)
data class StreamTarget(val kind: String, val id: Long? = null) {
    val key: String get() = if (id == null) kind else "$kind:$id"
    companion object { val Desktop = StreamTarget("desktop") }
}

data class InputToken(val connection: Long, val selection: Long, val geometry: Long, val ownership: Long = 0)

data class ClientState(
    val connectionId: Long = 0,
    val connection: ConnectionState = ConnectionState.Offline,
    val host: String = "",
    val message: String = "Connect to your desktop",
    val windows: List<HostWindow> = emptyList(),
    val catalogRevision: Long = 0,
    val actions: List<HostAction> = emptyList(),
    val capabilityRevision: Long = 0,
    val selection: Long = 0,
    val target: StreamTarget? = null,
    val streamReady: Boolean = false,
    val geometryEpoch: Long = 0,
    val width: Int = 0,
    val height: Int = 0,
    val playback: PlaybackState = PlaybackState.None,
    val decodedFrames: Long = 0,
    val inputGranted: Boolean = false,
    val controlEnabled: Boolean = false,
    val inputGeneration: Long = 0,
    val foreground: Boolean = true,
    val inputScope: String = "unknown",
    val captureScope: String = "unknown",
) {
    val connected: Boolean get() = connection == ConnectionState.Connected
    val canControl: Boolean get() = connected && streamReady && geometryEpoch > 0 &&
        playback == PlaybackState.Live && inputGranted && controlEnabled && foreground
    val inputToken: InputToken? get() = if (canControl) InputToken(connectionId, selection, geometryEpoch, inputGeneration) else null
    val videoLabel: String get() = when (playback) {
        PlaybackState.None -> "Choose a source"
        PlaybackState.Waiting -> "Waiting for the first frame"
        PlaybackState.Live -> if (canControl) "Live · Control enabled" else "Live · View only"
        PlaybackState.Recovering -> "Recovering video"
        PlaybackState.Frozen -> "Disconnected · Frozen picture"
    }
    val scopeLabel: String get() = when (inputScope) {
        "shared_desktop" -> "Control affects the shared desktop"
        "private_session" -> "Control affects this private session"
        else -> "Input scope is unconfirmed. Check sharing on the host."
    }
    val targetLabel: String get() = when (target?.kind) {
        "window" -> windows.find { it.id == target.id }?.title ?: "Selected window on host"
        "workspace" -> "Workspace ${target.id}"
        "desktop" -> "Desktop"
        else -> "Selected source on host"
    }
}

sealed interface ClientEvent {
    data class Authenticated(val inputGranted: Boolean) : ClientEvent
    data class Catalog(val revision: Long, val windows: List<HostWindow>) : ClientEvent
    data class Capabilities(val revision: Long, val actions: List<HostAction>) : ClientEvent
    data class Selecting(val target: StreamTarget) : ClientEvent
    data class Geometry(val target: StreamTarget, val epoch: Long, val width: Int, val height: Int,
                        val inputScope: String = "unknown", val captureScope: String = "unknown") : ClientEvent
    data class Frame(val selection: Long, val epoch: Long) : ClientEvent
    data class Recovering(val selection: Long) : ClientEvent
    data object Release : ClientEvent
    data class InputPermission(val granted: Boolean) : ClientEvent
    data class Control(val enabled: Boolean) : ClientEvent
    data class Foreground(val active: Boolean) : ClientEvent
    data class Message(val text: String) : ClientEvent
    data class Stopped(val reason: String) : ClientEvent
    data class Lost(val reason: String) : ClientEvent
}

object ClientReducer {
    fun reduce(state: ClientState, connection: Long, event: ClientEvent): ClientState {
        if (state.connectionId != connection) return state
        if (event !is ClientEvent.Authenticated && event !is ClientEvent.Foreground &&
            event !is ClientEvent.Message && event !is ClientEvent.Lost && !state.connected) return state
        return when (event) {
            is ClientEvent.Authenticated -> state.copy(connection = ConnectionState.Connected,
                inputGranted = event.inputGranted, message = "Connected. Choose a source.")
            is ClientEvent.Catalog -> if (event.revision <= state.catalogRevision) state else
                state.copy(windows = event.windows, catalogRevision = event.revision)
            is ClientEvent.Capabilities -> if (event.revision <= state.capabilityRevision) state else
                state.copy(actions = event.actions, capabilityRevision = event.revision)
            is ClientEvent.Selecting -> state.copy(target = event.target, selection = state.selection + 1,
                streamReady = false, geometryEpoch = 0, decodedFrames = 0, actions = emptyList(),
                playback = PlaybackState.Waiting, controlEnabled = false,
                inputScope = "unknown", captureScope = "unknown", message = "Opening source…")
            is ClientEvent.Geometry -> if (event.target != state.target || event.epoch <= 0 ||
                event.width <= 0 || event.height <= 0 || event.epoch < state.geometryEpoch) state else
                state.copy(streamReady = true, geometryEpoch = event.epoch, width = event.width,
                    height = event.height, inputScope = event.inputScope, captureScope = event.captureScope,
                    playback = if (event.epoch != state.geometryEpoch) PlaybackState.Waiting else state.playback,
                    controlEnabled = if (event.epoch != state.geometryEpoch) false else state.controlEnabled)
            is ClientEvent.Frame -> if (!state.streamReady || event.selection != state.selection ||
                event.epoch != state.geometryEpoch) state else state.copy(playback = PlaybackState.Live,
                    decodedFrames = state.decodedFrames + 1, message = "Connected")
            is ClientEvent.Recovering -> if (event.selection != state.selection) state else
                state.copy(playback = PlaybackState.Recovering, controlEnabled = false)
            ClientEvent.Release -> state.copy(inputGeneration = state.inputGeneration + 1)
            is ClientEvent.InputPermission -> state.copy(inputGranted = event.granted,
                controlEnabled = state.controlEnabled && event.granted, inputGeneration = state.inputGeneration + 1)
            is ClientEvent.Control -> state.copy(inputGeneration = state.inputGeneration + 1, controlEnabled = event.enabled && state.inputGranted &&
                state.playback == PlaybackState.Live && state.foreground)
            is ClientEvent.Foreground -> state.copy(foreground = event.active, inputGeneration = state.inputGeneration + 1,
                controlEnabled = state.controlEnabled && event.active)
            is ClientEvent.Message -> state.copy(message = event.text)
            is ClientEvent.Stopped -> state.copy(streamReady = false, playback = PlaybackState.None,
                controlEnabled = false, actions = emptyList(), message = event.reason)
            is ClientEvent.Lost -> state.copy(connection = ConnectionState.Reconnecting,
                playback = if (state.decodedFrames > 0) PlaybackState.Frozen else PlaybackState.None,
                streamReady = false, controlEnabled = false, inputGranted = false,
                windows = emptyList(), actions = emptyList(), message = event.reason)
        }
    }
}
