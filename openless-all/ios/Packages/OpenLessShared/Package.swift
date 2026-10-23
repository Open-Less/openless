// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "OpenLessShared",
    platforms: [.iOS(.v16), .macOS(.v14)],
    products: [.library(name: "OpenLessShared", targets: ["OpenLessShared"])],
    targets: [
        .systemLibrary(name: "CSQLite", pkgConfig: "sqlite3"),
        .target(name: "OpenLessNativeBridge", linkerSettings: [.linkedFramework("Foundation")]),
        .target(name: "OpenLessShared", dependencies: ["CSQLite", "OpenLessNativeBridge"]),
        .testTarget(name: "OpenLessSharedTests", dependencies: ["OpenLessShared"])
    ]
)
