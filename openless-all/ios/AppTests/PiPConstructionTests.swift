import AVKit
import XCTest
@testable import OpenLess

nonisolated final class PiPConstructionTests: XCTestCase {
  @MainActor
  func testInstalledRuntimeAcceptsHiddenControllerConstruction() throws {
    // Check real AVKit ABI/construction without starting PiP. This does not
    // establish that the remote service accepts background coexistence.
    guard AVPictureInPictureController.isPictureInPictureSupported() else {
      throw XCTSkip("Installed runtime does not advertise PiP support; use the target iPhone for AVKit construction verification.")
    }
    let pip = ExperimentalPiPKeepAlive()
    defer { pip.stop() }
    pip.arm()
    let attachment = XCTAttachment(string: pip.diagnosticReport)
    attachment.name = "Installed AVKit construction"
    attachment.lifetime = .keepAlways
    add(attachment)
    XCTAssertTrue(pip.diagnosticReport.contains("provider=true"), pip.diagnosticReport)
    XCTAssertFalse(pip.isReady, "Construction alone must not grant background recording.")
  }
}
