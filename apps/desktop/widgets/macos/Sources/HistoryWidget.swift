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
    /// The file exists but could not be read (`SnapshotRead.unreadable`).
    var unreadable = false
    /// The widget's size, from the timeline context (`.zero`: the family's
    /// own size, see `WidgetMetrics`).
    var displaySize: CGSize = .zero
    /// How far back the chart reaches (the widget's choice).
    var period: HistoryPeriod = .all
    /// How it is drawn (the widget's choices).
    var display = WidgetDisplay()

    init(date: Date, snapshot: WidgetSnapshot?, pinnedKey: String?, unreadable: Bool = false,
         displaySize: CGSize = .zero, period: HistoryPeriod = .all, display: WidgetDisplay = WidgetDisplay()) {
        self.date = date
        self.snapshot = snapshot
        self.pinnedKey = pinnedKey
        self.unreadable = unreadable
        self.displaySize = displaySize
        self.period = period
        self.display = display
        self.display.now = date
    }

    init(date: Date, read: SnapshotRead, pinnedKey: String?, displaySize: CGSize = .zero,
         period: HistoryPeriod = .all, display: WidgetDisplay = WidgetDisplay()) {
        self.init(date: date, snapshot: read.snapshot, pinnedKey: pinnedKey, unreadable: read.isUnreadable,
                  displaySize: displaySize, period: period, display: display)
    }

    /// The account to chart: the pinned one, else the first published
    /// (the most restrictive, RF1.2).
    var account: WidgetAccount? {
        let accounts = snapshot?.accounts ?? []
        guard let pinnedKey else { return accounts.first }
        return accounts.first { $0.isPinned(pinnedKey) }
    }

    /// The published series, within the chosen period.
    var points: [WidgetAccount.HistoryPoint] {
        let all = account?.history ?? []
        guard let span = period.span else { return all }
        let start = date.addingTimeInterval(-span)
        return all.filter { $0.takenAt >= start }
    }
}

struct HistoryProvider: AppIntentTimelineProvider {
    func placeholder(in context: Context) -> HistoryEntry {
        HistoryEntry(date: Date(), snapshot: nil, pinnedKey: nil, displaySize: context.displaySize)
    }

    func snapshot(for configuration: HistoryIntent, in context: Context) async -> HistoryEntry {
        entry(for: configuration, in: context, read: WidgetSnapshot.read(), date: Date())
    }

    func timeline(for configuration: HistoryIntent, in context: Context) async -> Timeline<HistoryEntry> {
        let read = WidgetSnapshot.read()
        WidgetTimeline.announce(read)
        let now = Date()
        let entries = WidgetTimeline.dates(from: now).map { entry(for: configuration, in: context, read: read, date: $0) }
        return Timeline(entries: entries, policy: WidgetTimeline.policy(from: now))
    }

    private func entry(for configuration: HistoryIntent, in context: Context, read: SnapshotRead,
                       date: Date) -> HistoryEntry {
        HistoryEntry(date: date,
                     read: read,
                     pinnedKey: configuration.account?.id,
                     displaySize: context.displaySize,
                     period: configuration.period,
                     display: configuration.display)
    }
}

struct HistoryWidgetEntryView: View {
    @Environment(\.locale) private var systemLocale
    let entry: HistoryEntry

    /// The language the widget lays out in (`WidgetCanvas` sets it for the
    /// views inside; the texts below are formatted here, outside them).
    private var locale: Locale { entry.snapshot?.language ?? systemLocale }

    private var display: WidgetDisplay { entry.display }

    var body: some View {
        WidgetCanvas(displaySize: entry.displaySize, language: entry.snapshot?.language) { size in
            content(size)
        }
        .environment(\.widgetDisplay, entry.display)
    }

