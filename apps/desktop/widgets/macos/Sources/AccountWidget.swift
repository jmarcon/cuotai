import AppIntents
import SwiftUI
import WidgetKit

// PRD 0014 RF3.2: the Account widget — one account pinned by the user's
// choice (an App Intent), which never falls back to another account when it
// disappears: the key that is gone reads as unavailable.

/// The accounts of the snapshot, so the picker offers exactly what the app
/// published (RF1: the most restrictive first).
struct AccountEntity: AppEntity, Identifiable {
    static let typeDisplayRepresentation = TypeDisplayRepresentation(name: "Account")
    static let defaultQuery = AccountQuery()

    var id: String { key }
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
        return accounts.filter { identifiers.contains($0.key) }
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
            .map { AccountEntity(key: $0.key, name: $0.name, provider: $0.provider) }
    }
}

/// The pinned account's key. The picker lists the accounts the file has;
/// a key that vanishes leaves the widget empty (never another account).
struct AccountIntent: WidgetConfigurationIntent {
    static var title: LocalizedStringResource = "Account"
    static var description = IntentDescription("Which account this widget shows.")

    @Parameter(title: "Account")
    var account: AccountEntity?
}

struct AccountProvider: AppIntentTimelineProvider {
    func placeholder(in context: Context) -> SummaryEntry {
        SummaryEntry(date: Date(), snapshot: nil, pinnedKey: AccountEntity.all.first?.key)
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
                     pinnedKey: configuration.account?.key)
    }
}

struct AccountWidgetEntryView: View {
    @Environment(\.widgetFamily) private var family
    let entry: SummaryEntry

    var body: some View {
        if let account = entry.accounts.first {
            VStack(alignment: .leading, spacing: 3) {
                WidgetHeader(title: account.name)
                AccountRowView(account: account, compact: family != .systemLarge)
                Spacer(minLength: 0)
                if let pace = account.metrics.first?.pace {
                    PaceLine(pace: pace)
                }
            }
        } else {
            // The pinned account is not in the file: say it, never draw
            // someone else's numbers (RF3.2).
            VStack(spacing: 4) {
                Image(systemName: "exclamationmark.triangle")
                    .font(.title3)
                Text("Account unavailable")
                    .font(.caption)
                Text("It was removed or is not published. No other account is shown here.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
            .accessibilityElement(children: .combine)
        }
    }
}

/// The pace (PRD 0007) as one line: the outlook and the points apart from
/// the uniform consumption, never colour alone.
struct PaceLine: View {
    let pace: WidgetAccount.Metric.Pace

    private var stage: String {
        switch pace.stage {
        case "exhausted": return "Runs out before the reset"
        case "reserve": return "In reserve"
        case "ahead": return "Ahead of the average"
        default: return "Following the average"
        }
    }

    var body: some View {
        HStack(spacing: 3) {
            Image(systemName: pace.tone == "bad" ? "xcircle"
                  : pace.tone == "warn" ? "exclamationmark.triangle"
                  : pace.tone == "good" ? "checkmark" : "clock")
                .font(.caption2)
            Text("\(stage) · \(abs(Int(pace.deltaPoints.rounded()))) points \(pace.deltaPoints < 0 ? "in reserve" : "ahead")")
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