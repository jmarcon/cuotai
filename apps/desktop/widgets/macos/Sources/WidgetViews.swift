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
/// the inner size it is handed.
struct WidgetCanvas<Content: View>: View {
    @Environment(\.widgetFamily) private var family
    @Environment(\.widgetContentMargins) private var systemMargins
    /// `TimelineProviderContext.displaySize`, carried by the entry.
    let displaySize: CGSize
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

// MARK: - One account

/// How much room an account gets. `row` is one line of the Summary list
/// (name and percentage, the bar, then the metric and its age); `hero` is
/// the only account of a small widget, with the percentage as its headline.
enum AccountStyle {
    case row
    case hero
}

/// An account as the widgets draw it: the binding metric with its
/// percentage, the account name (and the e-mail when the user asked for it)
/// and the age of the reading. It is exactly `width` wide; a hero fills the
/// height it is given, up to `height`, with the bar and the age at its
/// bottom.
struct AccountRowView: View {
    let account: WidgetAccount
    var style: AccountStyle = .row
    /// The medium Summary prints one age in its header; a row then shows
    /// its own only when the reading is old (stale or aging).
    var ageInHeader = false
    let width: CGFloat
    /// The most a hero may grow to: it takes what its neighbours leave, up
    /// to this (a row is as tall as its lines).
    var height: CGFloat = 0

    /// The binding metric's share. Only an account with a plan or rate
    /// limit has one: another metric's share is not the account's quota.
    private var percent: Int {
        Int(((account.binding?.usedRatio ?? 0).safeFraction * 100).rounded())
    }

    /// Whether the headline is a percentage (else "Unavailable" or "No plan
    /// limit", set smaller).
    private var showsPercent: Bool { !account.isUnavailable && account.hasPlanLimit }

    private var tone: UsageTone {
        UsageTone(usedFraction: account.binding?.usedRatio ?? 0,
                  exhausted: account.binding?.isExhausted ?? false,
                  pace: account.metrics.first?.pace?.tone)
    }

    /// The binding metric's bar; without one, an invisible bar keeps the
    /// layout instead of an empty bar beside a figure.
    private var bar: some View {
        UsageBar(usedFraction: account.binding?.usedRatio ?? 0,
                 width: width,
                 stale: account.isStale,
                 tone: tone)
            .opacity(account.hasPlanLimit ? 1 : 0)
    }

    var body: some View {
        Group {
            switch style {
            case .row: row
            case .hero: hero
            }
        }
        // Old data is dimmed, never hidden and never presented as current.
        .opacity(account.isStale ? 0.6 : 1)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(Text(accessibilityLabel))
    }

