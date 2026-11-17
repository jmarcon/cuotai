import SwiftUI
import WidgetKit

// PRD 0014 RF3.3/RF3.4/RF3.5: how an account is drawn — the measurement age
// in the system's own relative text (which the system advances by itself),
// dimmed bars for preserved readings, no animation, and the empty state when
// the file is absent. Colours never carry meaning alone: the tone has a label.

// MARK: - Size

/// The widget's size, from WidgetKit, and everything inside laid out from it.
///
/// macOS 27.2 lays widget content out under proposals that are not the
/// widget's size, an unbounded width among them. A view that grows to what
/// it is offered (a bar's track, a chart, a flexible frame, a spacer) then
/// takes an infinite width and SwiftUI stops the extension ("view origin is
/// invalid"), and the system keeps its grey placeholder; or the content is
/// measured at its natural size and drawn as a thin column. So no widget
/// view depends on the proposal: the root has the size WidgetKit gives the
/// timeline (`TimelineProviderContext.displaySize`), the content margins are
/// applied here, and every bar, column and chart inside gets a width
/// computed from that size. `render.sh --check` refuses views that grow to
/// the proposal and compares the drawing under every proposal.
enum WidgetMetrics {
    /// The point sizes macOS 26 and 27 give the desktop families (logged by
    /// `chronod` for a live widget); used when the context has no size.
    static func size(for family: WidgetFamily) -> CGSize {
        switch family {
        case .systemMedium: return CGSize(width: 344, height: 164)
        case .systemLarge: return CGSize(width: 344, height: 344)
        default: return CGSize(width: 164, height: 164)
        }
    }

    /// The content margin when the system reports none.
    static let margin: CGFloat = 16

    /// The context's size when it is a real one, else the family's.
    static func resolve(_ size: CGSize, family: WidgetFamily) -> CGSize {
        let valid = size.width.isFinite && size.height.isFinite && size.width >= 60 && size.height >= 60
            && size.width <= 2000 && size.height <= 2000
        return valid ? size : self.size(for: family)
    }

    /// The system's content margins (the widgets turn the automatic ones
    /// off and apply these themselves), each finite and at most a quarter
    /// of the widget; all zero, as outside an extension, means the default.
    static func margins(_ insets: EdgeInsets, in size: CGSize) -> EdgeInsets {
        let limit = min(size.width, size.height) / 4
        func edge(_ value: CGFloat) -> CGFloat {
            value.isFinite && value >= 0 ? min(value, limit) : margin
        }
        let edges = EdgeInsets(top: edge(insets.top), leading: edge(insets.leading),
                               bottom: edge(insets.bottom), trailing: edge(insets.trailing))
        if edges.top + edges.leading + edges.bottom + edges.trailing == 0 {
            return EdgeInsets(top: margin, leading: margin, bottom: margin, trailing: margin)
        }
        return edges
    }

    /// A length a frame accepts: finite and not negative, else zero.
    static func finite(_ length: CGFloat) -> CGFloat {
        length.isFinite && length >= 0 ? length : 0
    }
}

/// The root of every widget: exactly the widget's size whatever the system
/// proposes, with the content margins inside, and the content laid out in
/// the inner size it is handed, in the app's language when the file names
/// it (PRD 0034: the strings, numbers and dates follow it), else the
/// system's.
struct WidgetCanvas<Content: View>: View {
    @Environment(\.widgetFamily) private var family
    @Environment(\.widgetContentMargins) private var systemMargins
    @Environment(\.locale) private var systemLocale
    /// `TimelineProviderContext.displaySize`, carried by the entry.
    let displaySize: CGSize
    /// The app's language, from the file (`WidgetSnapshot.language`).
    var language: Locale? = nil
    @ViewBuilder let content: (CGSize) -> Content

    var body: some View {
        let size = WidgetMetrics.resolve(displaySize, family: family)
        let margins = WidgetMetrics.margins(systemMargins, in: size)
        let inner = CGSize(width: max(size.width - margins.leading - margins.trailing, 0),
                           height: max(size.height - margins.top - margins.bottom, 0))
        content(inner)
            .frame(width: inner.width, height: inner.height, alignment: .topLeading)
            .padding(margins)
            .frame(width: size.width, height: size.height)
            .environment(\.locale, language ?? systemLocale)
    }
}

/// Room that grows with what is left, up to a finite `limit`: what a
/// spacer does, without a spacer's unbounded size. The lowest layout
/// priority, so the texts beside it are sized first.
struct Gap: View {
    enum Axis { case horizontal, vertical }

