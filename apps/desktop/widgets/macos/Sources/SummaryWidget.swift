import SwiftUI
import WidgetKit

// PRD 0014 RF3.2/RF3.5: the Summary widget — each account's binding metric,
// in the small, medium and large sizes. It reads the snapshot and nothing
// else; without a file it shows the empty state.

struct SummaryEntry: TimelineEntry {
    let date: Date
    let snapshot: WidgetSnapshot?
    /// The pinned account (its id, or a legacy key), when the user narrowed
    /// the widget down; the App Intent widget passes its own.
    let pinnedKey: String?

    var accounts: [WidgetAccount] {
        let all = snapshot?.accounts ?? []
        guard let pinnedKey else { return all }
        // A pinned account that disappeared is not replaced by another one.
        return all.filter { $0.isPinned(pinnedKey) }
    }
}

struct SummaryProvider: TimelineProvider {
    func placeholder(in context: Context) -> SummaryEntry {
        SummaryEntry(date: Date(), snapshot: nil, pinnedKey: nil)
    }

    func getSnapshot(in context: Context, completion: @escaping (SummaryEntry) -> Void) {
        completion(SummaryEntry(date: Date(), snapshot: WidgetSnapshot.load(), pinnedKey: nil))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<SummaryEntry>) -> Void) {
        let entry = SummaryEntry(date: Date(), snapshot: WidgetSnapshot.load(), pinnedKey: nil)
        // The app writes at most once every 15 seconds (RF1.4) and the
        // system decides when to redraw (RF4.2): ask again after a quarter
        // of an hour, which is also the age the snapshot itself carries.
        let next = Calendar.current.date(byAdding: .minute, value: 15, to: Date()) ?? Date()
        completion(Timeline(entries: [entry], policy: .after(next)))
    }
}

struct SummaryWidgetEntryView: View {
    @Environment(\.widgetFamily) private var family
    let entry: SummaryEntry

    var body: some View {
        if entry.accounts.isEmpty {
            EmptyStateView()
        } else {
            content
        }
    }

    @ViewBuilder
    private var content: some View {
        switch family {
        case .systemSmall:
            VStack(alignment: .leading, spacing: 2) {
                WidgetHeader(title: "Summary")
                if let account = entry.accounts.first {
                    AccountRowView(account: account, compact: true)
                }
                if entry.accounts.count > 1 {
                    Text("\(entry.accounts.count - 1) more")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        case .systemMedium:
            VStack(alignment: .leading, spacing: 4) {
                WidgetHeader(title: "Summary")
                HStack(alignment: .top, spacing: 10) {
                    ForEach(entry.accounts.prefix(2)) { account in
                        AccountRowView(account: account, compact: true)
                    }
                }
                Spacer(minLength: 0)
                TruncationNotice(truncated: entry.snapshot?.truncated ?? false,
                                 accounts: entry.accounts.count)
            }
        default:
            VStack(alignment: .leading, spacing: 6) {
                WidgetHeader(title: "Summary")
                ForEach(entry.accounts.prefix(5)) { account in
                    AccountRowView(account: account)
                }
                Spacer(minLength: 0)
                TruncationNotice(truncated: entry.snapshot?.truncated ?? false,
                                 accounts: entry.accounts.count)
            }
        }
    }
}

/// The most restrictive accounts come first; when the app published only
/// part of the configuration the widget says so instead of pretending.
struct TruncationNotice: View {
    let truncated: Bool
    let accounts: Int

    var body: some View {
        if truncated {
            Text("\(accounts) accounts shown; the rest are not published.")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }
}

struct SummaryWidget: Widget {
    static let kind = "dev.cuotai.desktop.summary"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: Self.kind, provider: SummaryProvider()) { entry in
            SummaryWidgetEntryView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
        }
        .configurationDisplayName("Cuotai summary")
        .description("Each account's binding quota, with the age of the measurement.")
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge])
    }
}