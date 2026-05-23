import SwiftUI

struct CallsView: View {
    var body: some View {
        NavigationStack {
            ContentUnavailableView(
                "No calls yet",
                systemImage: "phone",
                description: Text("Voice and video calls will appear here.")
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color.avPaper)
            .navigationTitle("Calls")
        }
    }
}
