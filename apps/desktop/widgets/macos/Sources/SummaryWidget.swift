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
    /// The file exists but could not be read (`SnapshotRead.unreadable`).
    var unreadable = false
    /// The widget's size, from the timeline context (`.zero`: the family's
    /// own size, see `WidgetMetrics`).
    var displaySize: CGSize = .zero

    /// An entry for what `WidgetSnapshot.read()` found.
    init(date: Date, snapshot: WidgetSnapshot?, pinnedKey: String?, unreadable: Bool = false,
         displaySize: CGSize = .zero) {
        self.date = date
        self.snapshot = snapshot
        self.pinnedKey = pinnedKey
        self.unreadable = unreadable
        self.displaySize = displaySize
    }

    init(date: Date, read: SnapshotRead, pinnedKey: String?, displaySize: CGSize = .zero) {
        self.init(date: date, snapshot: read.snapshot, pinnedKey: pinnedKey, unreadable: read.isUnreadable,
                  displaySize: displaySize)
    }

    var accounts: [WidgetAccount] {
        let all = snapshot?.accounts ?? []
        guard let pinnedKey else { return all }
        // A pinned account that disappeared is not replaced by another one.
        return all.filter { $0.isPinned(pinnedKey) }
    }
}

struct SummaryProvider: TimelineProvider {
    func placeholder(in context: Context) -> SummaryEntry {
        SummaryEntry(date: Date(), snapshot: nil, pinnedKey: nil, displaySize: context.displaySize)
    }

    func getSnapshot(in context: Context, completion: @escaping (SummaryEntry) -> Void) {
        completion(SummaryEntry(date: Date(), read: WidgetSnapshot.read(), pinnedKey: nil,
                                displaySize: context.displaySize))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<SummaryEntry>) -> Void) {
        let entry = SummaryEntry(date: Date(), read: WidgetSnapshot.read(), pinnedKey: nil,
                                 displaySize: context.displaySize)
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
        WidgetCanvas(displaySize: entry.displaySize) { size in
            if entry.unreadable {
                SnapshotUnreadableView(size: size)
            } else if entry.accounts.isEmpty {
                EmptyStateView(size: size)
            } else {
                content(size)
            }
        }
    }

    @ViewBuilder
    private func content(_ size: CGSize) -> some View {
        switch family {
        case .systemSmall:
            VStack(alignment: .leading, spacing: 6) {
                WidgetHeader(title: Text("Summary"), width: size.width)
                if let account = entry.accounts.first {
                    AccountRowView(account: account, style: .hero, width: size.width, height: size.height)
                }
                SummaryFooter(shown: 1, accounts: entry.accounts.count,
                              truncated: entry.snapshot?.truncated ?? false)
                    .frame(width: size.width, alignment: .leading)
            }
        case .systemMedium:
            // Two columns, two rows: the four most restrictive accounts.
            list(size, columns: 2, limit: 4)
        default:
            list(size, columns: 1, limit: 6)
        }
    }

    private func list(_ size: CGSize, columns: Int, limit: Int) -> some View {
        let shown = Array(entry.accounts.prefix(limit))
        // Two columns leave no room for an age per row: the header carries
        // the oldest current one, so every fresh row is at most that old.
        let ageInHeader = columns > 1
        let oldest = shown.filter { !$0.isStale && $0.freshness != "aging" }.compactMap(\.checkedAt).min()
        let spacing: CGFloat = 14
        let column = max((size.width - spacing * CGFloat(columns - 1)) / CGFloat(columns), 0)
        return VStack(alignment: .leading, spacing: 8) {
            WidgetHeader(title: Text("Summary"), width: size.width, checkedAt: ageInHeader ? oldest : nil)
            VStack(alignment: .leading, spacing: columns == 1 ? 9 : 8) {
                ForEach(Array(stride(from: 0, to: shown.count, by: columns)), id: \.self) { start in
                    HStack(alignment: .top, spacing: spacing) {
                        ForEach(start..<min(start + columns, shown.count), id: \.self) { index in
                            AccountRowView(account: shown[index], ageInHeader: ageInHeader, width: column)
                        }
                    }
                    .frame(width: size.width, alignment: .leading)
                }
            }
            Gap.vertical(size.height)
            SummaryFooter(shown: shown.count, accounts: entry.accounts.count,
                          truncated: entry.snapshot?.truncated ?? false)
                .frame(width: size.width, alignment: .leading)
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
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
        // The views apply the system's margins themselves (`WidgetCanvas`).
        .contentMarginsDisabled()
        .configurationDisplayName("Cuotai summary")
        .description("Each account's binding quota, with the age of the measurement.")
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge])
    }
}