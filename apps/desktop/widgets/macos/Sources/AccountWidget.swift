import AppIntents
import SwiftUI
import WidgetKit

// PRD 0014 RF3.2: the Account widget — one account pinned by the user's
// choice (an App Intent), which never falls back to another account when it
// disappears: the account that is gone reads as unavailable. It is pinned
// by its stable id (PRD 0029), so a rename keeps it; a widget pinned by the
// legacy key before ids still resolves through the key.

/// The accounts of the snapshot, so the picker offers exactly what the app
/// published (RF1: the most restrictive first).
struct AccountEntity: AppEntity, Identifiable {
    static let typeDisplayRepresentation = TypeDisplayRepresentation(name: "Account")
    static let defaultQuery = AccountQuery()

    /// The stable account id (the key in a file written before ids).
    let id: String
    let key: String
    let name: String
    /// The provider's display name (its id in an older file).
    let provider: String

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(name)",
                              subtitle: "\(provider)")
    }
}

struct AccountQuery: EntityQuery {
    func entities(for identifiers: [String]) async throws -> [AccountEntity] {
        let accounts = AccountEntity.all
        return accounts.filter { identifiers.contains($0.id) || identifiers.contains($0.key) }
    }

    func suggestedEntities() async throws -> [AccountEntity] {
        AccountEntity.all
    }

    func defaultResult() async -> AccountEntity? {
        AccountEntity.all.first
    }
}

extension AccountEntity {
    static var all: [AccountEntity] {
        (WidgetSnapshot.load()?.accounts ?? [])
            .map { AccountEntity(id: $0.id, key: $0.key, name: $0.name,
                                 provider: $0.providerName ?? $0.provider) }
    }
}

/// The pinned account. The picker lists the accounts the file has; an
/// account that vanishes leaves the widget empty (never another account).
struct AccountIntent: WidgetConfigurationIntent {
    static var title: LocalizedStringResource = "Account"
    static var description = IntentDescription("Which account this widget shows.")

    @Parameter(title: "Account")
    var account: AccountEntity?

    @Parameter(title: "Quota")
    var metric: MetricEntity?

    @Parameter(title: "Show", default: .remaining)
    var share: ShareDisplay

    @Parameter(title: "Detail", default: .detailed)
    var detail: WidgetDetail

    @Parameter(title: "Show the reset time", default: true)
    var showReset: Bool

    @Parameter(title: "Show the age of the data", default: true)
    var showAge: Bool

    var display: WidgetDisplay {
        WidgetDisplay(share: share, detail: detail, showAge: showAge, showReset: showReset)
    }
}

struct AccountProvider: AppIntentTimelineProvider {
    func placeholder(in context: Context) -> SummaryEntry {
        SummaryEntry(date: Date(), snapshot: nil, pinnedKey: AccountEntity.all.first?.id,
                     displaySize: context.displaySize)
    }

    func snapshot(for configuration: AccountIntent, in context: Context) async -> SummaryEntry {
        entry(for: configuration, in: context)
    }

    func timeline(for configuration: AccountIntent, in context: Context) async -> Timeline<SummaryEntry> {
        let read = WidgetSnapshot.read()
        WidgetTimeline.announce(read)
        let now = Date()
        let entries = WidgetTimeline.dates(from: now).map { entry(for: configuration, in: context, read: read, date: $0) }
        return Timeline(entries: entries, policy: WidgetTimeline.policy(from: now))
    }

    private func entry(for configuration: AccountIntent, in context: Context,
                       read: SnapshotRead = WidgetSnapshot.read(), date: Date = Date()) -> SummaryEntry {
        SummaryEntry(date: date,
                     read: read,
                     pinnedKey: configuration.account?.id,
                     displaySize: context.displaySize,
                     selection: AccountSelection(metric: configuration.metric?.metricKey),
                     display: configuration.display)
    }
}

