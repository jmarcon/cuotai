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

/// An account as the widgets draw it: the binding metric with its
/// percentage, the account name (and the e-mail when the user asked for it)
/// and the age of the reading.
struct AccountRowView: View {
    let account: WidgetAccount
    /// The medium widget fits two accounts side by side; the large one a
    /// full row each.
    var compact = false

    private var percent: Int {
        let ratio = account.binding?.usedRatio ?? account.metrics.first?.usedFraction ?? 0
        return Int((ratio * 100).rounded())
    }

    private var headline: Text {
        account.isUnavailable ? Text("Unavailable") : Text("\(percent)%")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: compact ? 1 : 3) {
            HStack(spacing: 4) {
                Text(account.name)
                    .font(compact ? .caption2 : .caption)
                    .bold()
                    .lineLimit(1)
                if let identity = account.shownIdentity {
                    Text(identity)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
            }
            HStack(alignment: .firstTextBaseline, spacing: 4) {
                headline
                    .font(compact ? .title3 : .title2)
                    .monospacedDigit()
                    .foregroundStyle(headlineColor)
                if let binding = account.binding {
                    Text(binding.isExhausted ? "Exhausted" : binding.label)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            UsageBar(usedFraction: account.binding?.usedRatio ?? 0,
                     stale: account.isStale,
                     exhausted: account.binding?.isExhausted ?? false)
            // RF3.3: the system's own relative text, so the age advances
            // without the app; the absolute instant stays in the tooltip.
            MeasurementAge(checkedAt: account.checkedAt, freshness: account.freshness, stale: account.isStale)
        }
        // Old data is dimmed, never hidden and never presented as current.
        .opacity(account.isStale ? 0.6 : 1)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(Text(accessibilityLabel))
    }

    /// VoiceOver (RF3.4): the same facts, said, never colour alone.
    private var accessibilityLabel: String {
        if account.isUnavailable {
            return "\(account.name). Unavailable"
        }
        let freshnessText = account.isStale ? "Stale" : "Current"
        return "\(account.name). \(percent) percent used. \(freshnessText)."
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

    private var clamped: Double { min(max(usedFraction, 0), 1) }

    var body: some View {
        // A GeometryReader would animate on resize; a fixed-height track with
        // an overlay stays still (RF3.3, no animation).
        ZStack(alignment: .leading) {
            Capsule().fill(.quaternary)
            Capsule()
                .fill(barColor)
                .frame(width: nil, alignment: .leading)
                .containerRelativeFrame(.horizontal) { length, _ in length * clamped }
        }
        .frame(height: 6)
        .opacity(stale ? 0.5 : 1)
        .accessibilityHidden(true)
    }

    private var barColor: Color {
        if exhausted || usedFraction > 1 { return .red }
        if usedFraction >= 0.9 { return .orange }
        return .accentColor
    }
}

/// RF3.3: the measurement age in the system's relative style, plus the
/// `Stale` state when the data is preserved; the exact instant is the
/// tooltip and the VoiceOver label.
struct MeasurementAge: View {
    let checkedAt: Date?
    let freshness: String?
    var stale = false

    var body: some View {
        HStack(spacing: 3) {
            if stale {
                Text("Stale")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            } else if freshness == "aging" {
                Text("Aging")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            if let checkedAt {
                Text(checkedAt, style: .relative)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .accessibilityLabel(Text("Updated \(checkedAt.formatted(date: .abbreviated, time: .shortened))"))
            } else {
                Text("No reading yet")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

/// The headline of a widget family, for VoiceOver and for the lock screen.
struct WidgetHeader: View {
    let title: String

    var body: some View {
        HStack(spacing: 4) {
            Text("Cuotai")
                .font(.caption2)
                .bold()
            Text("·")
                .font(.caption2)
                .foregroundStyle(.tertiary)
            Text(title)
                .font(.caption2)
                .foregroundStyle(.secondary)
            Spacer(minLength: 0)
        }
    }
}