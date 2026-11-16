import AppKit
import SwiftUI
import WidgetKit

// Dev-only harness (PRD 0014 docs): draws the shipped WidgetKit entry views
// into PNG files with SwiftUI's `ImageRenderer`, so the docs can show the
// widgets without a signed `.appex`. It is compiled by `render.sh` together
// with `apps/desktop/widgets/macos/Sources` (minus the `@main` bundle) and is
// never part of the app or of the widget package.
//
// Outside a widget extension `\.widgetFamily` is read-only and always the
// default, so `render.sh` rewrites that environment read to `\.shotFamily`,
// declared below. Everything else is the shipped code.

private struct ShotFamilyKey: EnvironmentKey {
    static let defaultValue: WidgetFamily = .systemSmall
}

extension EnvironmentValues {
    var shotFamily: WidgetFamily {
        get { self[ShotFamilyKey.self] }
        set { self[ShotFamilyKey.self] = newValue }
    }
}

// MARK: - Widget chrome

/// Point sizes of the macOS desktop widget families, as the system logs them
/// for a live widget on macOS 26 and 27 (`chronod`: small 164x164, medium
/// 344x164, large 344x344, corner radius 27.88). The shipped views take their
/// size from the timeline context and fall back to these same sizes.
enum Chrome {
    static func size(_ family: WidgetFamily) -> CGSize {
        switch family {
        case .systemMedium: return CGSize(width: 344, height: 164)
        case .systemLarge: return CGSize(width: 344, height: 344)
        default: return CGSize(width: 164, height: 164)
        }
    }

    /// The content margin the views fall back to outside an extension.
    static let margin: CGFloat = 16
    static let radius: CGFloat = 27.88
}

/// What `.containerBackground(.fill.tertiary, for: .widget)` gives the
/// widget on the desktop, approximated with a solid card: outside an
/// extension the container background is not drawn. The entry view brings
/// its own size and margins, as it does in the extension.
struct WidgetCard<Content: View>: View {
    let family: WidgetFamily
    let scheme: ColorScheme
    @ViewBuilder let content: Content

    var body: some View {
        content
            .background(
                RoundedRectangle(cornerRadius: Chrome.radius, style: .continuous)
                    .fill(scheme == .dark ? Color(white: 0.17) : Color(white: 0.985))
            )
            .overlay(
                RoundedRectangle(cornerRadius: Chrome.radius, style: .continuous)
                    .strokeBorder(Color.primary.opacity(scheme == .dark ? 0.12 : 0.06), lineWidth: 1)
            )
            .environment(\.shotFamily, family)
            .environment(\.colorScheme, scheme)
            // The docs are in English, whatever the machine's language.
            .environment(\.locale, Locale(identifier: "en"))
    }
}

// MARK: - Shots

struct Shot {
    let name: String
    let family: WidgetFamily
    let view: AnyView
    /// What the widget's drawing must cover (see `Coverage`).
    var fills: Coverage = .area
}

/// How much of the content area a widget's drawing must cover in `--check`.
enum Coverage {
    /// A state (no file, account gone, "Update Cuotai"): a short centred
    /// text, which only has to stay inside the margins.
    case none
    /// A list (the medium and large Summary): rows whose bars span the
    /// width, as many rows as there are accounts.
    case width
    /// One account or a chart: a bar or the chart across the width, and the
    /// content from the header down to the bottom of the widget.
    case area

    /// For a widget whose first account is `account`: a bar spans the width
    /// only for an account with a reading and a plan limit.
    static func of(_ account: WidgetAccount?, _ full: Coverage) -> Coverage {
        guard let account, account.hasPlanLimit, !account.isUnavailable else { return .none }
        return full
    }
}

