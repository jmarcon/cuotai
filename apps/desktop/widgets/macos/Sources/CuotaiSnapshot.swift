import Foundation

// PRD 0014 RF1/RF3: the snapshot the app publishes (`widget_snapshot` of
// `cuotai-core`, schema 1) and nothing else. The extension never queries a
// provider, never reads a credential and never computes usage: it draws what
// the app already measured. See `docs/widgets.md` for the contract.

/// Where the app writes the file: the *app group* container when the signed
/// app has the entitlement (RF2.1); otherwise the same per-user location the
/// app falls back to, which a sandboxed extension cannot reach.
enum SnapshotLocation {
    static let appGroup = "group.dev.cuotai.desktop"
    static let fileName = "widget.json"

    static var url: URL? {
        let container = FileManager.default
            .containerURL(forSecurityApplicationGroupIdentifier: appGroup)
        if let container {
            return container.appendingPathComponent(fileName)
        }
        return fallback
    }

    /// `<local data>/cuotai/widget.json` (RF5.1): the same directory the app
    /// falls back to, so the two agree when there is no *app group*.
    static var fallback: URL? {
        FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first?
            .appendingPathComponent("cuotai")
            .appendingPathComponent(fileName)
    }
}

/// One account of the file. Fields the widgets do not read are absent here
/// too: the extension has no use for them.
struct WidgetAccount: Codable, Identifiable {
    let key: String
    let name: String
    let provider: String
    /// The e-mail, only when the user turned identity on *and* the account
    /// has one; absent or `null` otherwise, which both mean "no e-mail".
    let email: String?
    let state: String
    let checkedAt: Date?
    let freshness: String?
    let binding: Binding?
    let metrics: [Metric]
    let history: [HistoryPoint]?

    /// The core writes `snake_case`; Swift's synthesized keys would expect
    /// `checkedAt`, so every key is declared here.
    enum CodingKeys: String, CodingKey {
        case key, name, provider, email, state, checkedAt = "checked_at"
        case freshness, binding, metrics, history
    }

    var id: String { key }

    enum State {
        static let ready = "ready"
        static let stale = "stale"
        static let unavailable = "unavailable"
    }

    /// An account that cannot be drawn: no reading, current or preserved.
    var isUnavailable: Bool { state == State.unavailable || metrics.isEmpty }

    /// Data older than the current query: dimmed, with its original age.
    var isStale: Bool { state == State.stale }

    /// The identity to show, when the user asked for it and it is known.
    var shownIdentity: String? {
        guard let email, !email.isEmpty else { return nil }
        return email
    }

    struct Binding: Codable {
        let index: Int
        let label: String
        let window: String?
        /// Consumed fraction in `0...1` (`1` when exhausted).
        let usedRatio: Double
        let remaining: Double
        let resetsAt: Date?
        let reason: String
        let blockedUntil: Date?

        enum CodingKeys: String, CodingKey {
            case index, label, window, remaining, reason
            case usedRatio = "used_ratio"
            case resetsAt = "resets_at"
            case blockedUntil = "blocked_until"
        }

        var isExhausted: Bool { reason == "exhausted" }
    }

    struct Metric: Codable {
        let label: String
        let usedFraction: Double
        let limit: Double
        let unit: String
        let window: String?
        let resetsAt: Date?
        /// The consumption pace (PRD 0007) when the app could assess it.
        let pace: Pace?

        enum CodingKeys: String, CodingKey {
            case label, limit, unit, window, pace
            case usedFraction = "used_fraction"
            case resetsAt = "resets_at"
        }

        var isOverused: Bool { usedFraction > 1 }

        struct Pace: Codable {
            let elapsedFraction: Double
            let expectedUsedFraction: Double
            let usedFraction: Double
            let deltaPoints: Double
            let stage: String
            let tone: String
            let source: String
            let resetsAt: Date
            let projectedExhaustionAt: Date?
            let lastsUntilReset: Bool

            enum CodingKeys: String, CodingKey {
                case stage, tone, source
                case elapsedFraction = "elapsed_fraction"
                case expectedUsedFraction = "expected_used_fraction"
                case usedFraction = "used_fraction"
                case deltaPoints = "delta_points"
                case resetsAt = "resets_at"
                case projectedExhaustionAt = "projected_exhaustion_at"
                case lastsUntilReset = "lasts_until_reset"
            }
        }
    }

    struct HistoryPoint: Codable {
        let takenAt: Date
        let usedFraction: Double

        enum CodingKeys: String, CodingKey {
            case takenAt = "taken_at"
            case usedFraction = "used_fraction"
        }
    }
}

/// The published file. `truncated` says that accounts were left out, so a
/// widget never shows an empty list as if no account existed.
struct WidgetSnapshot: Codable {
    let schema: Int
    let generatedAt: Date
    let accounts: [WidgetAccount]
    let truncated: Bool?

    enum CodingKeys: String, CodingKey {
        case schema, accounts, truncated
        case generatedAt = "generated_at"
    }

    /// The schema this extension understands; a newer file is read as
    /// unknown rather than half-interpreted.
    static let supportedSchema = 1

    /// The file as it is now, `nil` when it is absent (the empty state) or
    /// unreadable: a widget never draws made-up numbers.
    static func load(from url: URL? = SnapshotLocation.url) -> WidgetSnapshot? {
        guard let url, let data = try? Data(contentsOf: url) else { return nil }
        let decoder = JSONDecoder()
        // The core serializes RFC 3339 with sub-second precision
        // (`2026-10-20T12:00:00.123456Z`); `.iso8601` alone rejects the
        // fraction, so parse it explicitly.
        decoder.dateDecodingStrategy = .custom { decoder in
            let raw = try decoder.singleValueContainer().decode(String.self)
            let formatter = ISO8601DateFormatter()
            formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            if let date = formatter.date(from: raw) { return date }
            formatter.formatOptions = [.withInternetDateTime]
            guard let date = formatter.date(from: raw) else {
                throw DecodingError.dataCorrupted(
                    .init(codingPath: decoder.codingPath,
                          debugDescription: "not an RFC 3339 date: \(raw)"))
            }
            return date
        }
        guard let snapshot = try? decoder.decode(WidgetSnapshot.self, from: data),
              snapshot.schema <= WidgetSnapshot.supportedSchema
        else { return nil }
        return snapshot
    }
}