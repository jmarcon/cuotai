import Foundation

// PRD 0014 RF1/RF3: the snapshot the app publishes (`widget_snapshot` of
// `cuotai-core`, schema 1) and nothing else. The extension never queries a
// provider, never reads a credential and never computes usage: it draws what
// the app already measured. See `docs/widgets.md` for the contract.

/// Where the app writes the file: `<local data>/cuotai/widget.json`, the one
/// path outside its container the sandbox lets this extension read
/// (`CuotaiWidgets.entitlements`). `FileManager` would answer the container,
/// so the path starts at the real home directory.
enum SnapshotLocation {
    static let fileName = "widget.json"

    static var url: URL? {
        guard let home = getpwuid(getuid())?.pointee.pw_dir else { return nil }
        return URL(fileURLWithPath: String(cString: home))
            .appendingPathComponent("Library/Application Support/cuotai")
            .appendingPathComponent(fileName)
    }
}

/// One account of the file. Fields the widgets do not read are absent here
/// too: the extension has no use for them.
struct WidgetAccount: Codable, Identifiable {
    /// PRD 0029: the stable account id, which a rename keeps; absent in a
    /// file written by an older app.
    let accountId: String?
    let key: String
    /// The account's place in the configuration (schema revision 2);
    /// absent in a file written by an older app, which then keeps the
    /// file's own order.
    let position: Int?
    let name: String
    let provider: String
    /// The provider's display name; absent in a file written by an older
    /// app, which then shows the provider id.
    let providerName: String?
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
        case accountId = "id", key, name, provider, email, state, checkedAt = "checked_at"
        case providerName = "provider_name"
        case freshness, binding, metrics, history, position
    }

    /// What a pinned widget stores: the stable id, else the legacy key.
    var id: String { accountId ?? key }

    /// Whether `pinned` (an id, or the key a widget pinned before ids)
    /// names this account.
    func isPinned(_ pinned: String) -> Bool { pinned == id || pinned == key }

    /// The binding metric's name to draw: the text in the app's language
    /// (the metric's, else the binding's), else the English default the
    /// file carries, else the raw label of an older file.
    var bindingLabel: String? {
        guard let binding else { return nil }
        let metric = metrics.indices.contains(binding.index) ? metrics[binding.index] : nil
        return WidgetText.choose([metric?.labelText, binding.labelText], raw: binding.rawLabel)
            .strippingPercentHint
    }

    /// [label] without a leading name the widget already shows beside it
    /// ("Codex — 1-week window" under the account Codex reads "1-week
    /// window"), so a small widget has room for what tells the quotas
    /// apart. Any other label is unchanged.
    func shortLabel(_ label: String) -> String {
        let names = [providerName, Optional(provider), Optional(name)].compactMap { $0 }.filter { !$0.isEmpty }
        for name in names {
            for separator in [" — ", " – ", " - ", ": "] {
                let prefix = name + separator
                if label.count > prefix.count,
                   label.range(of: prefix, options: [.anchored, .caseInsensitive]) != nil {
                    let rest = String(label.dropFirst(prefix.count)).trimmingCharacters(in: .whitespaces)
                    guard let first = rest.first else { return label }
                    return first.uppercased() + rest.dropFirst()
                }
            }
        }
        return label
    }

    enum State {
        static let ready = "ready"
        static let stale = "stale"
        static let unavailable = "unavailable"
    }

    /// An account that cannot be drawn: no reading, current or preserved.
    var isUnavailable: Bool { state == State.unavailable || metrics.isEmpty }

    /// Whether the account has a plan or rate limit to headline. Without
    /// one the file still lists the metrics it can draw, but none of them
    /// is the account's quota, so no percentage stands for the account.
    var hasPlanLimit: Bool { binding != nil }

    /// Data older than the current query: dimmed, with its original age.
    var isStale: Bool { state == State.stale }

    /// The identity to show, when the user asked for it and it is known.
    var shownIdentity: String? {
        guard let email, !email.isEmpty else { return nil }
        return email
    }

    struct Binding: Codable {
        let index: Int
        /// The adapter's raw label: an id, often in Portuguese. Private so
        /// no view can draw it; views use [displayLabel].
        private let label: String
        /// PRD 0034: the label's text (see [PublishedText]).
        fileprivate let labelText: PublishedText?
        let window: String?
        /// Consumed fraction in `0...1` (`1` when exhausted).
        let usedRatio: Double
        let remaining: Double
        let resetsAt: Date?
        let reason: String
        let blockedUntil: Date?

        enum CodingKeys: String, CodingKey {
            case index, label, window, remaining, reason
            case labelText = "label_text"
            case usedRatio = "used_ratio"
            case resetsAt = "resets_at"
            case blockedUntil = "blocked_until"
        }

        var isExhausted: Bool { reason == "exhausted" }

        /// For [WidgetAccount.bindingLabel], the last resort only.
        fileprivate var rawLabel: String { label }
    }

    struct Metric: Codable {
        /// The adapter's raw label and unit: ids, often in Portuguese.
        /// Private so no view can draw them; views use [displayLabel] and
        /// [displayUnit].
        private let label: String
        let usedFraction: Double
        let limit: Double
        private let unit: String
        /// PRD 0034: the label's and the unit's texts (see
        /// [PublishedText]); absent in a file written by an older app.
        fileprivate let labelText: PublishedText?
        private let unitText: PublishedText?
        let window: String?
        let resetsAt: Date?
        /// The consumption pace (PRD 0007) when the app could assess it.
        let pace: Pace?

        enum CodingKeys: String, CodingKey {
            case label, limit, unit, window, pace
            case labelText = "label_text"
            case unitText = "unit_text"
            case usedFraction = "used_fraction"
            case resetsAt = "resets_at"
        }

        var isOverused: Bool { usedFraction > 1 }

        /// What a widget's metric choice stores to find this metric again:
        /// the adapter's label, an identifier that does not change with the
        /// language. Never drawn.
        var choiceKey: String { label }

        /// The name to draw, without the trailing unit hint `(%)` (the
        /// widgets print the percentage next to it): the text in the app's
        /// language, else the English default, else the raw label.
        var displayLabel: String { WidgetText.choose([labelText], raw: label).strippingPercentHint }

        /// The unit to draw, by the same rule.
        var displayUnit: String { WidgetText.choose([unitText], raw: unit) }

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

extension Double {
    /// A used fraction safe to draw and to turn into an integer: within
    /// `0...10` (the core's own ceiling, `MAX_USED_FRACTION`), `0` when not
    /// a number. The app never writes more, but the file is only data.
    var safeFraction: Double {
        guard !isNaN else { return 0 }
        return min(max(self, 0), 10)
    }

    /// Pace points apart from the average, within `±1000` (the core's
    /// `MAX_DELTA_POINTS`), `0` when not a number.
    var safePoints: Double {
        guard !isNaN else { return 0 }
        return min(max(self, -1000), 1000)
    }
}

/// PRD 0034: a `label_text` or `unit_text` of the file. The app writes the
/// text already in its language (a string); a core `Localized` object
/// (`{"id", "params", "fallback"}`, as the binding carries) is read for its
/// English `fallback`, which is never worse than the raw label. Anything
/// else is ignored rather than making the whole file unreadable.
enum PublishedText: Decodable {
    case resolved(String)
    case fallback(String)
    case none

    private struct Object: Decodable {
        let fallback: String?
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let text = try? container.decode(String.self), !text.isEmpty {
            self = .resolved(text)
        } else if let text = (try? container.decode(Object.self))?.fallback, !text.isEmpty {
            self = .fallback(text)
        } else {
            self = .none
        }
    }

    var resolved: String? {
        if case .resolved(let text) = self { return text }
        return nil
    }

    var fallback: String? {
        if case .fallback(let text) = self { return text }
        return nil
    }
}

extension PublishedText: Encodable {
    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .resolved(let text), .fallback(let text): try container.encode(text)
        case .none: try container.encodeNil()
        }
    }
}