    let axis: Axis
    let limit: CGFloat
    var minLength: CGFloat = 0

    static func horizontal(_ limit: CGFloat, min: CGFloat = 0) -> Gap {
        Gap(axis: .horizontal, limit: limit, minLength: min)
    }

    static func vertical(_ limit: CGFloat, min: CGFloat = 0) -> Gap {
        Gap(axis: .vertical, limit: limit, minLength: min)
    }

    var body: some View {
        let low = WidgetMetrics.finite(minLength)
        let high = max(WidgetMetrics.finite(limit), low)
        Group {
            switch axis {
            case .horizontal:
                Color.clear.frame(minWidth: low, maxWidth: high, minHeight: 0, maxHeight: 0)
            case .vertical:
                Color.clear.frame(minWidth: 0, maxWidth: 0, minHeight: low, maxHeight: high)
            }
        }
        .layoutPriority(-100)
        .accessibilityHidden(true)
    }
}

extension View {
    /// Exactly `size.width` wide, and as tall as the parent leaves up to
    /// `size.height`, centred: the frame of a state or a chart below a
    /// header, finite under any proposal.
    func box(_ size: CGSize) -> some View {
        frame(width: WidgetMetrics.finite(size.width))
            .frame(minHeight: 0, maxHeight: WidgetMetrics.finite(size.height))
    }
}

// MARK: - Empty state

/// RF3.5: "Open Cuotai to get started" when there is no file. Never sample
/// data disguised as real.
struct EmptyStateView: View {
    let size: CGSize

    var body: some View {
        VStack(spacing: 6) {
            Image(systemName: "gauge.with.dots.needle.33percent")
                .font(.title2)
            Text("Cuotai")
                .font(.headline)
            Text("Open Cuotai to get started.")
                .font(.caption)
                .multilineTextAlignment(.center)
        }
        .foregroundStyle(.secondary)
        .box(size)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(Text("Cuotai. Open Cuotai to get started."))
    }
}

/// RF3.2: the pinned account is not in the file. Said plainly, never
/// replaced by another account's numbers.
struct AccountUnavailableView: View {
    let size: CGSize

    var body: some View {
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
        .box(size)
        .accessibilityElement(children: .combine)
    }
}

/// The file is there but this extension cannot read it: a newer schema
/// than it knows, a file it cannot decode, or one it may not open. Said
/// plainly, with the way out, instead of the "get started" text of a
/// missing file (and never the system's blank placeholder).
struct SnapshotUnreadableView: View {
    let size: CGSize

    var body: some View {
        VStack(spacing: 4) {
            Image(systemName: "arrow.triangle.2.circlepath")
                .font(.title3)
            Text("Update Cuotai")
                .font(.caption)
            Text("This widget cannot read the data Cuotai published. Update Cuotai and open it again.")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .box(size)
        .accessibilityElement(children: .combine)
    }
}


// MARK: - Quota

/// DS-ID-5 and DS-CMP-MTR-1: a widget shows what is **left** of a quota,
/// never what was used, in the level role of that share (`good` from 70%
/// left, `warn` from 30%, `bad` below or exhausted), as every other surface
/// of the app does (DS-COL-4).
enum QuotaLevel {
    case good, warn, bad

    init(remaining: Double, exhausted: Bool) {
        let percent = remaining * 100
        if exhausted || percent < 30 {
            self = .bad
        } else if percent < 70 {
            self = .warn
        } else {
            self = .good
        }
    }

    var color: Color {
        switch self {
        case .good: return .green
        case .warn: return .orange
        case .bad: return .red
        }
    }
}

/// One quota's share as the widgets draw it.
struct QuotaShare {
    /// The used fraction the file carries (`0...10`, above 1 over the limit).
    let used: Double
    /// The core said the quota is used up.
    let exhausted: Bool

    /// What is left, `0...1`; nothing once used up or over the limit.
    var remaining: Double {
        exhausted ? 0 : max(0, 1 - min(used.safeFraction, 1))
    }

    var level: QuotaLevel { QuotaLevel(remaining: remaining, exhausted: exhausted) }

    /// Nothing used yet (all of it left).
    var isUnused: Bool { !exhausted && used.safeFraction <= 0 }

    /// What was used, `0...10` (over the limit stays in the text).
    var usedShare: Double { exhausted ? max(used.safeFraction, 1) : used.safeFraction }

