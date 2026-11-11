package vista.module

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path

class StreamScopeTest {
    @Test fun sharedRustSwiftAndroidScopesRemainUnknownOnLegacyHosts() {
        var directory: Path? = Path.of(System.getProperty("user.dir")).toAbsolutePath()
        var fixtures: JSONObject? = null
        while (directory != null) {
            val path = directory.resolve("docs/fixtures/stream-scopes-v1.json")
            if (Files.isRegularFile(path)) { fixtures = JSONObject(String(Files.readAllBytes(path), Charsets.UTF_8)); break }
            directory = directory.parent
        }
        val root = requireNotNull(fixtures)
        for (name in listOf("legacy", "scoped")) {
            val frame = root.getJSONObject(name)
            var state = ClientState(connectionId = 1)
            state = ClientReducer.reduce(state, 1, ClientEvent.Authenticated(true))
            val target = StreamTarget("window", 5)
            state = ClientReducer.reduce(state, 1, ClientEvent.Selecting(target))
            state = ClientReducer.reduce(state, 1, ClientEvent.Geometry(target, frame.getLong("geometry_epoch"),
                frame.getInt("width"), frame.getInt("height"), frame.optString("input_scope", "unknown"),
                frame.optString("capture_scope", "unknown")))
            assertEquals(if (name == "legacy") "unknown" else "shared_desktop", state.inputScope)
            assertEquals(if (name == "legacy") "unknown" else "display", state.captureScope)
            assertFalse(state.canControl)
            assertFalse(state.scopeLabel.contains("private session"))
        }
    }
}
