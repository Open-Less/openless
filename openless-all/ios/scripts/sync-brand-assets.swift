import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

// Reuse the official art, never draw a substitute
// logo. The app icon is flattened onto white solely to satisfy iOS's no-alpha
// requirement. The transparent in-product mark is maintained separately.
let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let canonical = root.deletingLastPathComponent().appendingPathComponent("app/src-tauri/icons")
let assets = root.appendingPathComponent("Resources/Assets.xcassets")
let icon = assets.appendingPathComponent("AppIcon.appiconset")
let brand = root.appendingPathComponent("Resources/Brand.xcassets")
let mark = brand.appendingPathComponent("OpenLessBrand.imageset")
let colors = assets.appendingPathComponent("LaunchBackground.colorset")
for directory in [icon, mark, colors] {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
}
func json(_ value: [String: Any], _ url: URL) throws {
    try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys]).write(to: url, options: .atomic)
}
let info: [String: Any] = ["author": "xcode", "version": 1]
try json(["info": info], assets.appendingPathComponent("Contents.json"))
try json(["info": info], brand.appendingPathComponent("Contents.json"))
let officialIcon = canonical.appendingPathComponent("ios/AppIcon-512@2x.png")
guard let source = CGImageSourceCreateWithURL(officialIcon as CFURL, nil),
      let original = CGImageSourceCreateImageAtIndex(source, 0, nil),
      original.width == 1024, original.height == 1024,
      let context = CGContext(data: nil, width: 1024, height: 1024, bitsPerComponent: 8,
                              bytesPerRow: 1024 * 4, space: CGColorSpaceCreateDeviceRGB(),
                              bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else {
    fatalError("Cannot load the official 1024px OpenLess icon")
}
let bounds = CGRect(x: 0, y: 0, width: 1024, height: 1024)
context.setFillColor(CGColor(gray: 1, alpha: 1))
context.fill(bounds)
context.draw(original, in: bounds)
guard let opaque = context.makeImage(),
      let output = CGImageDestinationCreateWithURL(icon.appendingPathComponent("AppIcon.png") as CFURL,
                                                   UTType.png.identifier as CFString, 1, nil) else {
    fatalError("Cannot prepare the opaque iOS icon")
}
CGImageDestinationAddImage(output, opaque, nil)
guard CGImageDestinationFinalize(output) else { fatalError("Cannot save AppIcon.png") }
// Do not overwrite the transparent in-product mark with the SpringBoard tile.
guard FileManager.default.fileExists(atPath: mark.appendingPathComponent("OpenLessBrand.png").path) else {
    fatalError("Missing transparent OpenLessBrand.png from the checked-in asset catalog")
}
try json(["info": info, "images": [["filename": "AppIcon.png", "idiom": "universal", "platform": "ios", "size": "1024x1024"]]],
         icon.appendingPathComponent("Contents.json"))
try json(["info": info, "images": [["filename": "OpenLessBrand.png", "idiom": "universal"]]],
         mark.appendingPathComponent("Contents.json"))
let palette: [[String: Any]] = [
    ["idiom": "universal", "color": ["color-space": "srgb", "components": ["red": "0.965", "green": "0.966", "blue": "0.955", "alpha": "1"]]],
    ["idiom": "universal", "appearances": [["appearance": "luminosity", "value": "dark"]],
     "color": ["color-space": "srgb", "components": ["red": "0.075", "green": "0.079", "blue": "0.09", "alpha": "1"]]]
]
try json(["info": info, "colors": palette], colors.appendingPathComponent("Contents.json"))
print("Prepared the official Apple OpenLess icon without alpha; preserved the transparent in-product mark.")
