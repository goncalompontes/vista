import SwiftUI
import VistaCore

private enum LibraryFilter: String, CaseIterable, Identifiable {
    case all = "All apps"
    case desktop = "On desktop"
    case independent = "Independent"

    var id: LibraryFilter { self }

    func includes(_ app: RemoteApplication) -> Bool {
        switch self {
        case .all: true
        case .desktop: app.source == .existingWindow
        case .independent: app.source == .independentSession
        }
    }
}

struct LibraryView: View {
    let onExit: () -> Void
    var liveApplications: [RemoteApplication] = []
    var isLive = false
    var hostLabel: String?
    var hostCapabilities: CapabilitySnapshot?
    var onInvoke: ((String) -> Void)?
#if !SKIP
    var mediaRecords: [H264AccessUnit] = []
    var onKeyframeNeeded: (() -> Void)?
#endif
    @State private var filter: LibraryFilter = .all

    private var applications: [RemoteApplication] {
        let source = isLive ? liveApplications : DemoCatalog.applications
        return source.filter { filter.includes($0) }
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    HStack(alignment: .center, spacing: 13) {
                        VistaLogo(size: 49)
                        VStack(alignment: .leading, spacing: 3) {
                            Text("Vista")
                                .font(.title2.bold())
                                .foregroundStyle(.white)
                            Text("Your apps")
                                .font(.subheadline)
                                .foregroundStyle(VistaPalette.subtleText)
                        }
                        Spacer()
                        Text(isLive ? "LIVE" : "DEMO")
                            .font(.caption2.weight(.bold))
                            .tracking(1.2)
                            .padding(.horizontal, 11)
                            .padding(.vertical, 7)
                            .background(VistaPalette.elevated, in: Capsule())
                            .foregroundStyle(isLive ? .green : VistaPalette.accent)
                    }

                    HStack(alignment: .top, spacing: 14) {
                        VistaSymbol(name: isLive ? "wifi" : "wifi.slash")
                        VStack(alignment: .leading, spacing: 5) {
                            Text(isLive ? "Connected to \(hostLabel ?? "Linux host")" : "No Linux host paired")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(.white)
                            Text(
                                isLive
                                    ? "Showing the window catalog reported by the host. Video and input are not enabled yet."
                                    : "This is a local interface preview. The catalog below is sample data, not discovered windows."
                            )
                            .font(.footnote)
                            .foregroundStyle(VistaPalette.subtleText)
                        }
                    }
                    .padding(17)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(VistaPalette.surface, in: RoundedRectangle(cornerRadius: 19))

                    VStack(alignment: .leading, spacing: 13) {
                        Text("Applications")
                            .font(.title3.bold())
                            .foregroundStyle(.white)

                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 8) {
                                ForEach(LibraryFilter.allCases) { option in
                                    Button {
                                        withAnimation(.easeInOut(duration: 0.2)) { filter = option }
                                    } label: {
                                        Text(option.rawValue)
                                            .font(.subheadline.weight(.medium))
                                            .padding(.horizontal, 15)
                                            .padding(.vertical, 10)
                                            .background(
                                                filter == option ? VistaPalette.accent : VistaPalette.surface,
                                                in: Capsule()
                                            )
                                            .foregroundStyle(filter == option ? VistaPalette.background : .white)
                                    }
                                    .buttonStyle(.plain)
                                    #if !SKIP
                                    .accessibilityAddTraits(filter == option ? .isSelected : [])
                                    #endif
                                }
                            }
                        }
                    }

                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: 150, maximum: 240), spacing: 14)],
                        spacing: 14
                    ) {
                        ForEach(applications) { app in
                            NavigationLink(value: app) {
                                ApplicationTile(application: app)
                            }
                            .buttonStyle(.plain)
                        }
                    }

                    if isLive && applications.isEmpty {
                        Text(
                            filter == .all
                                ? "The host reported no windows."
                                : "No windows match this filter."
                        )
                        .font(.footnote)
                        .foregroundStyle(VistaPalette.subtleText)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }

                    Text(
                        isLive
                            ? "Catalog from the paired host. Video and input arrive in a later increment."
                            : "Streaming, host pairing and input are not available in this preview."
                    )
                        .font(.footnote)
                        .foregroundStyle(VistaPalette.subtleText)
                        .padding(.top, 3)
                }
                #if SKIP
                .frame(maxWidth: 380)
                #else
                .frame(maxWidth: 850)
                #endif
                .padding(.horizontal, 22)
                .padding(.top, 20)
                .padding(.bottom, 40)
                .frame(maxWidth: .infinity)
            }
            .background(VistaPalette.background.ignoresSafeArea())
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Exit preview", systemImage: "arrow.left", action: onExit)
                        .tint(VistaPalette.accent)
                }
            }
            .navigationDestination(for: RemoteApplication.self) { app in
#if !SKIP
                StreamPreviewView(
                    application: app,
                    hostCapabilities: hostCapabilities,
                    onInvoke: onInvoke,
                    mediaRecords: mediaRecords,
                    onKeyframeNeeded: onKeyframeNeeded
                )
#else
                StreamPreviewView(
                    application: app,
                    hostCapabilities: hostCapabilities,
                    onInvoke: onInvoke
                )
#endif
            }
        }
    }
}

private struct ApplicationTile: View {
    let application: RemoteApplication

    var body: some View {
        VStack(alignment: .leading, spacing: 17) {
            VistaSymbol(name: application.symbolName, size: 31)
                .frame(width: 58, height: 58)
                .background(VistaPalette.elevated, in: RoundedRectangle(cornerRadius: 16))

            Spacer(minLength: 2)

            VStack(alignment: .leading, spacing: 5) {
                Text(application.name)
                    .font(.headline)
                    .foregroundStyle(.white)
                    .lineLimit(1)
                Text(application.subtitle)
                    .font(.caption)
                    .foregroundStyle(VistaPalette.subtleText)
                Text(application.source.description)
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(VistaPalette.accent)
                    .padding(.top, 3)
            }
        }
        .padding(18)
        .frame(maxWidth: .infinity, minHeight: 187, alignment: .leading)
        .background(VistaPalette.surface, in: RoundedRectangle(cornerRadius: 21))
        #if !SKIP
        .accessibilityElement(children: .combine)
        #endif
    }
}
