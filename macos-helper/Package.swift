// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "operator-macos-helper",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "operator-macos-helper",
            path: "Sources/OperatorMacOSHelper",
            linkerSettings: [
                .linkedFramework("ApplicationServices"),
                .linkedFramework("AppKit"),
            ]
        ),
    ]
)
