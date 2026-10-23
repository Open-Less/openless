import Charts
import SwiftUI

struct OverviewMetricsView: View {
  @ObservedObject var library: ProductLibrary
  @Environment(\.dynamicTypeSize) private var typeSize
  @State private var period = 7
  @State private var metric = Metric.count
  private enum Metric: String, CaseIterable {
    case count = "记录", characters = "字数", duration = "时长"
    func value(_ point: ActivityPoint) -> UInt64 {
      switch self { case .count: point.count; case .characters: point.chars; case .duration: point.durationMs }
    }
  }
  private var today: TodayMetrics { TodayMetrics(history: library.history) }
  private var points: [ActivityPoint] { HistoryFormatting.series(library.activity, period: period) }
  private var total: UInt64 { HistoryFormatting.sum(points.map { metric.value($0) }) }

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack {
        Text("概览").font(.title3.weight(.semibold)).accessibilityAddTraits(.isHeader).accessibilityIdentifier("overviewHeading")
        Spacer()
        Button { Task { await library.refresh() } } label: {
          Image(systemName: "arrow.clockwise").frame(width: 44, height: 44)
        }.accessibilityLabel("刷新概览").accessibilityIdentifier("refreshOverview").disabled(library.loading)
      }
      if library.error != nil {
        Label("统计暂时无法载入。", systemImage: "exclamationmark.circle").font(.footnote).foregroundStyle(.secondary)
      }
      metricsCard
      VStack(alignment: .leading, spacing: 16) {
        Picker("统计周期", selection: $period) {
          Text("近 7 天").tag(7); Text("近 30 天").tag(30)
        }.pickerStyle(.segmented).accessibilityIdentifier("overviewPeriod")
        HStack(alignment: .firstTextBaseline) {
          Text(library.hasLoaded ? (metric == .duration ? HistoryFormatting.duration(total) : total.formatted()) : "—")
            .font(.title2.weight(.semibold)).monospacedDigit().contentTransition(.opacity)
            .accessibilityIdentifier("overviewPeriodTotal")
          Spacer()
          Picker("统计指标", selection: $metric) {
            ForEach(Metric.allCases, id: \.self) { Text(LocalizedStringKey($0.rawValue)).tag($0) }
          }.pickerStyle(.menu)
        }
        Chart(points) { point in
          BarMark(x: .value("日期", point.date, unit: .day),
                  y: .value(metric.rawValue, metric == .duration ? Double(point.durationMs) / 60_000 : Double(metric.value(point))))
            .foregroundStyle(AppDesign.accent).cornerRadius(3)
            .accessibilityLabel(point.date.formatted(date: .abbreviated, time: .omitted))
            .accessibilityValue(metric == .duration ? HistoryFormatting.duration(point.durationMs) : metric.value(point).formatted())
        }.chartXAxis { AxisMarks(values: .stride(by: .day, count: period == 7 ? 2 : 7)) { _ in AxisValueLabel(format: .dateTime.month().day()) } }
          .chartYAxis { AxisMarks(position: .leading) }
          .frame(height: 112).accessibilityIdentifier("overviewActivityChart")
          .overlay {
            if library.hasLoaded && total == 0 {
              Text("开始录音后，这里会显示趋势").font(.footnote).foregroundStyle(.secondary)
            }
          }
        Text("最近的使用趋势\(metric == .duration ? "图表单位：分钟。" : "")")
          .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
      }.productCard()
    }
  }
  private var metricsCard: some View {
    let tiles: [AnyView] = [
      AnyView(tile("今日字数", value: today.characters.formatted(), detail: "今天写下的文字", id: "overviewTodayCharacters")),
      AnyView(tile("今日时长", value: HistoryFormatting.duration(today.durationMs), detail: "\(today.count) 段记录", id: "overviewTodayDuration")),
      AnyView(tile("平均段落", value: HistoryFormatting.duration(today.averageDurationMs), detail: "今日每段平均时长", id: "overviewAverage")),
      AnyView(tile("累计记录", value: library.history.count.formatted(), detail: "当前保留的历史", id: "overviewRecords")),
    ]
    return Group {
      if typeSize.isAccessibilitySize {
        VStack(spacing: 0) {
          ForEach(0..<4, id: \.self) { index in
            if index > 0 { Divider() }
            tiles[index]
          }
        }
      } else {
        VStack(spacing: 0) {
          HStack(spacing: 0) { tiles[0]; Divider(); tiles[1] }
          Divider()
          HStack(spacing: 0) { tiles[2]; Divider(); tiles[3] }
        }
      }
    }.fixedSize(horizontal: false, vertical: true)
      .background(AppDesign.surface, in: RoundedRectangle(cornerRadius: AppDesign.Radius.card, style: .continuous))
      .overlay(RoundedRectangle(cornerRadius: AppDesign.Radius.card, style: .continuous).strokeBorder(AppDesign.outline, lineWidth: 0.5))
  }
  private func tile(_ title: String, value: String, detail: String, id: String) -> some View {
    VStack(alignment: .leading, spacing: 9) {
      Text(LocalizedStringKey(title)).font(.caption).foregroundStyle(.secondary)
      Text(library.hasLoaded ? value : "—").font(.title3.weight(.semibold)).monospacedDigit()
        .fixedSize(horizontal: false, vertical: true).accessibilityIdentifier(id)
      Text(detail).font(.caption2).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }.frame(maxWidth: .infinity, alignment: .leading).padding(AppDesign.Space.card)
  }
}
