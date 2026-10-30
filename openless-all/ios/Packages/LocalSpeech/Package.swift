// swift-tools-version: 6.0
import PackageDescription

let package = Package(
  name: "LocalSpeech",
  platforms: [.iOS(.v16)],
  products: [.library(name: "LocalSpeech", targets: ["LocalSpeech"])],
  targets: [
    .binaryTarget(name: "SherpaOnnxIOS", path: "Frameworks/sherpa-onnx.xcframework"),
    .binaryTarget(name: "OnnxruntimeIOS", path: "Frameworks/onnxruntime.xcframework"),
    .target(name: "LocalSpeech", dependencies: ["SherpaOnnxIOS", "OnnxruntimeIOS"],
            linkerSettings: [.linkedLibrary("c++")])
  ]
)
