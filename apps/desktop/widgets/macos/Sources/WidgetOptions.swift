import AppIntents
import SwiftUI
import WidgetKit

// PRD 0014 RF3.2 (owner request of 2026-11-17): what each widget lets the
// user choose — which accounts and which quota it shows, in which order,
// whether it shows what is left or what was used, how much detail, and
// whether the age and the reset time appear. The choices only select and
// arrange what the app published: the extension still computes no usage.
// Every default reproduces the widget as it was, minus its defects.

// MARK: - Choices

/// Whether a share reads as what is left (DS-ID-5, the default everywhere
/// in the app) or as what was used.
enum ShareDisplay: String, AppEnum {
    case remaining
    case used

    static var typeDisplayRepresentation: TypeDisplayRepresentation = "Show"
    static var caseDisplayRepresentations: [ShareDisplay: DisplayRepresentation] = [
        .remaining: "What is left",
        .used: "What was used",
    ]
}

/// How much a widget says about each account.
enum WidgetDetail: String, AppEnum {
    case detailed
    case compact

    static var typeDisplayRepresentation: TypeDisplayRepresentation = "Detail"
    static var caseDisplayRepresentations: [WidgetDetail: DisplayRepresentation] = [
        .detailed: "Detailed",
        .compact: "Compact",
    ]
}

/// The order of the Summary's accounts.
enum SummaryOrder: String, AppEnum {
    case mostRestrictive
    case configuration

    static var typeDisplayRepresentation: TypeDisplayRepresentation = "Order"
    static var caseDisplayRepresentations: [SummaryOrder: DisplayRepresentation] = [
        .mostRestrictive: "Most restrictive first",
        .configuration: "As in Cuotai",
    ]
}

/// How far back the History chart reaches (the file keeps at most what the
/// app published).
enum HistoryPeriod: String, AppEnum {
    case all
    case hours6
    case hours24
    case days7

    static var typeDisplayRepresentation: TypeDisplayRepresentation = "Period"
    static var caseDisplayRepresentations: [HistoryPeriod: DisplayRepresentation] = [
        .all: "Everything published",
        .hours6: "Last 6 hours",
        .hours24: "Last 24 hours",
        .days7: "Last 7 days",
    ]

    /// The span, `nil` for everything the file has.
    var span: TimeInterval? {
        switch self {
        case .all: return nil
        case .hours6: return 6 * 3600
        case .hours24: return 24 * 3600
        case .days7: return 7 * 86400
        }
    }
}

/// The display choices every widget view reads from the environment, so
/// the views below the entry need no extra arguments.
struct WidgetDisplay {
    var share: ShareDisplay = .remaining
    var detail: WidgetDetail = .detailed
    var showAge = true
    var showReset = true
    /// The entry's date: the instant the reset time is counted from.
    var now = Date()
}

private struct WidgetDisplayKey: EnvironmentKey {
    static let defaultValue = WidgetDisplay()
}

extension EnvironmentValues {
    var widgetDisplay: WidgetDisplay {
        get { self[WidgetDisplayKey.self] }
        set { self[WidgetDisplayKey.self] = newValue }
    }
}

/// What narrows and orders the accounts of a widget.
struct AccountSelection {
    /// The chosen accounts (ids, or legacy keys); empty for all of them.
    var accounts: [String] = []
    var order: SummaryOrder = .mostRestrictive
    var hideUnused = false
    var hideWithoutData = false
    /// The metric the widget headlines (`Metric.choiceKey`); `nil` for the
    /// binding one.
    var metric: String? = nil
}

extension WidgetSnapshot {
    /// The accounts a widget shows under `selection`, in its order. The
    /// file's own order is the most restrictive first (RF1.2), which the
    /// default order keeps; `position` gives the configuration order (a
    /// file without it keeps the file's).
    func accounts(_ selection: AccountSelection) -> [WidgetAccount] {
        var list = accounts.enumerated().map { (offset: $0.offset, account: $0.element) }
        if !selection.accounts.isEmpty {
            list = list.filter { item in selection.accounts.contains { item.account.isPinned($0) } }
        }
        if selection.hideWithoutData {
            list = list.filter { !$0.account.isUnavailable && $0.account.headline(metric: selection.metric) != nil }
        }
        if selection.hideUnused {
            list = list.filter { item in
                guard !item.account.isUnavailable, let headline = item.account.headline(metric: selection.metric)
                else { return true }
                return !headline.share.isUnused
            }
        }
        if selection.order == .configuration {
            list.sort { ($0.account.position ?? Int.max, $0.offset) < ($1.account.position ?? Int.max, $1.offset) }
        }
        return list.map(\.account)
    }
}

// MARK: - Metric picker

/// One quota of an account, for the Account widget's "Quota" choice. Its
/// id is the metric's `choiceKey` (the adapter's label, which does not
/// change with the language); [binding] stands for "the most restrictive".
struct MetricEntity: AppEntity, Identifiable {
    static let typeDisplayRepresentation = TypeDisplayRepresentation(name: "Quota")
    static let defaultQuery = MetricQuery()
    static let bindingID = "cuotai.binding"
    static let binding = MetricEntity(id: bindingID, name: nil)

