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
    @State private var filter: LibraryFilter = .all

    private var applications: [RemoteApplication] {
        DemoCatalog.applications.filter { filter.includes($0) }
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
                        Text("DEMO")
                            .font(.caption2.weight(.bold))
                            .tracking(1.2)
                            .padding(.horizontal, 11)
                            .padding(.vertical, 7)
                            .background(VistaPalette.elevated, in: Capsule())
                            .foregroundStyle(VistaPalette.accent)
                    }

                    HStack(alignment: .top, spacing: 14) {
                        VistaSymbol(name: "wifi.slash")
                        VStack(alignment: .leading, spacing: 5) {
                            Text("No Linux host paired")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(.white)
                            Text("This is a local interface preview. The catalog below is sample data, not discovered windows.")
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

                    Text("Streaming, host pairing and input are not available in this preview.")
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
                StreamPreviewView(application: app)
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
