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
            VStack(alignment: .leading, spacing: 6) {
                WidgetHeader(title: Text("Summary"))
                if let account = entry.accounts.first {
                    AccountRowView(account: account, style: .hero)
                }
                SummaryFooter(shown: 1, accounts: entry.accounts.count,
                              truncated: entry.snapshot?.truncated ?? false)
            }
        case .systemMedium:
            // Two columns, two rows: the four most restrictive accounts.
            list(columns: 2, limit: 4)
        default:
            list(columns: 1, limit: 6)
        }
    }

    private func list(columns: Int, limit: Int) -> some View {
        let shown = Array(entry.accounts.prefix(limit))
        // Two columns leave no room for an age per row: the header carries
        // the oldest current one, so every fresh row is at most that old.
        let ageInHeader = columns > 1
        let oldest = shown.filter { !$0.isStale && $0.freshness != "aging" }.compactMap(\.checkedAt).min()
        return VStack(alignment: .leading, spacing: 8) {
            WidgetHeader(title: Text("Summary"), checkedAt: ageInHeader ? oldest : nil)
            Grid(alignment: .topLeading, horizontalSpacing: 14, verticalSpacing: columns == 1 ? 9 : 8) {
                ForEach(Array(stride(from: 0, to: shown.count, by: columns)), id: \.self) { start in
                    GridRow {
                        ForEach(start..<min(start + columns, shown.count), id: \.self) { index in
                            AccountRowView(account: shown[index], ageInHeader: ageInHeader)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        // An odd last row keeps its column the same width.
                        if start + columns > shown.count {
                            Color.clear.gridCellUnsizedAxes([.horizontal, .vertical])
                        }
                    }
                }
            }
            Spacer(minLength: 0)
            SummaryFooter(shown: shown.count, accounts: entry.accounts.count,
                          truncated: entry.snapshot?.truncated ?? false)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

/// The last line of the Summary: how many accounts did not fit, and, when
/// the app published only part of the configuration, that the rest exists
/// (the widget never shows a short list as if it were the whole one).
struct SummaryFooter: View {
    let shown: Int
    let accounts: Int
    let truncated: Bool

    var body: some View {
        if accounts > shown {
            Text("\(accounts - shown) more")
                .font(.caption2)
                .foregroundStyle(.secondary)
        } else if truncated {
            TruncationNotice(truncated: truncated, accounts: accounts)
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
                .lineLimit(1)
                .minimumScaleFactor(0.8)
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