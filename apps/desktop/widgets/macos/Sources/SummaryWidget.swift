import AppIntents
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
    /// Which accounts and quota, in which order (the widget's choices).
    var selection = AccountSelection()
    /// How they are drawn (the widget's choices).
    var display = WidgetDisplay()

    /// An entry for what `WidgetSnapshot.read()` found.
    init(date: Date, snapshot: WidgetSnapshot?, pinnedKey: String?, unreadable: Bool = false,
         displaySize: CGSize = .zero, selection: AccountSelection = AccountSelection(),
         display: WidgetDisplay = WidgetDisplay()) {
        self.date = date
        self.snapshot = snapshot
        self.pinnedKey = pinnedKey
        self.unreadable = unreadable
        self.displaySize = displaySize
        self.selection = selection
        self.display = display
        self.display.now = date
    }

    init(date: Date, read: SnapshotRead, pinnedKey: String?, displaySize: CGSize = .zero,
         selection: AccountSelection = AccountSelection(), display: WidgetDisplay = WidgetDisplay()) {
        self.init(date: date, snapshot: read.snapshot, pinnedKey: pinnedKey, unreadable: read.isUnreadable,
                  displaySize: displaySize, selection: selection, display: display)
    }

    var accounts: [WidgetAccount] {
        guard let snapshot else { return [] }
        if let pinnedKey {
            // A pinned account that disappeared is not replaced by another one.
            return snapshot.accounts.filter { $0.isPinned(pinnedKey) }
        }
        return snapshot.accounts(selection)
    }

    /// The user chose accounts and none of them is in the file any more.
    var selectionGone: Bool {
        guard let snapshot, !selection.accounts.isEmpty else { return false }
        return !snapshot.accounts.contains { account in selection.accounts.contains { account.isPinned($0) } }
    }
}

struct SummaryProvider: AppIntentTimelineProvider {
    func placeholder(in context: Context) -> SummaryEntry {
        SummaryEntry(date: Date(), snapshot: nil, pinnedKey: nil, displaySize: context.displaySize)
    }

    func snapshot(for configuration: SummaryIntent, in context: Context) async -> SummaryEntry {
        SummaryEntry(date: Date(), read: WidgetSnapshot.read(), pinnedKey: nil, displaySize: context.displaySize,
                     selection: configuration.selection, display: configuration.display)
    }

    func timeline(for configuration: SummaryIntent, in context: Context) async -> Timeline<SummaryEntry> {
        let read = WidgetSnapshot.read()
        WidgetTimeline.announce(read)
        let now = Date()
        // The app writes at most once every 15 seconds (RF1.4) and the
        // system decides when to redraw (RF4.2).
        let entries = WidgetTimeline.dates(from: now).map {
            SummaryEntry(date: $0, read: read, pinnedKey: nil, displaySize: context.displaySize,
                         selection: configuration.selection, display: configuration.display)
        }
        return Timeline(entries: entries, policy: WidgetTimeline.policy(from: now))
    }
}

struct SummaryWidgetEntryView: View {
    @Environment(\.widgetFamily) private var family
    let entry: SummaryEntry

    var body: some View {
        WidgetCanvas(displaySize: entry.displaySize, language: entry.snapshot?.language) { size in
            if entry.unreadable {
                SnapshotUnreadableView(size: size)
            } else if entry.selectionGone {
                AccountUnavailableView(size: size)
            } else if entry.accounts.isEmpty {
                EmptyStateView(size: size)
            } else {
                content(size)
            }
        }
        .environment(\.widgetDisplay, entry.display)
    }

    private var compact: Bool { entry.display.detail == .compact }

    @ViewBuilder
    private func content(_ size: CGSize) -> some View {
        let accounts = entry.accounts
        if accounts.count == 1, let account = accounts.first, family == .systemSmall {
            // One account: the share is the headline.
            AccountRowView(account: account, style: .hero, metric: entry.selection.metric,
                           width: size.width, height: size.height)
        } else {
            switch family {
            case .systemSmall:
                list(size, columns: 1, style: .compact)
            case .systemMedium:
                // Detailed: two columns of rows with the quota's name;
                // compact: one line per account, as many as fit.
                compact ? list(size, columns: 2, style: .compact) : list(size, columns: 2, style: .row)
            default:
                compact ? list(size, columns: 2, style: .compact) : list(size, columns: 1, style: .row)
            }
        }
    }

    /// The accounts in `columns`, as many rows as the height leaves above
    /// the footer (each style has a fixed height), never one clipped.
    private func list(_ size: CGSize, columns: Int, style: AccountStyle) -> some View {
        let accounts = entry.accounts
        let truncated = entry.snapshot?.truncated ?? false
        let spacing: CGFloat = style == .row ? 8 : 6
        let footer = ListFooter.height(truncated: truncated)
        let available = size.height - footer - spacing
        // Fill row by row while the next row still fits.
        var rows: [[WidgetAccount]] = []
        var used: CGFloat = 0
        var index = 0
        while index < accounts.count {
            let row = Array(accounts[index..<min(index + columns, accounts.count)])
            let height = row.map { AccountStyle.height(style, stale: $0.isStale) }.max() ?? 0
            let needed = (rows.isEmpty ? 0 : spacing) + height
            if used + needed > available { break }
            rows.append(row)
            used += needed
            index += row.count
        }
        let shown = rows.flatMap { $0 }
        let column = max((size.width - 14 * CGFloat(columns - 1)) / CGFloat(columns), 0)
        // One age for the widget: the oldest current reading shown (an
        // old one says its own on its row).
        let oldest = shown.filter { !$0.isStale && $0.freshness != "aging" }.compactMap(\.checkedAt).min()
        return VStack(alignment: .leading, spacing: spacing) {
            ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                HStack(alignment: .top, spacing: 14) {
                    ForEach(row, id: \.id) { account in
                        AccountRowView(account: account, style: style, metric: entry.selection.metric, width: column)
                    }
                }
                .frame(width: size.width, alignment: .leading)
            }
            Gap.vertical(size.height)
            ListFooter(checkedAt: entry.display.showAge ? oldest : nil,
                       hidden: accounts.count - shown.count, truncated: truncated,
                       accounts: accounts.count, width: size.width)
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
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
        AppIntentConfiguration(kind: Self.kind, intent: SummaryIntent.self, provider: SummaryProvider()) { entry in
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