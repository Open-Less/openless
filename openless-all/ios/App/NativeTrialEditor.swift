import SwiftUI
import UIKit

/// UIKit owns text, selection and first-responder state. SwiftUI only sizes
/// this view in the native keyboard safe area.
struct NativeTrialEditor: UIViewRepresentable {
  @Binding var text: String
  @Binding var focused: Bool
  @Binding var selection: NSRange
  func makeCoordinator() -> Coordinator { Coordinator(self) }
  func makeUIView(context: Context) -> UITextView {
    let view = UITextView()
    view.delegate = context.coordinator
    view.font = .preferredFont(forTextStyle: .body)
    view.adjustsFontForContentSizeCategory = true
    view.backgroundColor = .clear
    view.textContainerInset = UIEdgeInsets(top: 8, left: 5, bottom: 8, right: 5)
    view.accessibilityLabel = "键盘试用输入框"
    view.accessibilityIdentifier = "keyboardTestEditor"
    return view
  }
  func updateUIView(_ view: UITextView, context: Context) {
    context.coordinator.parent = self
    context.coordinator.updating = true
    defer { context.coordinator.updating = false }
    if view.text != text { view.text = text }
    // Selection changes can arrive before didBeginEditing. Do not treat the
    // initial false binding as a request to cancel UIKit's in-flight focus.
    let dismissRequested = context.coordinator.lastFocus && !focused
    context.coordinator.lastFocus = focused
    if dismissRequested && view.isFirstResponder { view.resignFirstResponder() }
  }
  final class Coordinator: NSObject, UITextViewDelegate {
    var parent: NativeTrialEditor
    var lastFocus = false
    var updating = false
    init(_ parent: NativeTrialEditor) { self.parent = parent }
    func textViewDidChange(_ view: UITextView) { parent.text = view.text }
    func textViewDidBeginEditing(_ view: UITextView) {
      lastFocus = true
      parent.focused = true
    }
    func textViewDidEndEditing(_ view: UITextView) {
      if !updating && parent.focused { parent.focused = false }
    }
    func textViewDidChangeSelection(_ view: UITextView) {
      if !updating && parent.selection != view.selectedRange { parent.selection = view.selectedRange }
    }
  }
}

