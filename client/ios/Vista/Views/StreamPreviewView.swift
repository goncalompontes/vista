import SwiftUI
import VistaCore

struct StreamPreviewView: View {
    let title: String
    let subtitle: String
    let symbolName: String
    let target: StreamTarget
    var hostCapabilities: CapabilitySnapshot?
    var streamMessage: String?
    var isStreaming = false
    var inputEnabled = false
    var geometryEpoch: UInt64 = 1
    var onInvoke: ((String) -> Void)?
    var onSelect: ((StreamTarget) -> Void)?
    var onStop: (() -> Void)?
    var onInput: ((PortableInputEvent) -> Void)?
#if !SKIP
    var mediaRecords: [H264AccessUnit] = []
    var mediaRecordSequence: UInt64 = 0
    var onKeyframeNeeded: (() -> Void)?
#endif
    @State private var text = ""
    @State private var lastPointer: CGPoint?

    var body: some View {
        ScrollView {
            VStack(spacing: 20) {
#if canImport(VideoToolbox) && !SKIP
                ZStack {
                    VideoStreamView(
                        mediaRecords: mediaRecords,
                        mediaRecordSequence: mediaRecordSequence,
                        onKeyframeNeeded: onKeyframeNeeded
                    )
                    if inputEnabled {
                        pointerSurface
                    }
                }
#else
                ZStack {
                    RoundedRectangle(cornerRadius: 25)
                        .fill(VistaPalette.surface)

                    VStack(spacing: 16) {
                        VistaSymbol(name: symbolName, size: 56)
                        VistaSymbol(name: isStreaming ? "video" : "video.slash", size: 20)
                        Text(isStreaming ? "Stream selected" : "No active stream")
                            .font(.headline)
                            .foregroundStyle(.white)
                        Text(streamMessage ?? "Live video appears here after the host starts the selected target.")
                            .font(.footnote)
                            .foregroundStyle(VistaPalette.subtleText)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 310)
                    }
                    .padding(25)
                }
                .aspectRatio(16.0 / 9.0, contentMode: .fit)
#endif

                VStack(alignment: .leading, spacing: 8) {
                    Text(title)
                        .font(.title2.bold())
                        .foregroundStyle(.white)
                    Text(subtitle)
                        .font(.subheadline)
                        .foregroundStyle(VistaPalette.accent)
                    Text(
                        streamMessage
                            ?? (isStreaming
                                ? "Receiving authenticated H.264 frames from the paired host."
                                : "Selecting this target on the host…")
                    )
                    .font(.subheadline)
                    .foregroundStyle(VistaPalette.subtleText)
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                if inputEnabled {
                    inputBar
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
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { onSelect?(target) }
        .onDisappear { onStop?() }
    }

#if !SKIP
    private var pointerSurface: some View {
        GeometryReader { geometry in
            Color.clear
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { value in
                            lastPointer = value.location
                            sendPointer(value.location, in: geometry.size)
                        }
                        .onEnded { value in
                            lastPointer = value.location
                            sendPointer(value.location, in: geometry.size)
                            onInput?(.pointerButton(button: .primary, pressed: true))
                            onInput?(.pointerButton(button: .primary, pressed: false))
                        }
                )
        }
        .clipShape(RoundedRectangle(cornerRadius: 20))
    }

    private func sendPointer(_ location: CGPoint, in size: CGSize) {
        guard size.width > 0, size.height > 0 else { return }
        let x = min(max(location.x / size.width, 0), 1)
        let y = min(max(location.y / size.height, 0), 1)
        onInput?(.pointerAbsolute(x: x, y: y, geometryEpoch: geometryEpoch))
    }
#endif

    private var inputBar: some View {
        VStack(spacing: 10) {
            HStack(spacing: 8) {
                TextField("Keyboard input", text: $text)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .textFieldStyle(.roundedBorder)
                Button("Send") {
                    guard !text.isEmpty else { return }
                    onInput?(.textCommit(text: text))
                    text = ""
                }
                .buttonStyle(.borderedProminent)
                .tint(VistaPalette.accent)
            }
            HStack(spacing: 8) {
                keyButton("Esc", usage: 0x29)
                keyButton("Tab", usage: 0x2B)
                keyButton("Enter", usage: 0x28)
                keyButton("⌫", usage: 0x2A)
                keyButton("←", usage: 0x50)
                keyButton("→", usage: 0x4F)
            }
        }
        .padding(12)
        .background(VistaPalette.surface, in: RoundedRectangle(cornerRadius: 16))
    }

    private func keyButton(_ label: String, usage: UInt16) -> some View {
        Button(label) {
            onInput?(.keyPhysical(usage: usage, pressed: true, isRepeat: false))
            onInput?(.keyPhysical(usage: usage, pressed: false, isRepeat: false))
        }
        .buttonStyle(.bordered)
        .tint(VistaPalette.accent)
    }
}
