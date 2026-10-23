import SwiftUI
import UIKit

struct NativeSearchField: UIViewRepresentable {
  @Binding var text: String
  var placeholder: String
  var identifier: String
  var focused: Bool = false
  var onIconTap: (() -> Void)? = nil
  @Environment(\.locale) private var locale

  func makeCoordinator() -> Coordinator { Coordinator(text: $text) }
  func makeUIView(context: Context) -> UISearchTextField {
    let field = UISearchTextField()
    field.autocapitalizationType = .none
    field.autocorrectionType = .no
    field.returnKeyType = .search
    field.delegate = context.coordinator
    field.addTarget(context.coordinator, action: #selector(Coordinator.changed), for: .editingChanged)
    field.backgroundColor = .clear
    field.font = .preferredFont(forTextStyle: .body)
    field.adjustsFontForContentSizeCategory = true
    field.setContentHuggingPriority(.defaultLow, for: .horizontal)
    return field
  }
  func updateUIView(_ field: UISearchTextField, context: Context) {
    context.coordinator.text = $text
    if field.text != text { field.text = text }
    let language = locale.language.languageCode?.identifier == "en" ? "en" : "zh-Hans"
    let bundle = Bundle.main.path(forResource: language, ofType: "lproj").flatMap(Bundle.init(path:)) ?? .main
    field.placeholder = bundle.localizedString(forKey: placeholder, value: placeholder, table: nil)
    field.accessibilityIdentifier = identifier
    context.coordinator.onIconTap = onIconTap
    if onIconTap != nil, !(field.leftView is UIButton) {
      let icon = UIButton(type: .system)
      icon.setImage(UIImage(systemName: "magnifyingglass"), for: .normal)
      icon.tintColor = .label
      icon.frame = CGRect(x: 0, y: 0, width: 44, height: 44)
      icon.accessibilityLabel = bundle.localizedString(forKey: "关闭搜索", value: "关闭搜索", table: nil)
      icon.accessibilityIdentifier = "searchVocabulary"
      icon.addTarget(context.coordinator, action: #selector(Coordinator.iconTapped), for: .touchUpInside)
      field.leftView = icon
      field.leftViewMode = .always
    }
    if context.coordinator.requestedFocus != focused {
      context.coordinator.requestedFocus = focused
      DispatchQueue.main.async { [weak field] in
        guard let field else { return }
        if focused { field.becomeFirstResponder() } else { field.resignFirstResponder() }
      }
    }
  }
  final class Coordinator: NSObject, UITextFieldDelegate {
    var text: Binding<String>
    var requestedFocus = false
    var onIconTap: (() -> Void)?
    @objc func iconTapped() { onIconTap?() }
    init(text: Binding<String>) { self.text = text }
    @objc func changed(_ field: UITextField) { text.wrappedValue = field.text ?? "" }
    func textFieldShouldReturn(_ textField: UITextField) -> Bool { textField.resignFirstResponder(); return true }
  }
}

struct SearchGlass: ViewModifier {
  func body(content: Content) -> some View {
    if #available(iOS 26.0, *) { content.glassEffect(.regular, in: Capsule()) }
    else { content.background(.regularMaterial, in: Capsule()) }
  }
}

struct PinnedSearchBar: View {
  @Binding var text: String
  let placeholder: String
  let identifier: String
  var body: some View {
    NativeSearchField(text: $text, placeholder: placeholder, identifier: identifier)
      .frame(height: 48)
      .padding(.horizontal, AppDesign.Space.page).padding(.vertical, 8)
      .background(AppDesign.canvas)
      .overlay(alignment: .bottom) { Rectangle().fill(AppDesign.outline).frame(height: 0.5) }
  }
}