    /// The share the widget was set to show.
    func value(_ display: ShareDisplay) -> Double {
        display == .remaining ? remaining : usedShare
    }

    /// The bar's filled part, `0...1`.
    func fill(_ display: ShareDisplay) -> Double { min(value(display), 1) }

    /// DS-CMP-MTR-4: whole percent, rounded so it never lies: below 100
    /// never reads 100, above 0 never reads 0 (`nil`: "< 1%").
    func percent(_ display: ShareDisplay) -> Int? {
        let value = value(display) * 100
        if value > 0 && value < 1 { return nil }
        return value < 100 ? min(99, Int(value.rounded())) : Int(value.rounded())
    }

    /// The share in the widget's language ("19%", "19 %", "%19"; "< 1%").
    func text(_ locale: Locale, _ display: ShareDisplay = .remaining) -> String {
        let style = FloatingPointFormatStyle<Double>.Percent(locale: locale).precision(.fractionLength(0))
        guard let percent = percent(display) else { return "< " + (0.01).formatted(style) }
        return (Double(percent) / 100).formatted(style)
    }
}

/// The quota a widget headlines for an account: the metric the user chose
/// for the widget, else the binding one (PRD 0009).
struct Headline {
    let share: QuotaShare
    let label: String
    let pace: WidgetAccount.Metric.Pace?
    /// When the quota renews, when the file says.
    var resetsAt: Date? = nil
    /// Where it sits in the file's `metrics`, so the others can leave it out.
    let metricIndex: Int?
}

extension WidgetAccount {
    /// The quota to headline: the metric whose `choiceKey` is `metric` when
    /// the account has it, else the binding one; `nil` without either (no
    /// plan limit), so no other metric stands for the account.
    func headline(metric: String? = nil) -> Headline? {
        if let metric, let index = metrics.firstIndex(where: { $0.choiceKey == metric }) {
            let chosen = metrics[index]
            if let binding, binding.index == index {
                return Headline(share: QuotaShare(used: binding.usedRatio, exhausted: binding.isExhausted),
                                label: shortLabel(bindingLabel ?? chosen.displayLabel), pace: chosen.pace,
                                resetsAt: binding.resetsAt ?? chosen.resetsAt, metricIndex: index)
            }
            return Headline(share: QuotaShare(used: chosen.usedFraction, exhausted: chosen.usedFraction.safeFraction >= 1),
                            label: shortLabel(chosen.displayLabel), pace: chosen.pace,
                            resetsAt: chosen.resetsAt, metricIndex: index)
        }
        guard let binding else { return nil }
        let index = metrics.indices.contains(binding.index) ? binding.index : nil
        return Headline(share: QuotaShare(used: binding.usedRatio, exhausted: binding.isExhausted),
                        label: shortLabel(bindingLabel ?? ""), pace: index.flatMap { metrics[$0].pace },
                        resetsAt: binding.resetsAt ?? index.flatMap { metrics[$0].resetsAt },
                        metricIndex: index)
    }
}

// MARK: - One account

/// How much room an account gets. `row` is one entry of the medium and
/// large Summary (name and share, the bar, then the quota's name or the age
/// of an old reading); `compact` is one line and a thin bar (the small
/// Summary); `hero` is the only account of a small widget, with the share
/// as its headline.
enum AccountStyle {
    case row
    case compact
    case hero

    /// The fixed height of a list entry, so a widget knows how many fit.
    static func height(_ style: AccountStyle, stale: Bool = false) -> CGFloat {
        switch style {
        case .row: return 38
        case .compact: return stale ? 34 : 21
        case .hero: return 0
        }
    }
}

/// An account as the widgets draw it: what is left of its headline quota,
/// the account name (and the e-mail when the user asked for it) and, where
/// the style has room, the quota's name and the age of an old reading. It
/// is exactly `width` wide; a hero fills the height it is given, up to
/// `height`, with the bar and the age at its bottom.
struct AccountRowView: View {
    @Environment(\.locale) private var locale
    @Environment(\.widgetDisplay) private var display
    let account: WidgetAccount
    var style: AccountStyle = .row
    /// The metric the widget was set to headline (`Metric.choiceKey`).
    var metric: String? = nil
    let width: CGFloat
    /// The most a hero may grow to: it takes what its neighbours leave, up
    /// to this (a row is as tall as its lines).
    var height: CGFloat = 0

    private var headlineQuota: Headline? { account.isUnavailable ? nil : account.headline(metric: metric) }