func shots(_ snapshot: WidgetSnapshot, now: Date) -> [Shot] {
    let summary = SummaryEntry(date: now, snapshot: snapshot, pinnedKey: nil)
    let pinned = SummaryEntry(date: now, snapshot: snapshot, pinnedKey: snapshot.accounts.first?.key)
    let gone = SummaryEntry(date: now, snapshot: snapshot, pinnedKey: "claude:Removed")
    let history = HistoryEntry(date: now, snapshot: snapshot, pinnedKey: nil)
    let empty = SummaryEntry(date: now, snapshot: nil, pinnedKey: nil)
    let first = snapshot.accounts.first
    let listed = snapshot.accounts.contains { $0.hasPlanLimit && !$0.isUnavailable } ? Coverage.width : .none
    return [
        Shot(name: "summary-small", family: .systemSmall, view: AnyView(SummaryWidgetEntryView(entry: summary)),
             fills: .of(first, .area)),
        Shot(name: "summary-medium", family: .systemMedium, view: AnyView(SummaryWidgetEntryView(entry: summary)),
             fills: listed),
        Shot(name: "summary-large", family: .systemLarge, view: AnyView(SummaryWidgetEntryView(entry: summary)),
             fills: listed),
        Shot(name: "account-small", family: .systemSmall, view: AnyView(AccountWidgetEntryView(entry: pinned)),
             fills: .of(first, .area)),
        Shot(name: "account-medium", family: .systemMedium, view: AnyView(AccountWidgetEntryView(entry: pinned)),
             fills: .of(first, .area)),
        Shot(name: "account-unavailable", family: .systemSmall, view: AnyView(AccountWidgetEntryView(entry: gone)), fills: .none),
        Shot(name: "history-medium", family: .systemMedium, view: AnyView(HistoryWidgetEntryView(entry: history)),
             fills: (first?.history ?? []).isEmpty ? .none : .area),
        Shot(name: "empty-small", family: .systemSmall, view: AnyView(SummaryWidgetEntryView(entry: empty)), fills: .none),
    ]
}

/// `--states`: the states the docs do not show, for review rather than the
/// docs — no file at all, a pinned account that went away, and every account
/// of the fixture pinned in turn (pair it with `fixtures/widget-edge.json`
/// for an account without a plan limit and extreme readings).
func stateShots(_ snapshot: WidgetSnapshot, now: Date) -> [Shot] {
    let noFile = SummaryEntry(date: now, snapshot: nil, pinnedKey: "claude:Work")
    var result = [
        Shot(name: "state-account-no-file", family: .systemSmall,
             view: AnyView(AccountWidgetEntryView(entry: noFile)), fills: .none),
        Shot(name: "state-history-no-file", family: .systemMedium,
             view: AnyView(HistoryWidgetEntryView(entry: HistoryEntry(date: now, snapshot: nil, pinnedKey: "claude:Work"))), fills: .none),
        Shot(name: "state-history-removed", family: .systemMedium,
             view: AnyView(HistoryWidgetEntryView(entry: HistoryEntry(date: now, snapshot: snapshot, pinnedKey: "claude:Removed"))), fills: .none),
    ]
    // The placeholder of every Summary size: the providers' `placeholder`
    // has no file, so it is this state (a crash or a zero size here would
    // leave the system's grey placeholder too).
    let none = SummaryEntry(date: now, snapshot: nil, pinnedKey: nil)
    result.append(Shot(name: "state-summary-no-file-medium", family: .systemMedium,
                       view: AnyView(SummaryWidgetEntryView(entry: none)), fills: .none))
    result.append(Shot(name: "state-summary-no-file-large", family: .systemLarge,
                       view: AnyView(SummaryWidgetEntryView(entry: none)), fills: .none))
    result.append(Shot(name: "state-account-no-file-medium", family: .systemMedium,
                       view: AnyView(AccountWidgetEntryView(entry: noFile)), fills: .none))
    // A file this extension cannot read (a newer schema, say): every widget
    // says "Update Cuotai", in each of its sizes.
    let unreadable = SummaryEntry(date: now, read: .unreadable, pinnedKey: nil)
    for family in [WidgetFamily.systemSmall, .systemMedium] {
        let size = family == .systemSmall ? "small" : "medium"
        result.append(Shot(name: "state-summary-unreadable-\(size)", family: family,
                           view: AnyView(SummaryWidgetEntryView(entry: unreadable)), fills: .none))
        result.append(Shot(name: "state-account-unreadable-\(size)", family: family,
                           view: AnyView(AccountWidgetEntryView(entry: unreadable)), fills: .none))
    }
    result.append(Shot(name: "state-history-unreadable", family: .systemMedium,
                       view: AnyView(HistoryWidgetEntryView(entry: HistoryEntry(date: now, read: .unreadable, pinnedKey: nil))), fills: .none))
    for (offset, account) in snapshot.accounts.enumerated() {
        let pinned = SummaryEntry(date: now, snapshot: snapshot, pinnedKey: account.id)
        // An account without a reading or a plan limit has no bar to span
        // the widget; one without a series says "No history yet".
        result.append(Shot(name: "state-account-\(offset)-small", family: .systemSmall,
                           view: AnyView(AccountWidgetEntryView(entry: pinned)), fills: .of(account, .area)))
        result.append(Shot(name: "state-account-\(offset)-medium", family: .systemMedium,
                           view: AnyView(AccountWidgetEntryView(entry: pinned)), fills: .of(account, .area)))
        result.append(Shot(name: "state-history-\(offset)", family: .systemMedium,
                           view: AnyView(HistoryWidgetEntryView(entry: HistoryEntry(date: now, snapshot: snapshot, pinnedKey: account.id))),
                           fills: (account.history ?? []).isEmpty ? .none : .area))
    }
    return result
}