/// PRD 0034: the texts the widgets draw from the file.
enum WidgetText {
    /// The first text in the app's language among `texts`, else the first
    /// English default, else `raw` (the adapter's label, often Portuguese,
    /// only for a file written by an older app).
    static func choose(_ texts: [PublishedText?], raw: String) -> String {
        texts.lazy.compactMap { $0?.resolved }.first
            ?? texts.lazy.compactMap { $0?.fallback }.first
            ?? raw
    }
}

/// PRD 0034: the language the app runs in, carried by the file, so the
/// widgets speak it (their own strings and their number and date formats)
/// instead of the system's.
enum WidgetLanguage {
    /// The locale for the file's `locale` (a registry code such as `de`,
    /// `pt-BR` or `zh-CN`), or `nil` when the file has none (an older app)
    /// or names a language this extension has no strings for: the widgets
    /// then follow the system's language.
    static func locale(for code: String?, bundle: Bundle = .main) -> Locale? {
        guard let code, (1...35).contains(code.count),
              code.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-" || $0 == "_") })
        else { return nil }
        let locale = Locale(identifier: code)
        guard let language = locale.language.languageCode?.identifier else { return nil }
        let known = bundle.localizations.contains {
            Locale(identifier: $0).language.languageCode?.identifier == language
        }
        return known ? locale : nil
    }
}

