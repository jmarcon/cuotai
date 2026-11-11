import SwiftUI

// PRD 0014 RF3.3/RF3.4/RF3.5: how an account is drawn — the measurement age
// in the system's own relative text (which the system advances by itself),
// dimmed bars for preserved readings, no animation, and the empty state when
// the file is absent. Colours never carry meaning alone: the tone has a label.

// MARK: - Empty state

/// RF3.5: "Open Cuotai to get started" when there is no file. Never sample
/// data disguised as real.
struct EmptyStateView: View {
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
        .accessibilityElement(children: .combine)
        .accessibilityLabel(Text("Cuotai. Open Cuotai to get started."))
    }
}

/// RF3.2: the pinned account is not in the file. Said plainly, never
/// replaced by another account's numbers.
struct AccountUnavailableView: View {
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
        .frame(maxWidth: .infinity, maxHeight: .infinity)
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
/// and the age of the reading.
struct AccountRowView: View {
    let account: WidgetAccount
    var style: AccountStyle = .row
    /// The medium Summary prints one age in its header; a row then shows
    /// its own only when the reading is old (stale or aging).
    var ageInHeader = false

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
                Spacer(minLength: 4)
                headline
                    .font(.system(showsPercent ? .callout : .caption, design: .rounded).weight(.semibold))
            }
            bar
            HStack(alignment: .firstTextBaseline, spacing: 4) {
                metricLabel
                    .layoutPriority(-1)
                Spacer(minLength: 4)
                // RF3.3: the system's own relative text, so the age advances
                // without the app; the absolute instant stays in VoiceOver.
                if !ageInHeader || account.isStale || account.freshness == "aging" {
                    MeasurementAge(checkedAt: account.checkedAt, freshness: account.freshness, stale: account.isStale)
                }
            }
        }
    }

    private var hero: some View {
        VStack(alignment: .leading, spacing: 2) {
            name
            headline
                .font(.system(size: showsPercent ? 32 : 17, weight: .semibold, design: .rounded))
                .minimumScaleFactor(0.6)
                .lineLimit(1)
            metricLabel
                .lineLimit(2)
            Spacer(minLength: 4)
            bar
                .padding(.bottom, 2)
            MeasurementAge(checkedAt: account.checkedAt, freshness: account.freshness, stale: account.isStale)
        }
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
struct UsageBar: View {
    let usedFraction: Double
    var stale = false
    /// The same tone as the percentage beside it.
    var tone: UsageTone? = nil
    var height: CGFloat = 5

    private var clamped: Double { min(usedFraction.safeFraction, 1) }

    var body: some View {
        // The fill is a share of the bar's own track, never of the widget:
        // a fill sized from the container overflows a column and pushes the
        // whole layout past the widget's edges. Widgets do not animate, so
        // the GeometryReader is inert (RF3.3).
        Capsule()
            .fill(.quaternary)
            .overlay(alignment: .leading) {
                GeometryReader { proxy in
                    Capsule()
                        .fill((tone ?? UsageTone(usedFraction: usedFraction)).barColor)
                        .frame(width: max(proxy.size.width * clamped, clamped > 0 ? height : 0))
                }
            }
            .frame(height: height)
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
    /// The age of the oldest current reading, when the rows leave it out.
    var checkedAt: Date? = nil

    var body: some View {
        HStack(spacing: 4) {
            Text("Cuotai")
                .font(.caption2)
                .bold()
            Text("·")
                .font(.caption2)
                .foregroundStyle(.tertiary)
            title
                .font(.caption2)
                .foregroundStyle(.secondary)
            Spacer(minLength: 0)
            if let checkedAt {
                MeasurementAge(checkedAt: checkedAt, freshness: nil)
            }
        }
    }
}