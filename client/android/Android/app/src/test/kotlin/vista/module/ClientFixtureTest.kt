package vista.module

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path

/** The design fixtures are semantic scenarios, not current v1 wire messages.
 * These are the state/input cases supported by the live v1 Android adapter.
 * Enrollment, clipboard and managed-session scenarios remain future work.
 */
class ClientFixtureTest {
    @Test fun supportedDesignFixtureExpectationsMatchTheNativeReducer() {
        var directory: Path? = Path.of(System.getProperty("user.dir")).toAbsolutePath()
        var root: JSONObject? = null
        while (directory != null) {
            val path = directory.resolve("docs/design/client-state-fixtures.json")
            if (Files.isRegularFile(path)) { root = JSONObject(String(Files.readAllBytes(path), Charsets.UTF_8)); break }
            directory = directory.parent
        }
        val cases = requireNotNull(root).getJSONArray("cases")
        val fixtures = (0 until cases.length()).map { cases.getJSONObject(it) }.associateBy { it.getString("id") }
        fun expected(id: String) = requireNotNull(fixtures[id]).getJSONObject("expected")
        fun event(state: ClientState, event: ClientEvent) = ClientReducer.reduce(state, 1, event)
        var state = event(ClientState(connectionId = 1), ClientEvent.Authenticated(true))
        state = event(state, ClientEvent.Selecting(StreamTarget.Desktop))
        state = event(state, ClientEvent.Geometry(StreamTarget.Desktop, 3, 1920, 1080, "shared_desktop", "display"))
        assertEquals(expected("media_connected_no_frame").getBoolean("live_badge"), state.playback == PlaybackState.Live)
        state = event(state, ClientEvent.Frame(state.selection, 3))
        assertEquals(expected("view_only").getBoolean("can_send_input"), state.canControl)
        state = event(state, ClientEvent.Control(true))
        assertEquals(expected("input_desktop_scope").getBoolean("can_send_input"), state.canControl)
        assertEquals(expected("input_desktop_scope").getBoolean("private_window_claim"), state.inputScope == "private_session")
        assertEquals(expected("lease_revoked").getBoolean("can_send_input"), event(state, ClientEvent.InputPermission(false)).canControl)
        assertEquals(expected("decoder_waiting_keyframe").getBoolean("live_badge"),
            event(state, ClientEvent.Recovering(state.selection)).playback == PlaybackState.Live)
        assertEquals(expected("disconnect_stops_input").getBoolean("can_send_input"), event(state, ClientEvent.Lost("offline")).canControl)
        val background = event(state, ClientEvent.Foreground(false))
        assertEquals(expected("backgrounds_during_control").getBoolean("can_send_input"), background.canControl)
        assertEquals(expected("backgrounds_during_control").getBoolean("no_auto_regrant"),
            !event(background, ClientEvent.Foreground(true)).controlEnabled)
        val token = state.inputToken!!
        val rect = VideoGeometry.fit(400, 400, 1920, 1080, token)!!
        assertEquals(expected("geometry_stale").getBoolean("input_event_emitted"),
            rect.sourcePoint(200.0, 200.0, token.copy(geometry = 4)) != null)
        assertEquals(expected("outside_letterboxed_video").getBoolean("input_event_emitted"),
            rect.sourcePoint(200.0, 0.0, token) != null)
        val resized = event(state, ClientEvent.Geometry(StreamTarget.Desktop, 4, 800, 600))
        assertEquals(expected("screen_resize_new_epoch").getBoolean("absolute_input_paused"), !resized.canControl)
    }
}
