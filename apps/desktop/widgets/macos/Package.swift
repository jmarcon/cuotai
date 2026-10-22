// swift-tools-version:5.9
//
// PRD 0014 RF3.1: a SwiftPM manifest so the WidgetKit sources can be compiled
// outside Xcode. It exists for `swift build` in CI and for CodeQL's Swift
// autobuilder, which needs a package or an Xcode project to trace. It does not
// package, sign or install the widget extension: the `.appex`, the app group
// and the signing identity wait for the Apple developer account (PRD 0019).
import PackageDescription

let package = Package(
    name: "CuotaiWidgets",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "CuotaiWidgets", targets: ["CuotaiWidgets"])
    ],
    targets: [
        .target(name: "CuotaiWidgets", path: "Sources")
    ]
)
