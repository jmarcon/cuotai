import SwiftUI
import WidgetKit

// PRD 0014 RF3.2: the bundle with the three widgets of this version
// (Summary, Account, History). Business logic stays in `cuotai-core`: the
// extension reads the snapshot the app published and draws it.

@main
struct CuotaiWidgets: WidgetBundle {
    var body: some Widget {
        SummaryWidget()
        AccountWidget()
        HistoryWidget()
    }
}