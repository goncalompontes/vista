// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "VistaCore",
    platforms: [.iOS(.v17), .macOS(.v13)],
    products: [.library(name: "VistaCore", targets: ["VistaCore"])],
    targets: [
        .target(name: "VistaCore"),
        .testTarget(name: "VistaCoreTests", dependencies: ["VistaCore"])
    ]
)
