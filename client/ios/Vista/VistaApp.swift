import SwiftUI

@main
struct VistaApp: App {
    var body: some Scene {
        WindowGroup {
            VistaRootView()
                .preferredColorScheme(.dark)
        }
    }
}

private struct VistaRootView: View {
    @State private var showingDemo = false

    var body: some View {
        Group {
            if showingDemo {
                LibraryView(onExit: { showingDemo = false })
            } else {
                WelcomeView(onExplore: { showingDemo = true })
            }
        }
    }
}
