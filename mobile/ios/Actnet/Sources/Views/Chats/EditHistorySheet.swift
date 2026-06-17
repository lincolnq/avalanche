import SwiftUI

/// Edit-history sheet for a message (docs/36-message-editing-deletion.md):
/// prior bodies oldest-first, then the current version. Reached from a
/// message's long-press menu when it has been edited at least once.
struct EditHistorySheet: View {
    let current: Message
    let revisions: [MessageRevisionFfi]

    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                ForEach(Array(revisions.enumerated()), id: \.offset) { _, rev in
                    row(body: rev.body, atMs: rev.replacedAtMs, label: "Edited")
                }
                row(
                    body: current.body,
                    atMs: current.editedAtMs ?? current.sentAtMs,
                    label: "Current"
                )
            }
            .listStyle(.plain)
            .background(Color.avPaper)
            .scrollContentBackground(.hidden)
            .navigationTitle("Edit History")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    private func row(body: String, atMs: Int64, label: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(.caption2)
                .foregroundStyle(.secondary)
            Text(body)
            Text(Date(timeIntervalSince1970: Double(atMs) / 1000), style: .relative)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .padding(.vertical, 2)
    }
}