    private var row: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline, spacing: 4) {
                name
                Gap.horizontal(width, min: 4)
                headline
                    .font(.system(showsPercent ? .callout : .caption, design: .rounded).weight(.semibold))
                    .fixedSize()
            }
            .frame(width: width, alignment: .leading)
            bar
            HStack(alignment: .firstTextBaseline, spacing: 4) {
                metricLabel
                    .layoutPriority(-1)
                Gap.horizontal(width, min: 4)
                // RF3.3: the system's own relative text, so the age advances
                // without the app; the absolute instant stays in VoiceOver.
                if !ageInHeader || account.isStale || account.freshness == "aging" {
                    MeasurementAge(checkedAt: account.checkedAt, freshness: account.freshness, stale: account.isStale)
                }
            }
            .frame(width: width, alignment: .leading)
        }
        .frame(width: width, alignment: .leading)
    }

    private var hero: some View {
        VStack(alignment: .leading, spacing: 2) {
            name
                .frame(width: width, alignment: .leading)
            headline
                .font(.system(size: showsPercent ? 32 : 17, weight: .semibold, design: .rounded))
                .minimumScaleFactor(0.6)
                .lineLimit(1)
                .frame(width: width, alignment: .leading)
            metricLabel
                .lineLimit(2)
                .frame(width: width, alignment: .leading)
            Gap.vertical(height, min: 4)
            bar
                .padding(.bottom, 2)
            MeasurementAge(checkedAt: account.checkedAt, freshness: account.freshness, stale: account.isStale)
                .frame(width: width, alignment: .leading)
        }
        .frame(width: width)
        .frame(minHeight: 0, maxHeight: WidgetMetrics.finite(height), alignment: .topLeading)
    }

    private var name: some View {
        HStack(alignment: .firstTextBaseline, spacing: 4) {
            Text(account.name)
                .font(.caption)
                .bold()
                .lineLimit(1)
            if let identity = account.shownIdentity {
                Text(identity)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        }
    }

    private var headline: some View {
        Group {
            if account.isUnavailable {
                Text("Unavailable")
            } else if !account.hasPlanLimit {
                Text("No plan limit")
            } else {
                Text("\(percent)%")
            }
        }
        .monospacedDigit()
        .foregroundStyle(showsPercent ? tone.headlineColor : .secondary)
    }

    @ViewBuilder
    private var metricLabel: some View {
        if let binding = account.binding {
            Group { if binding.isExhausted { Text("Exhausted") } else { Text(verbatim: binding.shortLabel) } }
                .font(.caption2)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
    }

    /// VoiceOver (RF3.4): the same facts, said, never colour alone.
    private var accessibilityLabel: String {
        if account.isUnavailable {
            return String(localized: "\(account.name). Unavailable")
        }
        if !account.hasPlanLimit {
            return String(localized: "\(account.name). No plan limit")
        }
        return account.isStale
            ? String(localized: "\(account.name). \(percent) percent used. Stale.")
            : String(localized: "\(account.name). \(percent) percent used. Current.")
    }
}

/// One colour rule for a share, so the bar and the figure beside it always
/// agree: over the limit or exhausted is bad; then the pace tone (PRD 0007);
/// 90% or more is a warning even when the pace is fine.
enum UsageTone {
    case neutral, good, warn, bad

    init(usedFraction: Double, exhausted: Bool = false, pace: String? = nil) {
        let used = usedFraction.safeFraction
        if exhausted || used > 1 || pace == "bad" {
            self = .bad
        } else if used >= 0.9 || pace == "warn" {
            self = .warn
        } else if pace == "good" {
            self = .good
        } else {
            self = .neutral
        }
    }

    var headlineColor: Color {
        switch self {
        case .bad: return .red
        case .warn: return .orange
        case .good: return .green
        case .neutral: return .primary
        }
    }

    var barColor: Color {
        switch self {
        case .neutral: return .accentColor
        default: return headlineColor
        }
    }
}

/// The consumed share of the binding metric. A bar only, so it reads in the
/// light and dark themes; the colour is a hint, the label is the truth.
/// Both capsules have a fixed size computed from `width`: neither takes its
/// width from the proposal (RF3.3: widgets do not animate either).
struct UsageBar: View {
    let usedFraction: Double
    let width: CGFloat
    var stale = false
    /// The same tone as the percentage beside it.
    var tone: UsageTone? = nil
    var height: CGFloat = 5

    private var clamped: Double { min(usedFraction.safeFraction, 1) }

    var body: some View {
        let track = WidgetMetrics.finite(width)
        let fill = clamped > 0 ? min(track, max(track * clamped, height)) : 0
        ZStack(alignment: .leading) {
            Capsule()
                .fill(.quaternary)
                .frame(width: track, height: height)
            Capsule()
                .fill((tone ?? UsageTone(usedFraction: usedFraction)).barColor)
                .frame(width: fill, height: height)
        }
        .frame(width: track, height: height, alignment: .leading)
        .opacity(stale ? 0.5 : 1)
        .accessibilityHidden(true)
    }
}

/// RF3.3: the measurement age ("2 minutes ago"), which the system advances
/// by itself, plus the `Stale` state when the data is preserved; the exact
/// instant is the VoiceOver label.
struct MeasurementAge: View {
    let checkedAt: Date?
    let freshness: String?
    var stale = false

    var body: some View {
        HStack(spacing: 3) {
            if stale {
                Text("Stale")
                Text(verbatim: "·")
            } else if freshness == "aging" {
                Text("Aging")
                Text(verbatim: "·")
            }
            if let checkedAt {
                relative(checkedAt)
                    .accessibilityLabel(Text("Updated \(checkedAt.formatted(date: .abbreviated, time: .shortened))"))
            } else {
                Text("No reading yet")
            }
        }
        .font(.caption2)
        .foregroundStyle(.secondary)
        .lineLimit(1)
        .fixedSize()
    }

    /// Whole minutes, hours or days: the plain relative style ticks every
    /// second ("1 min, 4 sec"), which reads as a countdown.
    @ViewBuilder
    private func relative(_ date: Date) -> some View {
        if #available(macOS 15, *) {
            Text(.currentDate, format: .reference(to: date, allowedFields: [.minute, .hour, .day], maxFieldCount: 1))
        } else {
            Text(date, style: .relative)
        }
    }
}

/// The headline of a widget family, for VoiceOver and for the lock screen.
struct WidgetHeader: View {
    let title: Text
    let width: CGFloat
    /// The age of the oldest current reading, when the rows leave it out.
    var checkedAt: Date? = nil

    var body: some View {
        HStack(spacing: 4) {
            Text("Cuotai")
                .font(.caption2)
                .bold()
                .fixedSize()
            Text("·")
                .font(.caption2)
                .foregroundStyle(.tertiary)
                .fixedSize()
            title
                .font(.caption2)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            Gap.horizontal(width)
            if let checkedAt {
                MeasurementAge(checkedAt: checkedAt, freshness: nil)
            }
        }
        .frame(width: width, alignment: .leading)
    }
}
