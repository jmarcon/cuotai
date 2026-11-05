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

    private var percent: Int {
        let ratio = account.binding?.usedRatio ?? account.metrics.first?.usedFraction ?? 0
        return Int((ratio * 100).rounded())
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
                    .font(.system(account.isUnavailable ? .caption : .callout, design: .rounded).weight(.semibold))
            }
            UsageBar(usedFraction: account.binding?.usedRatio ?? 0,
                     stale: account.isStale,
                     exhausted: account.binding?.isExhausted ?? false,
                     tone: account.metrics.first?.pace?.tone)
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
                .font(.system(size: account.isUnavailable ? 17 : 32, weight: .semibold, design: .rounded))
                .minimumScaleFactor(0.6)
                .lineLimit(1)
            metricLabel
                .lineLimit(2)
            Spacer(minLength: 4)
            UsageBar(usedFraction: account.binding?.usedRatio ?? 0,
                     stale: account.isStale,
                     exhausted: account.binding?.isExhausted ?? false,
                     tone: account.metrics.first?.pace?.tone)
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
        Group { account.isUnavailable ? Text("Unavailable") : Text("\(percent)%") }
            .monospacedDigit()
            .foregroundStyle(account.isUnavailable ? .secondary : headlineColor)
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
        return account.isStale
            ? String(localized: "\(account.name). \(percent) percent used. Stale.")
            : String(localized: "\(account.name). \(percent) percent used. Current.")
    }

    private var headlineColor: Color {
        if account.binding?.isExhausted ?? false { return .red }
        if let tone = account.metrics.first?.pace?.tone {
            switch tone {
            case "good": return .green
            case "warn": return .orange
            case "bad": return .red
            default: return .primary
            }
        }
        return .primary
    }
}

/// The consumed share of the binding metric. A bar only, so it reads in the
/// light and dark themes; the colour is a hint, the label is the truth.
struct UsageBar: View {
    let usedFraction: Double
    var stale = false
    var exhausted = false
    /// The pace tone (PRD 0007), so the bar agrees with the percentage.
    var tone: String? = nil
    var height: CGFloat = 5

    private var clamped: Double { min(max(usedFraction, 0), 1) }

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
                        .fill(barColor)
                        .frame(width: max(proxy.size.width * clamped, clamped > 0 ? height : 0))
                }
            }
            .frame(height: height)
            .opacity(stale ? 0.5 : 1)
            .accessibilityHidden(true)
    }

    private var barColor: Color {
        if exhausted || usedFraction > 1 { return .red }
        if usedFraction >= 0.9 || tone == "warn" { return .orange }
        if tone == "bad" { return .red }
        return .accentColor
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