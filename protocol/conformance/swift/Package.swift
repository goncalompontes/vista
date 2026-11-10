// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "VistaProtobufCandidate",
    platforms: [.macOS(.v13), .iOS(.v16)],
    products: [.library(name: "VistaCandidate", targets: ["VistaCandidate"])],
    dependencies: [.package(url: "https://github.com/apple/swift-protobuf.git", exact: "1.29.0")],
    targets: [
        .target(name: "VistaCandidate", dependencies: [.product(name: "SwiftProtobuf", package: "swift-protobuf")]),
        .testTarget(name: "VistaCandidateTests", dependencies: ["VistaCandidate"])
    ]
)