    @ViewBuilder
    private func content(_ size: CGSize) -> some View {
        if entry.unreadable {
            SnapshotUnreadableView(size: size)
        } else if entry.snapshot == nil {
            // No file: widgets are off or the app never published one.
            EmptyStateView(size: size)
        } else if entry.pinnedKey != nil && entry.account == nil {
            // The pinned account went away: say it, never chart another
            // account and never blame the history setting (RF3.2).
            AccountUnavailableView(size: size)
        } else if let account = entry.account, !entry.points.isEmpty {
            chartView(account, size)
        } else {
            // No series: the app has no history for this account. Say it
            // instead of drawing nothing.
            VStack(alignment: .leading, spacing: 6) {
                title(entry.account, width: size.width)
                VStack(spacing: 4) {
                    Image(systemName: "chart.xyaxis.line")
                        .font(.title3)
                    Text("No history yet")
                        .font(.caption)
                    Text("Turn on the measurement history in Cuotai Settings to fill this chart.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }
                .box(size)
            }
            .frame(width: size.width, height: size.height, alignment: .topLeading)
        }
    }

    /// The first line: whose quota this is (the information, not the app).
    private func title(_ account: WidgetAccount?, width: CGFloat) -> some View {
        let name = account.map { Text(verbatim: $0.name).bold() } ?? Text("History")
        let label = account?.headline().map(\.label).flatMap { $0.isEmpty ? nil : $0 }
        return (label.map { name + Text(verbatim: " · ") + Text(verbatim: $0).foregroundColor(.secondary) } ?? name)
            .font(.caption)
            .lineLimit(1)
            .frame(width: WidgetMetrics.finite(width), alignment: .leading)
    }

    private func chartView(_ account: WidgetAccount, _ size: CGSize) -> some View {
        let points = entry.points
        let headline = account.headline()
        let share = headline?.share
            ?? QuotaShare(used: points.last?.usedFraction ?? 0, exhausted: false)
        let reset = display.showReset ? ResetText.text(headline?.resetsAt, now: entry.date, locale: locale) : nil
        let detailed = display.detail == .detailed
        return VStack(alignment: .leading, spacing: 4) {
            title(account, width: size.width)
            // The value, its unit word and the reset: plain texts on their
            // own line, inside the margins.
            HStack(alignment: .firstTextBaseline, spacing: 4) {
                Text(verbatim: share.text(locale, display.share))
                    .font(.system(.title2, design: .rounded).weight(.semibold))
                    .foregroundStyle(share.level.color)
                    .monospacedDigit()
                    .fixedSize()
                (display.share == .remaining ? Text("left") : Text("used"))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .fixedSize()
                if let reset {
                    (Text(verbatim: "· ") + reset)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .layoutPriority(-1)
                }
                Gap.horizontal(size.width)
            }
            .frame(width: size.width, alignment: .leading)
            chart(points: points, current: share, compactAxis: !detailed)
                // Without the axis labels the plot reaches both edges: room
                // for the end point's mark inside the margins.
                .padding(.horizontal, detailed ? 0 : 4)
                .box(size)
                .opacity(account.isStale ? 0.5 : 1)
            // The span on the left, the age on the right: two frames of
            // fixed widths, so the live age is never sized by its content.
            if detailed || display.showAge || account.isStale {
                ZStack(alignment: .topLeading) {
                    if detailed {
                        Text("Last \(spanLabel(points))")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .frame(width: size.width * 0.4, alignment: .leading)
                    }
                    if display.showAge || account.isStale {
                        AgeLine(checkedAt: account.checkedAt, freshness: account.freshness, stale: account.isStale,
                                width: size.width, alignment: .trailing)
                    }
                }
                .frame(width: size.width, height: 13, alignment: .topLeading)
                .frame(width: size.width, alignment: .leading)
            }
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
    }

    /// The span the series covers, in the system's own abbreviated units
    /// ("23 h", "2 d", or the language's own), whole hours below a day and
    /// whole days above.
    private func spanLabel(_ points: [WidgetAccount.HistoryPoint]) -> String {
        let hours = points.first.map { Int(entry.date.timeIntervalSince($0.takenAt) / 3600) } ?? 24
        let formatter = DateComponentsFormatter()
        var calendar = Calendar.current
        calendar.locale = locale
        formatter.calendar = calendar
        formatter.unitsStyle = .abbreviated
        formatter.maximumUnitCount = 1
        var span = DateComponents()
        if hours >= 24 {
            formatter.allowedUnits = [.day]
            span.day = hours / 24
        } else {
            formatter.allowedUnits = [.hour]
            span.hour = max(hours, 1)
        }
        return formatter.string(from: span) ?? ""
    }

    /// The share the chart plots for one sample, in the widget's choice.
    private func value(_ used: Double) -> Double {
        display.share == .remaining ? max(0, 1 - min(used.safeFraction, 1)) : used.safeFraction
    }

    private func chart(points: [WidgetAccount.HistoryPoint], current: QuotaShare, compactAxis: Bool) -> some View {
        // Overuse stays visible when the chart shows what was used: the
        // scale grows past 100% when a sample does, up to the file's own
        // ceiling. What is left never goes below 0 or above 100%.
        let top = max(1, points.map { value($0.usedFraction) }.max() ?? 1)
        let marks: [Double] = top <= 1 ? [0, 1] : [0, 1, top]
        let last = points.last
        return Chart {
            ForEach(points, id: \.takenAt) { point in
                AreaMark(x: .value("When", point.takenAt),
                         y: .value("Share", value(point.usedFraction)))
                    .foregroundStyle(.linearGradient(colors: [Color.accentColor.opacity(0.35), Color.accentColor.opacity(0.05)],
                                                     startPoint: .top, endPoint: .bottom))
                    .interpolationMethod(.monotone)
                LineMark(x: .value("When", point.takenAt),
                         y: .value("Share", value(point.usedFraction)))
                    .foregroundStyle(.tint)
                    .interpolationMethod(.monotone)
            }
            if let last {
                // Where the series ends: the latest reading, in its level.
                PointMark(x: .value("When", last.takenAt), y: .value("Share", value(last.usedFraction)))
                    .foregroundStyle(current.level.color)
                    .symbolSize(28)
            }
        }
        .chartYScale(domain: 0...top)
        .chartYAxis {
            AxisMarks(position: .trailing, values: marks) { value in
                AxisGridLine(stroke: StrokeStyle(lineWidth: 0.5, dash: [2, 3]))
                if !compactAxis {
                    AxisValueLabel {
                        if let ratio = value.as(Double.self) {
                            Text(ratio, format: .percent.precision(.fractionLength(0)))
                                .font(.system(size: 8))
                        }
                    }
                }
            }
        }
        .chartXAxis(.hidden)
        .accessibilityLabel(Text("Consumption history"))
        .accessibilityValue(Text("\(points.count) samples, now \(current.text(locale, display.share))"))
    }
}

struct HistoryWidget: Widget {
    static let kind = "dev.cuotai.desktop.history"

    var body: some WidgetConfiguration {
        AppIntentConfiguration(kind: Self.kind, intent: HistoryIntent.self, provider: HistoryProvider()) { entry in
            HistoryWidgetEntryView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        // The views apply the system's margins themselves (`WidgetCanvas`).
        .contentMarginsDisabled()
        .configurationDisplayName("Cuotai history")
        .description("The consumption of one account over the last hours or days.")
        .supportedFamilies([.systemMedium])
    }
}
