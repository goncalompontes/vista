import Foundation
import VistaCore

/// Observable holder for the latest catalog snapshot received from a paired
/// host. It maps host metadata to portable `RemoteApplication` rows and never
/// invents entries when the host reports none.
@MainActor
final class LiveCatalogStore: ObservableObject {
    @Published private(set) var snapshot: CatalogSnapshot?
    @Published private(set) var hostName: String?

    var isLive: Bool { snapshot != nil }
    var revision: UInt64 { snapshot?.revision ?? 0 }

    var applications: [RemoteApplication] {
        guard let snapshot else { return [] }
        return snapshot.entries.map { entry in
            RemoteApplication(
                id: entry.id,
                name: entry.displayName,
                subtitle: entry.appID ?? "Window \(entry.windowID)",
                symbolName: "macwindow",
                source: .existingWindow
            )
        }
    }

    func setHost(_ name: String) {
        hostName = name
    }

    func apply(_ snapshot: CatalogSnapshot, hostName: String?) {
        if let hostName {
            self.hostName = hostName
        }
        self.snapshot = snapshot
    }

    func clear() {
        snapshot = nil
        hostName = nil
    }
}