struct AccountWidgetEntryView: View {
    @Environment(\.widgetFamily) private var family
    let entry: SummaryEntry

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
            // No file: widgets are off or the app never published one
            // (RF3.5). Not the same as an account that went away.
            EmptyStateView(size: size)
        } else if let account = entry.accounts.first {
            let metric = entry.selection.metric
            let headline = account.headline(metric: metric)
            // Without a plan limit every metric is "other".
            let others = OtherMetrics.list(account, leaving: headline?.metricIndex)
            if family == .systemSmall || entry.display.detail == .compact || others.isEmpty {
                AccountRowView(account: account, style: .hero, metric: metric, width: size.width, height: size.height)
            } else {
                let spacing: CGFloat = 16
                let column = max((size.width - spacing) / 2, 0)
                let pace = headline?.pace
                VStack(alignment: .leading, spacing: 8) {
                    HStack(alignment: .top, spacing: spacing) {
                        AccountRowView(account: account, style: .hero, metric: metric, width: column,
                                       height: size.height - (pace == nil ? 0 : 22))
                        OtherMetrics(account: account, metrics: others, width: column)
                    }
                    .frame(width: size.width, alignment: .topLeading)
                    if let pace, !account.isStale {
                        PaceLine(pace: pace, width: size.width)
                    }
                }
                .frame(width: size.width, height: size.height, alignment: .topLeading)
            }
        } else {
            // The pinned account is not in the file: say it, never draw
            // someone else's numbers (RF3.2).
            AccountUnavailableView(size: size)
        }
    }
}

/// The account's other quotas, beside the headline one in the medium
/// size: each with its own share, so the widget shows what else is close.
struct OtherMetrics: View {
    @Environment(\.locale) private var locale
    @Environment(\.widgetDisplay) private var display
    let account: WidgetAccount
    let metrics: [WidgetAccount.Metric]
    let width: CGFloat

    /// Up to three metrics of the file other than the headline one
    /// (`index`); every metric without one.
    static func list(_ account: WidgetAccount, leaving index: Int?) -> [WidgetAccount.Metric] {
        Array(account.metrics.enumerated().filter { $0.offset != index }.map(\.element).prefix(3))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            ForEach(Array(metrics.enumerated()), id: \.offset) { _, metric in
                let share = QuotaShare(used: metric.usedFraction, exhausted: metric.usedFraction.safeFraction >= 1)
                VStack(alignment: .leading, spacing: 3) {
                    HStack(alignment: .firstTextBaseline, spacing: 4) {
                        Text(verbatim: account.shortLabel(metric.displayLabel))
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .layoutPriority(-1)
                        Gap.horizontal(width, min: 4)
                        Text(verbatim: share.text(locale, display.share))
                            .font(.system(.caption, design: .rounded).weight(.semibold))
                            .foregroundStyle(share.level.color)
                            .monospacedDigit()
                            .fixedSize()
                    }
                    .frame(width: width, alignment: .leading)
                    UsageBar(share: share, display: display.share, width: width, height: 4)
                }
                .accessibilityElement(children: .combine)
            }
        }
        .frame(width: width, alignment: .topLeading)
        .opacity(account.isStale ? 0.5 : 1)
    }
}

/// The pace (PRD 0007) as one line: the outlook and the points apart from
/// the uniform consumption, never colour alone.
struct PaceLine: View {
    let pace: WidgetAccount.Metric.Pace
    let width: CGFloat

    private var stage: Text {
        switch pace.stage {
        case "exhausted": return Text("Runs out before the reset")
        case "reserve": return Text("In reserve")
        case "ahead": return Text("Ahead of the average")
        default: return Text("Following the average")
        }
    }

    /// Whole points apart from the average, bounded so a wild value in the
    /// file can never trap the conversion.
    private var points: Int { Int(abs(pace.deltaPoints.safePoints).rounded()) }

    var body: some View {
        HStack(spacing: 3) {
            Image(systemName: pace.tone == "bad" ? "xmark.circle"
                  : pace.tone == "warn" ? "exclamationmark.triangle"
                  : pace.tone == "good" ? "checkmark" : "clock")
                .font(.caption2)
            (stage + (pace.deltaPoints < 0
                 ? Text(" · \(points) points in reserve")
                 : Text(" · \(points) points ahead")))
                .font(.caption2)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .frame(width: width, alignment: .leading)
        .accessibilityElement(children: .combine)
    }
}

struct AccountWidget: Widget {
    static let kind = "dev.cuotai.desktop.account"

    var body: some WidgetConfiguration {
        AppIntentConfiguration(kind: Self.kind, intent: AccountIntent.self, provider: AccountProvider()) { entry in
            AccountWidgetEntryView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        // The views apply the system's margins themselves (`WidgetCanvas`).
        .contentMarginsDisabled()
        .configurationDisplayName("Cuotai account")
        .description("One account you pin, with the quota you choose and the age of the measurement.")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}