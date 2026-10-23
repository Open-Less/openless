import OpenLessShared
import SwiftUI
import UIKit

@main
struct OpenLessApp: App {
  @UIApplicationDelegateAdaptor(KeyboardVoiceAppDelegate.self) private var voiceLifecycle
  @StateObject private var controller = AppController.shared
  @State private var selectedTab = 0
  @Environment(\.scenePhase) private var scenePhase
  var body: some Scene {
    WindowGroup {
      RootView(controller: controller, tab: $selectedTab)
        .id(controller.runtimeID)
        .overlay { if controller.restarting { ProgressView("正在重载应用…").padding(24).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20)) } }
        .tint(AppDesign.accent)
        .modifier(InterfacePreferenceModifier(preferences: controller.interfacePreferences))
        .overlay(alignment: .topTrailing) {
          if controller.preferences.backgroundKeepAlive {
            PiPInlineSource(keepAlive: controller.keepAlive)
              .frame(width: 48, height: 27)
              .padding(12)
              .allowsHitTesting(false)
              .accessibilityHidden(true)
          }
        }
        .onChange(of: scenePhase) { phase in
          if phase == .active { controller.interfacePreferences.reload() }
          controller.foregroundChanged(phase == .active)
        }
    }
  }
}

enum HomeRoute: Hashable { case keyboard, installation }

struct RootView: View {
  @ObservedObject var controller: AppController
  @Binding var tab: Int
  @State private var homePath: [HomeRoute] = []
  @State private var styleSection = StyleLibrarySection.library
  @State private var keyboardVisible = false
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  private var dockVisible: Bool { !keyboardVisible && !(tab == 0 && !homePath.isEmpty) }
  var body: some View {
    // Keep the selected page and its navigation chrome visible from the first
    // frame. Fading a whole NavigationStack exposes UIKit's blank tab backing.
    TabView(selection: $tab) {
      NavigationStack(path: $homePath) {
        HomeView(controller: controller)
          .navigationDestination(for: HomeRoute.self) { route in
            switch route {
            case .keyboard: KeyboardTrialView(controller: controller)
            case .installation: KeyboardInstallationView(controller: controller)
            }
          }
      }
        .toolbar(.hidden, for: .tabBar)
        .tabItem { Label("概览", systemImage: "chart.bar.xaxis") }.tag(0)
      NavigationStack { HistoryView(controller: controller, library: controller.library) }
        .toolbar(.hidden, for: .tabBar)
        .tabItem { Label("历史", systemImage: "clock") }.tag(1)
      NavigationStack { VocabularyView(library: controller.library, controller: controller) }
        .toolbar(.hidden, for: .tabBar)
        .tabItem { Label("词典", systemImage: "character.book.closed") }.tag(3)
      NavigationStack { StyleLibrarySettingsDestination(controller: controller, section: $styleSection) }
        .toolbar(.hidden, for: .tabBar)
        .tabItem { Label("风格", systemImage: "sparkles") }.tag(4)
      NavigationStack { SettingsView(controller: controller, onOpenStyles: { tab = 4 }) }
        .toolbar(.hidden, for: .tabBar)
        .tabItem { Label("设置", systemImage: "slider.horizontal.3") }.tag(2)
    }
      .toolbar(.hidden, for: .tabBar)
    .background(DockSafeAreaBridge(height: dockVisible ? 78 : 0).frame(width: 0, height: 0))
    // One persistent dock instance: fading it avoids a second copy (and duplicate
    // accessibility elements) when the keyboard hides and shows in quick succession.
    .overlay(alignment: .bottom) {
      ZStack(alignment: .bottom) {
        DockScrim()
        ProductDock(tab: $tab, section: $styleSection, hapticsEnabled: controller.preferences.haptics)
          .padding(.horizontal, 16).padding(.top, 8).padding(.bottom, 6)
      }
      .opacity(dockVisible ? 1 : 0)
      .allowsHitTesting(dockVisible)
      .accessibilityHidden(!dockVisible)
    }
    .animation(ProductMotion.resolve(ProductMotion.quick, reduce: reduceMotion), value: dockVisible)
    .background(AppDesign.canvas)
    .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillShowNotification)) { _ in
      DispatchQueue.main.async { keyboardVisible = true }
    }
    .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardWillHideNotification)) { _ in
      DispatchQueue.main.async { keyboardVisible = false }
    }
    .onOpenURL { url in
      guard url.scheme == "openless-ios" else { return }
      if url.host == "settings" { tab = 2 }
      else if url.host == "styles" { tab = 4 }
      // Voice activation is handled by KeyboardVoiceSceneDelegate so the
      // system-provided source application survives the SwiftUI URL boundary.
      // Only the matching, fresh keyboard voice request can authorize auto-start.
    }
    .sheet(item: Binding(get: { controller.hostActivation }, set: { value in
      if value == nil { controller.closeHostActivation() }
    })) { _ in HostVoiceActivationView(controller: controller) }
    .alert("需要下载系统语音资源", isPresented: $controller.needsDownload) {
      Button("暂不下载", role: .cancel) {}
      Button("下载并准备") { Task { await controller.prepareSpeech(allowDownload: true) } }
    } message: {
      Text("Apple 将管理中英混合识别所需的系统语音资源。下载大小由系统提供，建议连接无线网络。准备资源不会开始录音。")
    }
  }
}

