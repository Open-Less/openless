import SwiftUI
import WidgetKit

/// iOS 18 deployment: both controls and Live Activities use the existing host intent.
@main
struct OpenLessRecordingWidgetBundle: WidgetBundle {
  var body: some Widget {
    OpenLessRecordingControl()
    OpenLessRecordingLiveActivity()
  }
}
