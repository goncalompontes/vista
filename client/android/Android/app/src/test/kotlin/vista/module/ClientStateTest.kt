package vista.module

import org.junit.Assert.*
import org.junit.Test

class ClientStateTest {
    private fun ready(input: Boolean = true): ClientState {
        var state = ClientState(connectionId = 1)
        state = ClientReducer.reduce(state, 1, ClientEvent.Authenticated(input))
        state = ClientReducer.reduce(state, 1, ClientEvent.Selecting(StreamTarget.Desktop))
        return ClientReducer.reduce(state, 1, ClientEvent.Geometry(StreamTarget.Desktop, 3, 1920, 1080, "shared_desktop", "display"))
    }
    private fun live(): ClientState {
        val state = ready()
        return ClientReducer.reduce(state, 1, ClientEvent.Frame(state.selection, state.geometryEpoch))
    }
    private fun controlled(): ClientState = ClientReducer.reduce(live(), 1, ClientEvent.Control(true))

    @Test fun handshakeAndEncodedGeometryDoNotEstablishPlayback() {
        val state = ready()
        assertTrue(state.connected)
        assertEquals(PlaybackState.Waiting, state.playback)
        assertFalse(state.canControl)
        assertNull(state.inputToken)
        assertFalse(state.videoLabel.contains("Live"))
    }
    @Test fun firstFrameIsViewOnlyUntilUserEnablesControl() {
        assertEquals(PlaybackState.Live, live().playback)
        assertFalse(live().canControl)
        assertTrue(controlled().canControl)
        val viewer = ready(false)
        val decoded = ClientReducer.reduce(viewer, 1, ClientEvent.Frame(viewer.selection, 3))
        assertFalse(ClientReducer.reduce(decoded, 1, ClientEvent.Control(true)).canControl)
        assertEquals("Control affects the shared desktop", controlled().scopeLabel)
    }
    @Test fun staleConnectionsSelectionsAndGeometryCannotRestoreInput() {
        val state = controlled()
        assertEquals(state, ClientReducer.reduce(state, 2, ClientEvent.Lost("old callback")))
        assertEquals(state, ClientReducer.reduce(state, 1, ClientEvent.Frame(state.selection - 1, 3)))
        assertEquals(state, ClientReducer.reduce(state, 1, ClientEvent.Frame(state.selection, 2)))
        val changed = ClientReducer.reduce(state, 1, ClientEvent.Selecting(StreamTarget("window", 5)))
        assertFalse(changed.canControl)
        assertEquals(changed, ClientReducer.reduce(changed, 1, ClientEvent.Geometry(StreamTarget.Desktop, 4, 640, 480)))
        assertFalse(ClientReducer.reduce(changed, 1, ClientEvent.Frame(state.selection, 3)).canControl)
    }
    @Test fun permissionBackgroundStopAndDisconnectInvalidateQueuedInput() {
        val state = controlled()
        for (event in listOf(ClientEvent.InputPermission(false), ClientEvent.Foreground(false),
            ClientEvent.Stopped("closed"), ClientEvent.Lost("offline"), ClientEvent.Control(false))) {
            val next = ClientReducer.reduce(state, 1, event)
            assertFalse("$event", next.canControl)
            assertNotEquals(state.inputToken, next.inputToken)
        }
        val stopped = ClientReducer.reduce(state, 1, ClientEvent.Foreground(false))
        assertFalse(ClientReducer.reduce(stopped, 1, ClientEvent.Foreground(true)).canControl)
        assertNotEquals(state.inputToken, ClientReducer.reduce(state, 1, ClientEvent.Release).inputToken)
    }
    @Test fun resizeAndDecoderRecoveryRequireFreshFramesAndControl() {
        val state = controlled()
        val resized = ClientReducer.reduce(state, 1, ClientEvent.Geometry(StreamTarget.Desktop, 4, 800, 600))
        assertFalse(resized.canControl)
        assertEquals(PlaybackState.Waiting, resized.playback)
        assertEquals(resized, ClientReducer.reduce(resized, 1, ClientEvent.Frame(resized.selection, 3)))
        val recovering = ClientReducer.reduce(state, 1, ClientEvent.Recovering(state.selection))
        assertFalse(recovering.canControl)
        assertFalse(ClientReducer.reduce(recovering, 1, ClientEvent.Frame(state.selection, 3)).canControl)
    }
    @Test fun oldCatalogAndCapabilitySnapshotsNeverReplaceNewState() {
        val state = ready()
        val actions = listOf(HostAction("window.focus", "Focus", true, null))
        val snapshot = ClientReducer.reduce(state, 1, ClientEvent.Capabilities(5, actions))
        assertEquals(snapshot, ClientReducer.reduce(snapshot, 1, ClientEvent.Capabilities(4, emptyList())))
        assertTrue(ClientReducer.reduce(snapshot, 1, ClientEvent.Capabilities(6, emptyList())).actions.isEmpty())
        val catalog = ClientReducer.reduce(state, 1, ClientEvent.Catalog(5, listOf(HostWindow(5, "Test", null))))
        assertEquals(catalog, ClientReducer.reduce(catalog, 1, ClientEvent.Catalog(3, emptyList())))
    }
    @Test fun unknownSourceNeverFabricatesWindowIdentityOrPrivateScope() {
        val state = ready().copy(target = StreamTarget("picker"), inputScope = "unknown")
        assertEquals("Selected source on host", state.targetLabel)
        assertTrue(state.scopeLabel.contains("unconfirmed"))
        val lost = ClientReducer.reduce(controlled(), 1, ClientEvent.Lost("offline"))
        assertEquals(PlaybackState.Frozen, lost.playback)
        assertTrue(lost.actions.isEmpty())
        assertTrue(lost.windows.isEmpty())
    }
}
