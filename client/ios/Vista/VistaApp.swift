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
            if showingDemo || catalog.isLive {
                LibraryView(
                    onExit: {
                        if catalog.isLive { catalog.clear() }
                        showingDemo = false
                    },
                    liveApplications: catalog.applications,
                    desktops: catalog.desktops,
                    output: catalog.output,
                    isLive: catalog.isLive,
                    hostLabel: catalog.hostName,
                    hostCapabilities: catalog.capabilities,
                    streamTarget: catalog.streamTarget,
                    streamMessage: catalog.streamMessage,
                    inputEnabled: catalog.inputEnabled,
                    geometryEpoch: catalog.geometryEpoch,
                    onInvoke: { actionID in catalog.invokeAction(id: actionID) },
                    onSelectStream: { target in catalog.selectStream(target) },
                    onStopStream: { catalog.stopStream() },
                    onInput: { event in catalog.sendInput(event) },
                    mediaRecords: catalog.mediaRecords,
                    mediaRecordSequence: catalog.mediaRecordSequence,
                    onKeyframeNeeded: { catalog.requestMediaKeyframe() }
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