/// The docs' overview: a few widgets on a plain desktop-like gradient.
struct Gallery: View {
    let snapshot: WidgetSnapshot
    let now: Date
    let scheme: ColorScheme

    var body: some View {
        let summary = SummaryEntry(date: now, snapshot: snapshot, pinnedKey: nil)
        let pinned = SummaryEntry(date: now, snapshot: snapshot, pinnedKey: snapshot.accounts.first?.key)
        let history = HistoryEntry(date: now, snapshot: snapshot, pinnedKey: nil)
        HStack(alignment: .top, spacing: 24) {
            card(.systemLarge) { SummaryWidgetEntryView(entry: summary) }
            VStack(spacing: 42) {
                card(.systemMedium) { HistoryWidgetEntryView(entry: history) }
                HStack(spacing: 24) {
                    card(.systemSmall) { AccountWidgetEntryView(entry: pinned) }
                    card(.systemSmall) { SummaryWidgetEntryView(entry: summary) }
                }
            }
        }
        .padding(36)
        .background(
            // A flat colour, not a wallpaper: it keeps the PNG small.
            scheme == .dark ? Color(red: 0.13, green: 0.14, blue: 0.24)
                            : Color(red: 0.71, green: 0.78, blue: 0.93)
        )
        .environment(\.colorScheme, scheme)
    }

    private func card<C: View>(_ family: WidgetFamily, @ViewBuilder _ content: () -> C) -> some View {
        WidgetCard(family: family, scheme: scheme, content: content)
            // Flatten first, so the shadow falls behind the card and not
            // behind every label and bar inside it.
            .compositingGroup()
            .shadow(color: .black.opacity(0.18), radius: 10, y: 4)
    }
}

// MARK: - Fixture

/// Loads the fixture through the shipped `WidgetSnapshot.load(from:)`,
/// after moving every timestamp forward by the same amount so that
/// `generated_at` is "now": the relative ages the widgets print ("2 min")
/// then match the fixture instead of growing with the calendar.
func loadFixture(_ path: String, now: Date) throws -> WidgetSnapshot {
    let data = try Data(contentsOf: URL(fileURLWithPath: path))
    guard var root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
          let generatedRaw = root["generated_at"] as? String
    else { throw CocoaError(.fileReadCorruptFile) }
    let formatter = ISO8601DateFormatter()
    guard let generated = formatter.date(from: generatedRaw) else { throw CocoaError(.fileReadCorruptFile) }
    let shift = now.timeIntervalSince(generated)

    func move(_ value: Any) -> Any {
        if let string = value as? String, let date = formatter.date(from: string) {
            return formatter.string(from: date.addingTimeInterval(shift))
        }
        if let object = value as? [String: Any] { return object.mapValues(move) }
        if let array = value as? [Any] { return array.map(move) }
        return value
    }
    root = root.mapValues(move)

    let shifted = FileManager.default.temporaryDirectory
        .appendingPathComponent("cuotai-widget-shots-\(ProcessInfo.processInfo.processIdentifier).json")
    try JSONSerialization.data(withJSONObject: root).write(to: shifted)
    defer { try? FileManager.default.removeItem(at: shifted) }
    guard let snapshot = WidgetSnapshot.load(from: shifted) else { throw CocoaError(.fileReadCorruptFile) }
    return snapshot
}

// MARK: - Check

