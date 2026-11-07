/// Explicitly fictional catalog for exercising the SwiftUI flow before
/// real authenticated host discovery and media streaming are implemented.
public enum DemoCatalog {
    public static let applications: [RemoteApplication] = [
        RemoteApplication(
            id: "demo-browser", name: "Browser", subtitle: "Web workspace",
            symbolName: "globe", source: .existingWindow
        ),
        RemoteApplication(
            id: "demo-terminal", name: "Terminal", subtitle: "Shell session",
            symbolName: "terminal", source: .existingWindow
        ),
        RemoteApplication(
            id: "demo-editor", name: "Code Editor", subtitle: "Development",
            symbolName: "chevron.left.forwardslash.chevron.right",
            source: .existingWindow
        ),
        RemoteApplication(
            id: "demo-music", name: "Music Player", subtitle: "Media",
            symbolName: "music.note", source: .independentSession
        )
    ]
}