    /// Whether the figure is a share (else "Unavailable" or "No plan
    /// limit", set smaller).
    private var showsShare: Bool { headlineQuota != nil }

    private var isOld: Bool { account.isStale || account.freshness == "aging" }

    /// A column too narrow for the unit word beside the share (the medium
    /// Summary's two columns, the small one): a row says it below the
    /// share instead, a compact line leaves it to VoiceOver.
    private var narrow: Bool { width < 200 }

    var body: some View {
        Group {
            switch style {
            case .row: row
            case .compact: compact
            case .hero: hero
            }
        }
        // DS §3.2: an old reading is drawn at half opacity, never hidden
        // and never presented as current.
        .opacity(account.isStale ? 0.5 : 1)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityLabel)
    }

    private var row: some View {
        VStack(alignment: .leading, spacing: 3) {
            figureLine(.callout)
            bar(height: 5)
            detail
                .frame(width: WidgetMetrics.finite(width), alignment: .leading)
        }
        .frame(width: WidgetMetrics.finite(width), height: AccountStyle.height(.row), alignment: .topLeading)
    }

    private var compact: some View {
        VStack(alignment: .leading, spacing: 3) {
            figureLine(.caption)
            bar(height: 4)
            if account.isStale {
                AgeLine(checkedAt: account.checkedAt, freshness: account.freshness, stale: true, width: width)
            }
        }
        .frame(width: WidgetMetrics.finite(width),
               height: AccountStyle.height(.compact, stale: account.isStale), alignment: .topLeading)
    }

    private var hero: some View {
        VStack(alignment: .leading, spacing: 2) {
            name
                .frame(width: WidgetMetrics.finite(width), alignment: .leading)
            HStack(alignment: .firstTextBaseline, spacing: 4) {
                figure
                    .font(.system(size: showsShare ? 32 : 17, weight: .semibold, design: .rounded))
                    .minimumScaleFactor(0.6)
                    .lineLimit(1)
                if showsShare {
                    unitWord
                        .font(.caption)
                        .lineLimit(1)
                }
            }
            .frame(width: WidgetMetrics.finite(width), alignment: .leading)
            quotaName
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
                .frame(width: WidgetMetrics.finite(width), alignment: .leading)
            Gap.vertical(height, min: 4)
            bar(height: 6)
                .padding(.bottom, 2)
            if display.showAge || isOld {
                AgeLine(checkedAt: account.checkedAt, freshness: account.freshness, stale: account.isStale, width: width)
            }
        }
        .frame(width: WidgetMetrics.finite(width))
        .frame(minHeight: 0, maxHeight: WidgetMetrics.finite(height), alignment: .topLeading)
    }

