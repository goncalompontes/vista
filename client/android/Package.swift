// swift-tools-version: 6.1
// Skip project for the Vista Android client.
//
// The shared SwiftUI views and VistaCore models are copied into Sources/Vista by
// ./sync-sources.sh. Canonical sources live in client/ios/Vista and
// client/core/Sources/VistaCore.
import PackageDescription

let package = Package(
    name: "VistaSkip",
    defaultLocalization: "en",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "Vista", type: .dynamic, targets: ["Vista"]),
    ],
    dependencies: [
        .package(url: "https://github.com/skiptools/skip.git", from: "1.9.20"),
        .package(url: "https://github.com/skiptools/skip-ui.git", from: "1.0.0"),
    ],
    targets: [
        .target(name: "Vista", dependencies: [
            .product(name: "SkipUI", package: "skip-ui"),
        ], plugins: [.plugin(name: "skipstone", package: "skip")]),
    ]
)
