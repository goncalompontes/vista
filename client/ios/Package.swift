// swift-tools-version: 6.0
import PackageDescription

// xtool builds this executable-style library target as an iOS application.
// Keep it in sync with project.yml, which remains the XcodeGen source of truth
// for builds on macOS. Both paths compile the same SwiftUI source files.
let package = Package(
    name: "Vista",
    platforms: [.iOS(.v17)],
    products: [
        .library(name: "Vista", targets: ["Vista"])
    ],
    dependencies: [
        .package(path: "../core")
    ],
    targets: [
        .target(
            name: "Vista",
            dependencies: [
                .product(name: "VistaCore", package: "core")
            ],
            path: "Vista"
        )
    ]
)