    /// Name on the left, the share and its unit word on the right.
    private func figureLine(_ size: Font.TextStyle) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 4) {
            name
                .layoutPriority(-1)
            Gap.horizontal(width, min: 4)
            (Text(verbatim: "") + figureText.font(.system(size, design: .rounded).weight(.semibold))
                + (showsShare && !narrow ? Text(verbatim: " ") + unitText.font(.caption2).foregroundColor(.secondary) : Text(verbatim: "")))
                .lineLimit(1)
                .fixedSize()
        }
        .frame(width: WidgetMetrics.finite(width), alignment: .leading)
    }

    /// The second line of a row: the age of an old reading (what matters
    /// most about it), else the quota's name.
    @ViewBuilder
    private var detail: some View {
        if isOld {
            AgeLine(checkedAt: account.checkedAt, freshness: account.freshness, stale: account.isStale, width: width)
        } else if narrow && showsShare {
            // Plain texts only (no live date here), so an `HStack` may
            // measure them: the name takes what the unit word leaves.
            HStack(alignment: .firstTextBaseline, spacing: 4) {
                quotaName
                    .lineLimit(1)
                    .layoutPriority(-1)
                Gap.horizontal(width, min: 4)
                unitText
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .fixedSize()
            }
            .frame(width: WidgetMetrics.finite(width), alignment: .leading)
        } else {
            quotaName
                .lineLimit(1)
        }
    }

    /// The share's bar; without a share, an invisible bar keeps the layout
    /// instead of an empty bar beside a word (DS-CMP-MTR-5).
    private func bar(height: CGFloat) -> some View {
        UsageBar(share: headlineQuota?.share ?? QuotaShare(used: 0, exhausted: false),
                 display: display.share, width: width, height: height)
            .opacity(showsShare ? 1 : 0)
    }

    private var name: some View {
        HStack(alignment: .firstTextBaseline, spacing: 4) {
            Text(verbatim: account.name)
                .font(.caption)
                .bold()
                .lineLimit(1)
            if let identity = account.shownIdentity {
                Text(verbatim: identity)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        }
    }

    private var figureText: Text {
        if account.isUnavailable {
            return Text("Unavailable").foregroundColor(.secondary)
        }
        guard let headlineQuota else {
            return Text("No plan limit").foregroundColor(.secondary)
        }
        return Text(verbatim: headlineQuota.share.text(locale, display.share)).foregroundColor(headlineQuota.share.level.color)
    }

    private var figure: some View {
        figureText.monospacedDigit()
    }

    /// The unit word after the share (DS-MSG-4): "left" or "used".
    private var unitText: Text { display.share == .remaining ? Text("left") : Text("used") }

    private var unitWord: some View {
        unitText.foregroundStyle(.secondary)
    }

    /// The quota's name, or "Exhausted" once it is used up.
    @ViewBuilder
    private var quotaName: some View {
        if let headlineQuota {
            Group {
                // A narrow row (the medium Summary's two columns) has no
                // room for the reset beside the quota's name.
                let reset = display.showReset && (style != .row || width > 200)
                    ? ResetText.text(headlineQuota.resetsAt, now: display.now, locale: locale) : nil
                // Used up: say it first, and still which quota it is.
                let label = Text(verbatim: headlineQuota.label)
                let name = headlineQuota.share.exhausted
                    ? Text("Exhausted").foregroundColor(QuotaLevel.bad.color)
                        + (headlineQuota.label.isEmpty ? Text(verbatim: "") : Text(verbatim: " · ") + label)
                    : label
                if let reset {
                    name + Text(verbatim: " · ") + reset
                } else {
                    name
                }
            }
            .font(.caption2)
            .foregroundStyle(.secondary)
        }
    }

    /// VoiceOver (RF3.4): the same facts, said, never colour alone. A
    /// `Text`, so it is looked up in the widget's language (PRD 0034), not
    /// the process's.
    private var accessibilityLabel: Text {
        if account.isUnavailable {
            return Text("\(account.name). Unavailable")
        }
        guard let headlineQuota else {
            return Text("\(account.name). No plan limit")
        }
        let share = headlineQuota.share.text(locale, display.share)
        if display.share == .used {
            return account.isStale
                ? Text("\(account.name). \(share) used. Stale.")
                : Text("\(account.name). \(share) used. Current.")
        }
        return account.isStale
            ? Text("\(account.name). \(share) left. Stale.")
            : Text("\(account.name). \(share) left. Current.")
    }
}

/// What is left of a quota, as a bar in the level role of that share
/// (DS-CMP-MTR-1). Both capsules have a fixed size computed from `width`:
/// neither takes its width from the proposal (RF3.3: widgets do not animate
/// either). A share above nothing is at least a dot (DS-CMP-MTR-3), below
/// all of it never the full bar (DS-CMP-MTR-4); a used-up quota is the bare
/// track (DS-CMP-MTR-5).
struct UsageBar: View {
    let share: QuotaShare
    var display: ShareDisplay = .remaining
    let width: CGFloat
    var height: CGFloat = 5

    var body: some View {
        let track = WidgetMetrics.finite(width)
        let remaining = share.fill(display)
        let fill: CGFloat
        if remaining <= 0 {
            fill = 0
        } else if remaining < 1 {
            fill = min(max(track * remaining, height), max(track - 2, 0))
        } else {
            fill = track
        }
        return ZStack(alignment: .leading) {
            Capsule()
                .fill(.quaternary)
                .frame(width: track, height: height)
            Capsule()
                .fill(share.level.color)
                .frame(width: fill, height: height)
        }
        .frame(width: track, height: height, alignment: .leading)
        .accessibilityHidden(true)
    }
}

// MARK: - Reset

