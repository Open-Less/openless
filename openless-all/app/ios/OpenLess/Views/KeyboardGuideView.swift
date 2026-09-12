import SwiftUI
import UIKit

struct KeyboardGuideView: View {
    var showDoneButton = true
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                EmptyMessage(symbol: "keyboard", title: "把声音带到每个输入框",
                             message: "先在 OpenLess 中听写，再回到正在写字的应用，用键盘插入整理好的文字。")
                step(1, "添加 OpenLess 键盘", "打开系统设置 → 通用 → 键盘 → 键盘 → 添加新键盘，选择 OpenLess。随后点开 OpenLess，启用“允许完全访问”。")
                step(2, "完成听写，发送文字", "在听写页点击“发送到键盘”。只有这次选择的文字会进入键盘暂存。")
                step(3, "切回目标应用，点击插入", "长按键盘的地球图标切换到 OpenLess，再点一条文字即可插入光标处。点刷新可读取刚发送的内容。")
                Surface {
                    VStack(alignment: .leading, spacing: 10) {
                        Label("关于键盘权限", systemImage: "hand.raised").font(.headline)
                        Text("完全访问用于读取主应用共享的文字。此键盘没有联网代码，不读取系统剪贴板，也不收集你在其他应用输入的内容。")
                        Text("iOS 不允许第三方键盘直接使用麦克风。密码框、电话输入框或禁用第三方键盘的应用可能使用系统键盘；这些位置可使用主应用的复制功能。")
                    }.font(.subheadline).foregroundStyle(.secondary)
                }
                Button {
                    if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
                } label: {
                    Label("打开系统设置", systemImage: "gearshape")
                        .frame(maxWidth: .infinity).padding(.vertical, 8)
                }.buttonStyle(.borderedProminent)
            }.padding(24).frame(maxWidth: 680).frame(maxWidth: .infinity)
        }
        .background(OpenLessTheme.canvas)
        .navigationTitle("使用 OpenLess 键盘").navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if showDoneButton {
                ToolbarItem(placement: .confirmationAction) { Button("完成") { dismiss() } }
            }
        }
    }

    private func step(_ number: Int, _ title: String, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 14) {
            Text("\(number)").font(.subheadline.weight(.semibold)).foregroundStyle(.white)
                .frame(width: 30, height: 30).background(OpenLessTheme.accent, in: Circle())
            VStack(alignment: .leading, spacing: 8) {
                Text(title).font(.headline)
                Text(text).font(.subheadline).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}
