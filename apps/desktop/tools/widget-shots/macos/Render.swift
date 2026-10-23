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
// declared below. `ImageRenderer` also has no container for
// `containerRelativeFrame` (the usage bar's fill), which then draws nothing,
// so `render.sh` swaps that one modifier for `shotTrackFraction`: the fill
// as a fraction of the bar's own track. Everything else is the shipped code.

extension View {
    /// The usage bar's fill: `fraction` of the width the track proposes.
    func shotTrackFraction(_ fraction: Double) -> some View {
        GeometryReader { proxy in
            self.frame(width: proxy.size.width * fraction, height: proxy.size.height)
        }
    }
}

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

/// Point sizes of the macOS desktop widget families (Human Interface
/// Guidelines, "Widgets" → macOS) and the system's default content margin.
enum Chrome {
    static func size(_ family: WidgetFamily) -> CGSize {
        switch family {
        case .systemMedium: return CGSize(width: 364, height: 170)
        case .systemLarge: return CGSize(width: 364, height: 382)
        default: return CGSize(width: 170, height: 170)
        }
    }

    static let margin: CGFloat = 16
    static let radius: CGFloat = 22
}

/// What `.containerBackground(.fill.tertiary, for: .widget)` gives the
/// widget on the desktop, approximated with a solid card: outside an
/// extension the container background is not drawn.
struct WidgetCard<Content: View>: View {
    let family: WidgetFamily
    let scheme: ColorScheme
    @ViewBuilder let content: Content

    var body: some View {
        let size = Chrome.size(family)
        content
            .padding(Chrome.margin)
            // WidgetKit centres content that does not fill the widget.
            .frame(width: size.width, height: size.height)
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
    }
}

// MARK: - Shots

struct Shot {
    let name: String
    let family: WidgetFamily
    let view: AnyView
}

func shots(_ snapshot: WidgetSnapshot, now: Date) -> [Shot] {
    let summary = SummaryEntry(date: now, snapshot: snapshot, pinnedKey: nil)
    let pinned = SummaryEntry(date: now, snapshot: snapshot, pinnedKey: snapshot.accounts.first?.key)
    let gone = SummaryEntry(date: now, snapshot: snapshot, pinnedKey: "claude:Removed")
    let history = HistoryEntry(date: now, snapshot: snapshot, pinnedKey: nil)
    let empty = SummaryEntry(date: now, snapshot: nil, pinnedKey: nil)
    return [
        Shot(name: "summary-small", family: .systemSmall, view: AnyView(SummaryWidgetEntryView(entry: summary))),
        Shot(name: "summary-medium", family: .systemMedium, view: AnyView(SummaryWidgetEntryView(entry: summary))),
        Shot(name: "summary-large", family: .systemLarge, view: AnyView(SummaryWidgetEntryView(entry: summary))),
        Shot(name: "account-small", family: .systemSmall, view: AnyView(AccountWidgetEntryView(entry: pinned))),
        Shot(name: "account-medium", family: .systemMedium, view: AnyView(AccountWidgetEntryView(entry: pinned))),
        Shot(name: "account-unavailable", family: .systemSmall, view: AnyView(AccountWidgetEntryView(entry: gone))),
        Shot(name: "history-medium", family: .systemMedium, view: AnyView(HistoryWidgetEntryView(entry: history))),
        Shot(name: "empty-small", family: .systemSmall, view: AnyView(SummaryWidgetEntryView(entry: empty))),
    ]
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
        guard arguments.count == 3 else {
            FileHandle.standardError.write(Data("usage: render <widget.json> <out-dir>\n".utf8))
            exit(1)
        }
        let now = Date()
        let snapshot = try loadFixture(arguments[1], now: now)
        let out = URL(fileURLWithPath: arguments[2], isDirectory: true)
        try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

        for scheme in [ColorScheme.light, .dark] {
            let suffix = scheme == .dark ? "dark" : "light"
            for shot in shots(snapshot, now: now) {
                let card = WidgetCard(family: shot.family, scheme: scheme) { shot.view }
                    .padding(12)
                try write(card, to: out.appendingPathComponent("macos-\(shot.name)-\(suffix).png"))
            }
            try write(Gallery(snapshot: snapshot, now: now, scheme: scheme),
                      to: out.appendingPathComponent("macos-gallery-\(suffix).png"))
        }
    }
}
