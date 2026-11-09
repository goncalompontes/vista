// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "VistaCore",
    platforms: [.iOS(.v17), .macOS(.v13)],
    products: [.library(name: "VistaCore", targets: ["VistaCore"])],
    dependencies: [
        .package(url: "https://github.com/apple/swift-crypto.git", from: "3.15.1")
    ],
    targets: [
        .target(
            name: "VistaCore",
            dependencies: [
                .product(
                    name: "Crypto",
                    package: "swift-crypto",
                    condition: .when(platforms: [.linux, .macOS, .iOS])
                )
            ]
        ),
        .testTarget(name: "VistaCoreTests", dependencies: ["VistaCore"])
    ]
)
