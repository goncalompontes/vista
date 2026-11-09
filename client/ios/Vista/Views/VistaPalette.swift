import SwiftUI

enum VistaPalette {
    static let background = Color(red: 0.055, green: 0.067, blue: 0.105)
    static let surface = Color(red: 0.11, green: 0.13, blue: 0.19)
    static let elevated = Color(red: 0.17, green: 0.19, blue: 0.27)
    static let accent = Color(red: 0.50, green: 0.64, blue: 0.98)
    static let subtleText = Color(red: 0.67, green: 0.72, blue: 0.82)
}

struct VistaLogo: View {
    var size: CGFloat = 68

    var body: some View {
        #if SKIP
        ZStack {
            Color.clear
            RoundedRectangle(cornerRadius: size * 0.25, style: .continuous)
                .fill(Color(red: 0.50, green: 0.64, blue: 0.98))
            RoundedRectangle(cornerRadius: size * 0.10, style: .continuous)
                .fill(Color.white)
                .padding(size * 0.30)
        }
        .frame(width: size, height: size)
        .accessibilityLabel("Vista")
        #else
        Image(systemName: "rectangle.on.rectangle")
            .font(.system(size: size * 0.45, weight: .semibold, design: .rounded))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(
                LinearGradient(
                    colors: [VistaPalette.accent, Color(red: 0.43, green: 0.39, blue: 0.80)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                ),
                in: RoundedRectangle(cornerRadius: size * 0.25, style: .continuous)
            )
            .accessibilityLabel("Vista")
        #endif
    }
}


/// SF Symbols are not all mapped by SkipUI on Android; this wrapper keeps the
/// shared views readable on both platforms.
struct VistaSymbol: View {
    let name: String
    var size: CGFloat = 18

    var body: some View {
        #if SKIP
        Text(Self.glyph(for: name))
            .font(.system(size: size, weight: .medium))
            .foregroundStyle(VistaPalette.accent)
        #else
        Image(systemName: name)
            .font(.system(size: size, weight: .medium))
            .foregroundStyle(VistaPalette.accent)
        #endif
    }

    private static func glyph(for name: String) -> String {
        switch name {
        case "globe": return "◎"
        case "terminal": return "⌘"
        case "chevron.left.forwardslash.chevron.right": return "</>"
        case "music.note": return "♫"
        case "macwindow": return "▣"
        case "square.stack.3d.up": return "▤"
        case "hand.draw": return "☝"
        case "wifi.slash": return "⌁"
        case "video.slash": return "⊘"
        case "arrow.right": return "→"
        case "arrow.left": return "←"
        default: return "•"
        }
    }
}
