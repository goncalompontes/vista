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
    @State private var showingPairing = false
    @StateObject private var catalog = LiveCatalogStore()

    var body: some View {
        Group {
            if showingDemo {
                LibraryView(
                    onExit: { showingDemo = false },
                    liveApplications: catalog.applications,
                    isLive: catalog.isLive,
                    hostLabel: catalog.hostName
                )
            } else {
                WelcomeView(
                    onExplore: { showingDemo = true },
                    onPair: { showingPairing = true }
                )
            }
        }
        .sheet(isPresented: $showingPairing) {
            PairingView(store: catalog)
        }
    }
}
