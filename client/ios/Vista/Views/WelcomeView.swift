import SwiftUI

struct WelcomeView: View {
    let onExplore: () -> Void

    var body: some View {
        GeometryReader { geometry in
            ScrollView {
                VStack(spacing: 22) {
                    Spacer(minLength: 34)

                    VistaLogo(size: 92)
                        .padding(.bottom, 14)

                    VStack(spacing: 4) {
                        Text("Your Linux apps,")
                            .foregroundStyle(.white)
                        Text("anywhere.")
                            .foregroundStyle(VistaPalette.accent)
                    }
                    .font(.system(size: 38, weight: .bold, design: .rounded))
                    .multilineTextAlignment(.center)
                    .minimumScaleFactor(0.8)

                    Text("A native home for your Linux desktop apps on iPhone and iPad.")
                        .font(.body)
                        .foregroundStyle(VistaPalette.subtleText)
                        .multilineTextAlignment(.center)
                        .fixedSize(horizontal: false, vertical: true)
                        #if SKIP
                        .frame(maxWidth: 330)
                        #endif

                    VStack(alignment: .leading, spacing: 17) {
                        feature("macwindow", "Your open windows")
                        feature("square.stack.3d.up", "Independent app sessions")
                        feature("hand.draw", "Native touch and keyboard")
                    }
                    .padding(22)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(VistaPalette.surface, in: RoundedRectangle(cornerRadius: 24))
                    .padding(.top, 14)

                    Spacer(minLength: 32)

                    Button(action: onExplore) {
                        HStack(spacing: 10) {
                            Text("Explore the app preview")
                            #if SKIP
                            Text("→")
                            #else
                            Image(systemName: "arrow.right")
                            #endif
                        }
                        .font(.headline)
                        .frame(maxWidth: .infinity, minHeight: 54)
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(VistaPalette.accent)

                    Text("Preview only · No host connection or streaming yet")
                        .font(.caption)
                        .foregroundStyle(VistaPalette.subtleText)
                        .multilineTextAlignment(.center)
                }
                .frame(maxWidth: 520)
                .padding(.horizontal, 26)
                .padding(.vertical, 24)
                .frame(maxWidth: .infinity)
                .frame(minHeight: geometry.size.height)
            }
        }
        .background(VistaPalette.background.ignoresSafeArea())
    }

    private func feature(_ symbol: String, _ title: String) -> some View {
        HStack(spacing: 14) {
            Image(systemName: symbol)
                .font(.system(size: 19))
                .foregroundStyle(VistaPalette.accent)
                .frame(width: 26)
            Text(title)
                .font(.subheadline.weight(.medium))
                .foregroundStyle(.white)
        }
    }
}
