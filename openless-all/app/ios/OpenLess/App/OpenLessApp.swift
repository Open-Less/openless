import SwiftUI

@main
@MainActor
struct OpenLessApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(model)
                .tint(OpenLessTheme.accent)
                .preferredColorScheme(model.colorScheme)
        }
    }
}

struct RootView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.scenePhase) private var scenePhase
    @State private var confirmURLRecording = false

    var body: some View {
        TabView(selection: $model.selectedTab) {
            NavigationStack { DictationView() }
                .tabItem { Label("听写", systemImage: "waveform") }.tag(0)
            NavigationStack { HistoryView() }
                .tabItem { Label("历史", systemImage: "clock.arrow.circlepath") }.tag(1)
            NavigationStack { LibraryView() }
                .tabItem { Label("词典与风格", systemImage: "square.grid.2x2") }.tag(2)
            NavigationStack { SettingsView(settings: model.settings) }
                .tabItem { Label("设置", systemImage: "slider.horizontal.3") }.tag(3)
        }
        .overlay(alignment: .top) {
            if let feedback = model.feedback {
                Label(feedback, systemImage: "checkmark.circle.fill")
                    .font(.subheadline.weight(.medium))
                    .padding(.horizontal, 18).padding(.vertical, 12)
                    .background(.regularMaterial, in: Capsule())
                    .padding(.horizontal, 20).padding(.top, 8)
                    .accessibilityAddTraits(.updatesFrequently)
                    .allowsHitTesting(false)
            }
        }
        .alert(item: $model.alert) { notice in
            Alert(title: Text(notice.title), message: Text(notice.message), dismissButton: .default(Text("知道了")))
        }
        .confirmationDialog("开始一次新的听写？", isPresented: $confirmURLRecording, titleVisibility: .visible) {
            Button("开始听写") { model.startRecording() }
            Button("取消", role: .cancel) {}
        } message: {
            Text("当前文字会保存到历史。麦克风将在确认后开启。")
        }
        .onOpenURL { url in
            guard url.scheme?.lowercased() == "openless", url.host == "dictate" else { return }
            model.selectedTab = 0
            if !model.isBusy { confirmURLRecording = true }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .background { model.sceneDidEnterBackground() }
            else if phase == .inactive { model.persistDraft() }
        }
    }
}
