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
            .map { AccountEntity(id: $0.id, key: $0.key, name: $0.name, provider: $0.provider) }
    }
}

/// The pinned account. The picker lists the accounts the file has; an
/// account that vanishes leaves the widget empty (never another account).
struct AccountIntent: WidgetConfigurationIntent {
    static var title: LocalizedStringResource = "Account"
    static var description = IntentDescription("Which account this widget shows.")

    @Parameter(title: "Account")
    var account: AccountEntity?
}

struct AccountProvider: AppIntentTimelineProvider {
    func placeholder(in context: Context) -> SummaryEntry {
        SummaryEntry(date: Date(), snapshot: nil, pinnedKey: AccountEntity.all.first?.id)
    }

    func snapshot(for configuration: AccountIntent, in context: Context) async -> SummaryEntry {
        entry(for: configuration)
    }

    func timeline(for configuration: AccountIntent, in context: Context) async -> Timeline<SummaryEntry> {
        let entry = self.entry(for: configuration)
        let next = Calendar.current.date(byAdding: .minute, value: 15, to: Date()) ?? Date()
        return Timeline(entries: [entry], policy: .after(next))
    }

    private func entry(for configuration: AccountIntent) -> SummaryEntry {
        SummaryEntry(date: Date(),
                     snapshot: WidgetSnapshot.load(),
                     pinnedKey: configuration.account?.id)
    }
}

struct AccountWidgetEntryView: View {
    @Environment(\.widgetFamily) private var family
    let entry: SummaryEntry

    var body: some View {
        if entry.snapshot == nil {
            // No file: widgets are off or the app never published one
            // (RF3.5). Not the same as an account that went away.
            EmptyStateView()
        } else if let account = entry.accounts.first {
            if family == .systemSmall {
                VStack(alignment: .leading, spacing: 6) {
                    WidgetHeader(title: Text("Account"))
                    AccountRowView(account: account, style: .hero)
                }
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    WidgetHeader(title: Text("Account"))
                    HStack(alignment: .top, spacing: 16) {
                        AccountRowView(account: account, style: .hero)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        // Without a plan limit every metric is "other".
                        if account.metrics.count > (account.hasPlanLimit ? 1 : 0) {
                            OtherMetrics(account: account)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    if account.hasPlanLimit, let pace = account.metrics.first?.pace {
                        PaceLine(pace: pace)
                    }
                }
            }
        } else {
            // The pinned account is not in the file: say it, never draw
            // someone else's numbers (RF3.2).
            AccountUnavailableView()
        }
    }
}

/// The account's other quotas, beside the binding one in the medium size:
/// each with its own share, so the widget shows what else is close.
struct OtherMetrics: View {
    let account: WidgetAccount

    /// `binding.index` points into the file's `metrics` (the binding one
    /// goes first); without a binding every metric is listed.
    private var others: [WidgetAccount.Metric] {
        let binding = account.binding?.index
        return account.metrics.enumerated()
            .filter { $0.offset != binding }
            .map(\.element)
            .prefix(3)
            .map { $0 }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            ForEach(Array(others.enumerated()), id: \.offset) { _, metric in
                VStack(alignment: .leading, spacing: 3) {
                    HStack(alignment: .firstTextBaseline, spacing: 4) {
                        Text(verbatim: metric.shortLabel)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .layoutPriority(-1)
                        Spacer(minLength: 4)
                        Text("\(Int((metric.usedFraction.safeFraction * 100).rounded()))%")
                            .font(.system(.caption, design: .rounded).weight(.semibold))
                            .monospacedDigit()
                    }
                    UsageBar(usedFraction: metric.usedFraction, stale: account.isStale, height: 4)
                }
                .accessibilityElement(children: .combine)
            }
        }
        .opacity(account.isStale ? 0.6 : 1)
    }
}

/// The pace (PRD 0007) as one line: the outlook and the points apart from
/// the uniform consumption, never colour alone.
struct PaceLine: View {
    let pace: WidgetAccount.Metric.Pace

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
        .configurationDisplayName("Cuotai account")
        .description("One account you pin, with its binding quota and the age of the measurement.")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}