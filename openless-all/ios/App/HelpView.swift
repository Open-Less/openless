import SwiftUI

struct HelpView: View {
  var body: some View {
    Form {
      Section("开始说话") {
        Text("先在 App 中完成麦克风授权和语音准备，再主动开始录音。轻点开始，再次轻点结束。")
        Text("默认在本机识别声音。")
      }
      Section("系统录音") {
        SystemRecordingInstructions()
      }
      Section("键盘与完全访问") {
        Text("在系统设置 → 通用 → 键盘中添加 OpenLess，然后在输入框长按地球键切换。基础中文和英文打字不需要完全访问；共享偏好和语音控制需要。")
        Text("系统密码框和不支持第三方键盘的应用无法使用 OpenLess。")
      }
      Section("风格、翻译与帮写") {
        Text("在风格库查看、导入或安装市场风格，从详情选择使用。翻译和帮写需要先配置支持的文字模型；帮写结果预览后确认插入。")
      }
      Section("历史与录音") {
        Text("历史可查看、复制原文和处理结果；配置文字模型后可重新润色预览。未完成识别的录音可恢复、播放或导出。")
      }
      Section("词典与统计") {
        Text("词典支持新增、编辑、启用、删除和确认建议。已确认并启用的词条可用于识别上下文，并在允许完全访问后共享给中文输入词典。")
        Text("今日指标来自当前历史；近 7 天、30 天来自独立活动统计，清空历史不会清空这些趋势。平均段落表示今日每段记录的平均时长。")
      }
      Section("账户与同步") {
        Text("在个人主页登录 GitHub。同步暂不可用。")
      }
      Section("提供商设置") {
        Text("在风格页打开文字模型设置，可添加你使用的服务。密钥保存在此设备。")
      }
    }.font(.body).navigationTitle("帮助中心").navigationBarTitleDisplayMode(.inline)
  }
}

struct ReleaseNotesView: View {
  var body: some View {
    Form {
      Section("0.1.0") {
        Label("本地语音识别与中文键盘", systemImage: "waveform")
        Label("iOS 18+ 控制中心录音与实时活动", systemImage: "mic.circle")
        Label("风格库、市场安装、翻译与帮写预览", systemImage: "sparkles")
        Label("已确认词条建议与中文词典共享", systemImage: "text.book.closed")
        Label("历史详情、原文复制、删除与重新润色预览", systemImage: "clock.arrow.circlepath")
        Label("未完成识别的录音恢复与导出", systemImage: "waveform.path")
        Label("概览统计、词典管理与应用外观", systemImage: "chart.bar")
      }
      Section("账户与同步") {
        Text("在个人主页登录 GitHub。同步暂不可用。")
          .foregroundStyle(.secondary)
      }
    }.navigationTitle("版本说明").navigationBarTitleDisplayMode(.inline)
  }
}
