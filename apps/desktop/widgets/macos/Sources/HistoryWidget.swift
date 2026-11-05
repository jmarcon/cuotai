import Charts
import SwiftUI
import WidgetKit

// PRD 0014 RF3.2: the History widget (medium) — an area chart of the binding
// metric's recent series, only when the file carries one (the app publishes
// it while this device records history, PRD 0006). Without a series the
// widget says so; it never draws a made-up curve.

struct HistoryEntry: TimelineEntry {
    let date: Date
    let snapshot: WidgetSnapshot?
    let pinnedKey: String?

    /// The account to chart: the pinned one, else the first published
    /// (the most restrictive, RF1.2).
    var account: WidgetAccount? {
        let accounts = snapshot?.accounts ?? []
        guard let pinnedKey else { return accounts.first }
        return accounts.first { $0.isPinned(pinnedKey) }
    }

    var points: [WidgetAccount.HistoryPoint] {
        account?.history ?? []
    }
}

struct HistoryProvider: AppIntentTimelineProvider {
    func placeholder(in context: Context) -> HistoryEntry {
        HistoryEntry(date: Date(), snapshot: nil, pinnedKey: nil)
    }

    func snapshot(for configuration: AccountIntent, in context: Context) async -> HistoryEntry {
        entry(for: configuration)
    }

    func timeline(for configuration: AccountIntent, in context: Context) async -> Timeline<HistoryEntry> {
        let next = Calendar.current.date(byAdding: .minute, value: 15, to: Date()) ?? Date()
        return Timeline(entries: [entry(for: configuration)], policy: .after(next))
    }

    private func entry(for configuration: AccountIntent) -> HistoryEntry {
        HistoryEntry(date: Date(),
                     snapshot: WidgetSnapshot.load(),
                     pinnedKey: configuration.account?.id)
    }
}

struct HistoryWidgetEntryView: View {
    let entry: HistoryEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            WidgetHeader(title: entry.account.map { Text(verbatim: $0.name) } ?? Text("History"))
            if let account = entry.account, !entry.points.isEmpty {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text("\(Int(((account.binding?.usedRatio ?? entry.points.last?.usedFraction ?? 0) * 100).rounded()))%")
                        .font(.system(.title2, design: .rounded).weight(.semibold))
                        .monospacedDigit()
                    Text("\(account.binding?.shortLabel ?? account.name) · last \(spanLabel)")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .layoutPriority(-1)
                    Spacer(minLength: 4)
                    MeasurementAge(checkedAt: account.checkedAt,
                                   freshness: account.freshness,
                                   stale: account.isStale)
                }
                chart(points: entry.points)
                    .opacity(account.isStale ? 0.6 : 1)
            } else {
                // No series: the app has no history for this account (or the
                // file is absent). Say it instead of drawing nothing.
                VStack(spacing: 4) {
                    Image(systemName: "chart.xyaxis.line")
                        .font(.title3)
                    Group {
                        if entry.snapshot == nil { Text("Open Cuotai to get started.") } else { Text("No history yet") }
                    }
                    .font(.caption)
                    if entry.snapshot != nil {
                        Text("Turn on the measurement history in Cuotai Settings to fill this chart.")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }

    /// The span the series covers, as the file published it.
    private var spanLabel: String {
        guard let first = entry.points.first?.takenAt else { return "24 h" }
        let hours = Int(entry.date.timeIntervalSince(first) / 3600)
        if hours >= 24 { return "\(hours / 24) d" }
        return "\(max(hours, 1)) h"
    }

    private func chart(points: [WidgetAccount.HistoryPoint]) -> some View {
        Chart(points, id: \.takenAt) { point in
            AreaMark(x: .value("When", point.takenAt),
                     y: .value("Used", point.usedFraction))
                .foregroundStyle(.linearGradient(colors: [Color.accentColor.opacity(0.35), Color.accentColor.opacity(0.05)],
                                                 startPoint: .top, endPoint: .bottom))
                .interpolationMethod(.monotone)
            LineMark(x: .value("When", point.takenAt),
                    y: .value("Used", point.usedFraction))
                .foregroundStyle(.tint)
                .interpolationMethod(.monotone)
        }
        .chartYScale(domain: 0...1)
        .chartYAxis {
            AxisMarks(position: .trailing, values: [0, 0.5, 1]) { value in
                AxisGridLine(stroke: StrokeStyle(lineWidth: 0.5, dash: [2, 3]))
                AxisValueLabel {
                    if let ratio = value.as(Double.self) {
                        Text(ratio, format: .percent.precision(.fractionLength(0)))
                    }
                }
            }
        }
        .chartXAxis(.hidden)
        .accessibilityLabel(Text("Consumption history"))
        .accessibilityValue(Text("\(points.count) samples, now \(Int((points.last?.usedFraction ?? 0) * 100)) percent"))
    }
}

struct HistoryWidget: Widget {
    static let kind = "dev.cuotai.desktop.history"

    var body: some WidgetConfiguration {
        AppIntentConfiguration(kind: Self.kind, intent: AccountIntent.self, provider: HistoryProvider()) { entry in
            HistoryWidgetEntryView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Cuotai history")
        .description("The consumption of one account over the last hours or days.")
        .supportedFamilies([.systemMedium])
    }
}