extension String {
    /// `Week (7 days) (%)` → `Week (7 days)`; any other label is unchanged.
    var strippingPercentHint: String {
        guard hasSuffix("(%)") else { return self }
        return String(dropLast(3)).trimmingCharacters(in: .whitespaces)
    }
}

/// The published file. `truncated` says that accounts were left out, so a
/// widget never shows an empty list as if no account existed.
struct WidgetSnapshot: Codable {
    let schema: Int
    let generatedAt: Date
    let accounts: [WidgetAccount]
    let truncated: Bool?
    /// PRD 0034: the app's interface language (a registry code); absent in
    /// a file written by an older app. See [WidgetLanguage].
    var locale: String? = nil

    enum CodingKeys: String, CodingKey {
        case schema, accounts, truncated, locale
        case generatedAt = "generated_at"
    }

    /// The locale the widgets lay out in, `nil` for the system's.
    var language: Locale? { WidgetLanguage.locale(for: locale) }

    /// The schema this extension understands; a newer file is read as
    /// unknown rather than half-interpreted.
    static let supportedSchema = 1

    /// The largest file read. The app writes at most 256 KiB; anything far
    /// larger is not its file, and reading it could exhaust the extension's
    /// memory, so it reads as unreadable (the empty state).
    static let maxBytes = 1024 * 1024

    /// The file as it is now, `nil` when it is absent or unreadable: a
    /// widget never draws made-up numbers. [read(from:)] tells the two apart.
    static func load(from url: URL? = SnapshotLocation.url) -> WidgetSnapshot? {
        read(from: url).snapshot
    }

    /// The file as it is now: absent (widgets off, or the app never wrote
    /// it: the empty state), unreadable (a newer schema, a file this
    /// extension cannot decode or may not open: "Update Cuotai"), or ready.
    static func read(from url: URL? = SnapshotLocation.url) -> SnapshotRead {
        guard let url else { return .absent }
        guard FileManager.default.fileExists(atPath: url.path) else { return .absent }
        guard let snapshot = decode(at: url) else { return .unreadable }
        return .ready(snapshot)
    }

    private static func decode(at url: URL) -> WidgetSnapshot? {
        guard let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize,
              size <= maxBytes,
              let data = try? Data(contentsOf: url),
              data.count <= maxBytes
        else { return nil }
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

/// What [WidgetSnapshot.read(from:)] found.
enum SnapshotRead {
    case absent
    case unreadable
    case ready(WidgetSnapshot)

    var snapshot: WidgetSnapshot? {
        if case .ready(let snapshot) = self { return snapshot }
        return nil
    }

    var isUnreadable: Bool {
        if case .unreadable = self { return true }
        return false
    }
}