/// When a quota renews, counted from the entry's date ("Resets in 2 days").
/// A plain text computed per timeline entry (`WidgetTimeline.dates`), not a
/// live one: it only has to be right to the quarter of an hour.
enum ResetText {
    static func text(_ date: Date?, now: Date, locale: Locale) -> Text? {
        guard let date, date > now else { return nil }
        let formatter = RelativeDateTimeFormatter()
        formatter.locale = locale
        formatter.unitsStyle = .short
        formatter.dateTimeStyle = .numeric
        let interval = date.timeIntervalSince(now)
        // Whole units, never a countdown: minutes below an hour, hours
        // below two days, days above.
        let rounded: TimeInterval
        if interval < 3600 {
            rounded = max(60, (interval / 60).rounded(.up) * 60)
        } else if interval < 48 * 3600 {
            rounded = (interval / 3600).rounded() * 3600
        } else {
            rounded = (interval / 86400).rounded() * 86400
        }
        let relative = formatter.localizedString(fromTimeInterval: rounded)
        return Text("Resets \(relative)")
    }
}

// MARK: - Age

/// RF3.3: the measurement age ("2 minutes ago") in the system's own
/// relative text, which the system advances by itself, after `Stale` or
/// `Aging` in the warning role when the reading is old (DS §3.2).
///
/// One `Text`, in a frame of a width computed from the widget, never sized
/// to its own content: a live date text measured by its content (an
/// `HStack` around it, `fixedSize()`) is drawn by macOS 27 at the widget's
/// edge, over the other lines, instead of where the layout put it.
enum Age {
    static func text(checkedAt: Date?, freshness: String?, stale: Bool) -> Text {
        let body = checkedAt.map(relative) ?? Text("No reading yet")
        if stale {
            return Text("Stale").foregroundColor(QuotaLevel.warn.color) + Text(verbatim: " · ") + body
        }
        if freshness == "aging" {
            return Text("Aging").foregroundColor(QuotaLevel.warn.color) + Text(verbatim: " · ") + body
        }
        return body
    }

    /// Whole minutes, hours or days: the plain relative style ticks every
    /// second ("1 min, 4 sec"), which reads as a countdown.
    static func relative(_ date: Date) -> Text {
        if #available(macOS 15, *) {
            return Text(.currentDate, format: .reference(to: date, allowedFields: [.minute, .hour, .day], maxFieldCount: 1))
        }
        return Text(date, style: .relative)
    }
}

/// [Age] as a line exactly `width` wide; the exact instant is the
/// VoiceOver label.
struct AgeLine: View {
    @Environment(\.locale) private var locale
    let checkedAt: Date?
    let freshness: String?
    var stale = false
    let width: CGFloat
    var alignment: Alignment = .leading

    var body: some View {
        Age.text(checkedAt: checkedAt, freshness: freshness, stale: stale)
            .font(.caption2)
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .multilineTextAlignment(alignment == .trailing ? .trailing : .leading)
            .frame(width: WidgetMetrics.finite(width), alignment: alignment)
            .accessibilityLabel(label)
    }

    private var label: Text {
        guard let checkedAt else { return Text("No reading yet") }
        let when = checkedAt.formatted(Date.FormatStyle(date: .abbreviated, time: .shortened, locale: locale))
        if stale { return Text("Stale") + Text(verbatim: ". ") + Text("Updated \(when)") }
        return Text("Updated \(when)")
    }
}

/// The last line of a list: the age of the oldest current reading on the
/// left (one per widget; an old row says its own), and on the right how
/// many accounts did not fit. Two frames of fixed widths, so neither text
/// is sized by the other.
struct ListFooter: View {
    let checkedAt: Date?
    let hidden: Int
    let truncated: Bool
    let accounts: Int
    let width: CGFloat

    var body: some View {
        let right = width * 0.42
        VStack(alignment: .leading, spacing: 2) {
            // Overlaid, each as wide as the widget: the live age is never
            // laid out beside another view that measures it.
            ZStack(alignment: .topLeading) {
                if let checkedAt {
                    AgeLine(checkedAt: checkedAt, freshness: nil, width: width - right)
                }
                Group {
                    if hidden > 0 {
                        Text("\(hidden) more")
                    } else {
                        Text(verbatim: "")
                    }
                }
                .font(.caption2)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .minimumScaleFactor(0.8)
                .frame(width: WidgetMetrics.finite(width), alignment: .trailing)
            }
            .frame(width: WidgetMetrics.finite(width), height: 13, alignment: .topLeading)
            if hidden == 0 && truncated {
                TruncationNotice(truncated: truncated, accounts: accounts)
                    .frame(width: WidgetMetrics.finite(width), alignment: .leading)
            }
        }
        .frame(width: WidgetMetrics.finite(width), alignment: .leading)
    }

    /// The footer's height, for the rows above it.
    static func height(truncated: Bool) -> CGFloat { truncated ? 28 : 13 }
}
