import SwiftUI
import VistaCore

struct StreamPreviewView: View {
    let application: RemoteApplication
    var hostCapabilities: CapabilitySnapshot?
    var onInvoke: ((String) -> Void)?
#if !SKIP
    var mediaRecords: [H264AccessUnit] = []
    var onKeyframeNeeded: (() -> Void)?
#endif

    var body: some View {
        ScrollView {
            VStack(spacing: 22) {
#if canImport(VideoToolbox) && !SKIP
                VideoStreamView(mediaRecords: mediaRecords, onKeyframeNeeded: onKeyframeNeeded)
#else
                ZStack {
                    RoundedRectangle(cornerRadius: 25)
                        .fill(VistaPalette.surface)

                    VStack(spacing: 16) {
                        VistaSymbol(name: application.symbolName, size: 56)
                        VistaSymbol(name: "video.slash", size: 20)
                        Text("No active stream")
                            .font(.headline)
                            .foregroundStyle(.white)
                        Text("Live video will appear here once a Linux host, capture pipeline and secure transport are connected.")
                            .font(.footnote)
                            .foregroundStyle(VistaPalette.subtleText)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 310)
                    }
                    .padding(25)
                }
                .aspectRatio(1.25, contentMode: .fit)
#endif

                VStack(alignment: .leading, spacing: 10) {
                    Text(application.name)
                        .font(.title2.bold())
                        .foregroundStyle(.white)
                    Text(application.source.description)
                        .font(.subheadline)
                        .foregroundStyle(VistaPalette.accent)
#if !SKIP
                    let streamDescription = mediaRecords.isEmpty
                        ? "Waiting for an authenticated host video stream."
                        : "Receiving authenticated H.264 frames from the paired host."
#else
                    let streamDescription = "Waiting for an authenticated host video stream."
#endif
                    Text(streamDescription)
                        .font(.subheadline)
                        .foregroundStyle(VistaPalette.subtleText)
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                HStack(spacing: 16) {
                    control("pointer", label: "Pointer")
                    control("keyboard", label: "Keyboard")
                    control("speaker.wave.2", label: "Audio")
                }

                CapabilityControlsView(
                    snapshot: hostCapabilities ?? DemoCatalog.capabilitySnapshot,
                    onInvoke: onInvoke ?? { _ in },
                    isLive: hostCapabilities != nil
                )
            }
            #if SKIP
            .frame(maxWidth: 380)
            #else
            .frame(maxWidth: 760)
            #endif
            .padding(20)
            .frame(maxWidth: .infinity)
        }
        .background(VistaPalette.background.ignoresSafeArea())
        .navigationTitle(application.name)
        .navigationBarTitleDisplayMode(.inline)
    }

    private func control(_ symbol: String, label: String) -> some View {
        HStack(spacing: 6) {
            VistaSymbol(name: symbol, size: 16)
            Text(label)
                .font(.caption.weight(.semibold))
                .foregroundStyle(VistaPalette.subtleText)
        }
        .frame(maxWidth: .infinity, minHeight: 52)
        .background(VistaPalette.surface, in: RoundedRectangle(cornerRadius: 14))
        .accessibilityLabel("\(label) unavailable in preview")
    }
}