    let id: String
    /// The quota's name in the app's language; `nil` for [binding].
    let name: String?

    var displayRepresentation: DisplayRepresentation {
        if let name { return DisplayRepresentation(title: "\(name)") }
        return DisplayRepresentation(title: "Most restrictive quota")
    }

    /// The `choiceKey` to headline, `nil` for the binding one.
    var metricKey: String? { id == Self.bindingID ? nil : id }

    /// The quotas of `account` (any published account's when it is `nil`),
    /// after the binding choice, each once.
    static func all(for account: AccountEntity?) -> [MetricEntity] {
        let accounts = WidgetSnapshot.load()?.accounts ?? []
        let chosen = account.map { pick in accounts.filter { $0.isPinned(pick.id) || $0.isPinned(pick.key) } } ?? accounts
        var seen = Set<String>()
        var result = [binding]
        for owner in chosen {
            for metric in owner.metrics where seen.insert(metric.choiceKey).inserted {
                result.append(MetricEntity(id: metric.choiceKey, name: owner.shortLabel(metric.displayLabel)))
            }
        }
        return result
    }
}

struct MetricQuery: EntityQuery {
    @IntentParameterDependency<AccountIntent>(\.$account)
    var intent

    func entities(for identifiers: [String]) async throws -> [MetricEntity] {
        let known = MetricEntity.all(for: nil)
        return identifiers.map { id in known.first { $0.id == id } ?? MetricEntity(id: id, name: nil) }
    }

    func suggestedEntities() async throws -> [MetricEntity] {
        MetricEntity.all(for: intent?.account)
    }

    func defaultResult() async -> MetricEntity? {
        MetricEntity.binding
    }
}

// MARK: - Intents

/// The Summary's choices.
struct SummaryIntent: WidgetConfigurationIntent {
    static var title: LocalizedStringResource = "Summary"
    static var description = IntentDescription("Which accounts the summary shows, and how.")

    @Parameter(title: "Accounts", description: "None chosen: every account Cuotai publishes.")
    var accounts: [AccountEntity]?

    @Parameter(title: "Order", default: .mostRestrictive)
    var order: SummaryOrder

    @Parameter(title: "Show", default: .remaining)
    var share: ShareDisplay

    @Parameter(title: "Detail", default: .detailed)
    var detail: WidgetDetail

    @Parameter(title: "Hide accounts with nothing used", default: false)
    var hideUnused: Bool

    @Parameter(title: "Hide accounts without data", default: false)
    var hideWithoutData: Bool

    @Parameter(title: "Show the reset time", default: true)
    var showReset: Bool

    @Parameter(title: "Show the age of the data", default: true)
    var showAge: Bool

    var selection: AccountSelection {
        AccountSelection(accounts: (accounts ?? []).map(\.id), order: order,
                         hideUnused: hideUnused, hideWithoutData: hideWithoutData)
    }

    var display: WidgetDisplay {
        WidgetDisplay(share: share, detail: detail, showAge: showAge, showReset: showReset)
    }
}

/// The History widget's choices.
struct HistoryIntent: WidgetConfigurationIntent {
    static var title: LocalizedStringResource = "History"
    static var description = IntentDescription("Which account the chart follows, and how far back.")

    @Parameter(title: "Account")
    var account: AccountEntity?

    @Parameter(title: "Period", default: .all)
    var period: HistoryPeriod

    @Parameter(title: "Show", default: .remaining)
    var share: ShareDisplay

    @Parameter(title: "Detail", default: .detailed)
    var detail: WidgetDetail

    @Parameter(title: "Show the reset time", default: true)
    var showReset: Bool

    @Parameter(title: "Show the age of the data", default: true)
    var showAge: Bool

    var display: WidgetDisplay {
        WidgetDisplay(share: share, detail: detail, showAge: showAge, showReset: showReset)
    }
}

// MARK: - Timeline

enum WidgetTimeline {
    /// The entries of one read of the file: one now and one every five
    /// minutes for the next quarter of an hour, so the reset time counts
    /// down; the system asks again after that (RF4.2: it decides when).
    static func dates(from now: Date = Date()) -> [Date] {
        (0..<4).map { now.addingTimeInterval(Double($0) * 300) }
    }

    static func policy(from now: Date = Date()) -> TimelineReloadPolicy {
        .after(now.addingTimeInterval(15 * 60))
    }

    /// Every widget kind reads the file on its own schedule, so two of them
    /// could show readings of different refreshes side by side. The first
    /// widget to read a newer file asks the others to read it too; the
    /// ones that then find the same file ask nothing.
    static func announce(_ read: SnapshotRead) {
        guard let generated = read.snapshot?.generatedAt.timeIntervalSince1970 else { return }
        let defaults = UserDefaults.standard
        let key = "cuotai.widget.generatedAt"
        let seen = defaults.double(forKey: key)
        guard generated > seen else { return }
        defaults.set(generated, forKey: key)
        if seen > 0 {
            WidgetCenter.shared.reloadAllTimelines()
        }
    }
}