/// A render's pixels, RGBA with premultiplied alpha, one byte a channel.
struct Bitmap {
    let width: Int
    let height: Int
    let pixels: [UInt8]

    init?(_ image: CGImage) {
        let width = image.width, height = image.height
        self.width = width
        self.height = height
        var buffer = [UInt8](repeating: 0, count: width * height * 4)
        let drawn = buffer.withUnsafeMutableBytes { raw -> Bool in
            guard let context = CGContext(data: raw.baseAddress, width: width, height: height,
                                          bitsPerComponent: 8, bytesPerRow: width * 4,
                                          space: CGColorSpaceCreateDeviceRGB(),
                                          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
            else { return false }
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
            return true
        }
        guard drawn else { return nil }
        pixels = buffer
    }

    /// The share of pixels whose colour differs by more than a rounding
    /// error, or 1 when the sizes differ.
    func difference(from other: Bitmap) -> Double {
        guard width == other.width, height == other.height else { return 1 }
        var differing = 0
        for pixel in stride(from: 0, to: pixels.count, by: 4) {
            for channel in 0..<4 where abs(Int(pixels[pixel + channel]) - Int(other.pixels[pixel + channel])) > 24 {
                differing += 1
                break
            }
        }
        return Double(differing) / Double(max(width * height, 1))
    }

    /// The box around everything drawn (any pixel that is not transparent),
    /// in points with the origin at the top left; nil when nothing is drawn.
    var drawnBox: CGRect? {
        var minX = width, minY = height, maxX = -1, maxY = -1
        for y in 0..<height {
            for x in 0..<width where pixels[(y * width + x) * 4 + 3] > 4 {
                minX = min(minX, x); maxX = max(maxX, x)
                minY = min(minY, y); maxY = max(maxY, y)
            }
        }
        guard maxX >= 0 else { return nil }
        // A bitmap context keeps the top row first.
        return CGRect(x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1)
    }
}

/// `--check`: the guard CI runs (PRD 0014 RF3). Each argument is
/// `<expected>:<widget.json>`, where `expected` is what the shipped
/// `WidgetSnapshot.read(from:)` must find: `ready` (and then every account
/// of the file decoded) or `unreadable` (a newer schema).
///
/// Then every widget, in each size and both schemes, is drawn bare (no
/// card, so only what the widget draws is opaque) under the widget's own
/// size and under the unspecified and unbounded proposals macOS 27.2 lays
/// widgets out with, and the check asks what was drawn, not only that
/// nothing stopped the process (0.86.4 passed that and still drew empty,
/// collapsed or crashed widgets on the desktop):
/// - every render is the widget's own size, and the same picture under
///   every proposal (a widget that depends on the proposal collapses or
///   overflows on the desktop);
/// - what is drawn stays inside the content margins;
/// - a widget with data covers the content area (bars and charts span its
///   width, the content reaches down most of its height) instead of a
///   thin column or a corner.
/// A layout SwiftUI rejects still stops this process, which fails the check.
@MainActor
func check(_ arguments: ArraySlice<String>) throws {
    var failures: [String] = []
    var renders = 0
    for argument in arguments {
        guard let colon = argument.firstIndex(of: ":") else {
            failures.append("\(argument): expected <ready|unreadable>:<path>")
            continue
        }
        let expected = String(argument[..<colon])
        let path = String(argument[argument.index(after: colon)...])
        let url = URL(fileURLWithPath: path)
        let read = WidgetSnapshot.read(from: url)
        switch (expected, read) {
        case ("ready", .ready(let snapshot)):
            let raw = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
            let published = (raw?["accounts"] as? [Any])?.count ?? -1
            if snapshot.accounts.count != published {
                failures.append("\(path): decoded \(snapshot.accounts.count) of \(published) accounts")
            }
        case ("unreadable", .unreadable):
            break
        default:
            failures.append("\(path): expected \(expected), read \(read)")
            continue
        }
        let now = Date()
        let snapshot = read.snapshot.map { _ in try? loadFixture(path, now: now) } ?? nil
        var all = snapshot.map { shots($0, now: now) + stateShots($0, now: now) } ?? []
        all += stateShots(WidgetSnapshot(schema: 1, generatedAt: now, accounts: [], truncated: false), now: now)
        let file = URL(fileURLWithPath: path).deletingLastPathComponent().lastPathComponent
            + "/" + URL(fileURLWithPath: path).lastPathComponent
        for shot in all {
            let size = Chrome.size(shot.family)
            let proposals: [(String, ProposedViewSize)] = [
                ("unspecified", .unspecified),
                ("unbounded width", ProposedViewSize(width: .infinity, height: size.height)),
                ("unbounded height", ProposedViewSize(width: size.width, height: .infinity)),
                ("unbounded", .infinity),
            ]
            for scheme in [ColorScheme.light, .dark] {
                let label = "\(file): \(shot.name) (\(scheme == .dark ? "dark" : "light"))"
                func draw(_ proposal: ProposedViewSize) -> Bitmap? {
                    let renderer = ImageRenderer(content: shot.view
                        .environment(\.shotFamily, shot.family)
                        .environment(\.colorScheme, scheme)
                        .environment(\.locale, Locale(identifier: "en")))
                    renderer.proposedSize = proposal
                    renderer.scale = 1
                    renders += 1
                    return renderer.cgImage.flatMap(Bitmap.init)
                }
                guard let reference = draw(ProposedViewSize(size)) else {
                    failures.append("\(label): drew nothing at its own size")
                    continue
                }
                if reference.width != Int(size.width) || reference.height != Int(size.height) {
                    failures.append("\(label): drawn \(reference.width)x\(reference.height), not the widget's \(Int(size.width))x\(Int(size.height))")
                }
                for (name, proposal) in proposals {
                    guard let other = draw(proposal) else {
                        failures.append("\(label): drew nothing under the \(name) proposal")
                        continue
                    }
                    let difference = other.difference(from: reference)
                    if difference > 0.002 {
                        failures.append("\(label): under the \(name) proposal drawn \(other.width)x\(other.height), \(Int((difference * 100).rounded()))% of it unlike the widget's own size")
                    }
                }
                guard let box = reference.drawnBox else {
                    failures.append("\(label): blank")
                    continue
                }
                let content = CGRect(origin: .zero, size: size).insetBy(dx: Chrome.margin, dy: Chrome.margin)
                if !content.insetBy(dx: -1.5, dy: -1.5).contains(box) {
                    failures.append("\(label): drawing \(box) leaves the content area \(content)")
                }
                let narrow = shot.fills != .none && box.width < content.width * 0.9
                let short = shot.fills == .area && box.height < content.height * 0.75
                if narrow || short {
                    failures.append("\(label): collapsed, drawing \(box) covers too little of the content area \(content)")
                }
            }
        }
    }
    // The size comes from the timeline context, not from the family table:
    // a context size the table does not know (another macOS) is honoured
    // under every proposal, and a zero or absurd one falls back to the table.
    let now = Date()
    for family in [WidgetFamily.systemSmall, .systemMedium, .systemLarge] {
        let base = Chrome.size(family)
        for (given, expected) in [(CGSize(width: base.width + 6, height: base.height + 6), CGSize(width: base.width + 6, height: base.height + 6)),
                                  (CGSize.zero, base),
                                  (CGSize(width: CGFloat.infinity, height: base.height), base)] {
            let entry = SummaryEntry(date: now, snapshot: nil, pinnedKey: nil, displaySize: given)
            for proposal in [ProposedViewSize(base), .unspecified, .infinity] {
                let renderer = ImageRenderer(content: SummaryWidgetEntryView(entry: entry)
                    .environment(\.shotFamily, family))
                renderer.proposedSize = proposal
                renderer.scale = 1
                renders += 1
                let image = renderer.cgImage
                if image?.width != Int(expected.width) || image?.height != Int(expected.height) {
                    failures.append("display size \(given) (\(family)): drawn \(image.map { "\($0.width)x\($0.height)" } ?? "nothing") under \(proposal), expected \(Int(expected.width))x\(Int(expected.height))")
                }
            }
        }
    }
    if failures.isEmpty {
        print("widget check: \(arguments.count) file(s), \(renders) renders, every widget drawn at its size, the same under every proposal")
    } else {
        FileHandle.standardError.write(Data(failures.map { "widget check: \($0)\n" }.joined().utf8))
        FileHandle.standardError.write(Data("widget check: \(failures.count) failure(s) in \(renders) renders\n".utf8))
        exit(1)
    }
}

// MARK: - Proposals

/// `--proposals`: for review, each widget drawn under its own size and under
/// the unspecified and unbounded proposals, each on a card of the widget's
/// size (centred and clipped, as the desktop shows content that does not
/// match the widget), so a reviewer sees what a proposal does to it.
@MainActor
func writeProposals(_ snapshot: WidgetSnapshot, now: Date, to out: URL) throws {
    let proposals: [(String, (CGSize) -> ProposedViewSize)] = [
        ("own-size", { ProposedViewSize($0) }),
        ("unspecified", { _ in .unspecified }),
        ("unbounded-width", { ProposedViewSize(width: .infinity, height: $0.height) }),
        ("unbounded", { _ in .infinity }),
    ]
    for scheme in [ColorScheme.light, .dark] {
        let suffix = scheme == .dark ? "dark" : "light"
        for shot in shots(snapshot, now: now) {
            let size = Chrome.size(shot.family)
            for (name, proposal) in proposals {
                let renderer = ImageRenderer(content: shot.view
                    .environment(\.shotFamily, shot.family)
                    .environment(\.colorScheme, scheme)
                    .environment(\.locale, Locale(identifier: "en")))
                renderer.proposedSize = proposal(size)
                renderer.scale = 2
                let drawn = renderer.cgImage
                let card = ZStack {
                    RoundedRectangle(cornerRadius: Chrome.radius, style: .continuous)
                        .fill(scheme == .dark ? Color(white: 0.17) : Color(white: 0.985))
                    if let drawn {
                        Image(decorative: drawn, scale: 2)
                    }
                }
                .frame(width: size.width, height: size.height)
                .clipped()
                .clipShape(RoundedRectangle(cornerRadius: Chrome.radius, style: .continuous))
                .padding(12)
                .background(scheme == .dark ? Color(red: 0.13, green: 0.14, blue: 0.24)
                                            : Color(red: 0.71, green: 0.78, blue: 0.93))
                .environment(\.colorScheme, scheme)
                try write(card, to: out.appendingPathComponent("macos-\(shot.name)-\(name)-\(suffix).png"))
            }
        }
    }
}

// MARK: - Output

@MainActor
func write<V: View>(_ view: V, to url: URL) throws {
    let renderer = ImageRenderer(content: view)
    renderer.scale = 2
    guard let image = renderer.cgImage else { throw CocoaError(.fileWriteUnknown) }
    let rep = NSBitmapImageRep(cgImage: image)
    guard let png = rep.representation(using: .png, properties: [:]) else { throw CocoaError(.fileWriteUnknown) }
    try png.write(to: url)
    print("wrote \(url.path) (\(image.width)x\(image.height), \(png.count / 1024) KiB)")
}

@main
struct RenderWidgetShots {
    @MainActor
    static func main() throws {
        let arguments = CommandLine.arguments
        if arguments.count >= 3 && arguments[1] == "--check" {
            try check(arguments.dropFirst(2))
            return
        }
        if arguments.count == 4 && arguments[1] == "--proposals" {
            let out = URL(fileURLWithPath: arguments[3], isDirectory: true)
            try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
            try writeProposals(try loadFixture(arguments[2], now: Date()), now: Date(), to: out)
            return
        }
        guard arguments.count == 3 || (arguments.count == 4 && arguments[3] == "--states") else {
            FileHandle.standardError.write(Data("usage: render <widget.json> <out-dir> [--states] | render --check <ready|unreadable>:<widget.json>... | render --proposals <widget.json> <out-dir>\n".utf8))
            exit(1)
        }
        let states = arguments.count == 4
        let now = Date()
        let snapshot = try loadFixture(arguments[1], now: now)
        let out = URL(fileURLWithPath: arguments[2], isDirectory: true)
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

        for scheme in [ColorScheme.light, .dark] {
            let suffix = scheme == .dark ? "dark" : "light"
            for shot in shots(snapshot, now: now) + (states ? stateShots(snapshot, now: now) : []) {
                let card = WidgetCard(family: shot.family, scheme: scheme) { shot.view }
                    .padding(12)
                try write(card, to: out.appendingPathComponent("macos-\(shot.name)-\(suffix).png"))
            }
            try write(Gallery(snapshot: snapshot, now: now, scheme: scheme),
                      to: out.appendingPathComponent("macos-gallery-\(suffix).png"))
        }
    }
}
