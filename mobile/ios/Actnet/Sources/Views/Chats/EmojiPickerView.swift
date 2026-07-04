import SwiftUI

/// The full emoji picker sheet (docs/33) opened from the reaction bar's "⊕"
/// button. A categorized, scrollable grid with a search field and a category tab
/// strip. Selecting an emoji calls `onPick` and dismisses. System emoji only —
/// no custom/uploaded emoji (docs/33 "not doing").
struct EmojiPickerView: View {
    var onPick: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    /// Category the tab strip should scroll to; nil until a tab is tapped.
    @State private var scrollTarget: EmojiCategory?
    /// Recently-used emoji, loaded once when the sheet opens.
    @State private var recents: [String] = []

    private let columns = [GridItem(.adaptive(minimum: 44), spacing: 4)]

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                searchField
                if query.isEmpty {
                    if !recents.isEmpty { recentsRow }
                    categoryStrip
                    Divider()
                }
                grid
            }
            .background(Color.avPaper)
            .navigationTitle("React")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .onAppear { recents = EmojiRecents.all() }
    }

    /// A single horizontal row of the most-recently-used emoji.
    private var recentsRow: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("Recently Used")
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12)
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 4) {
                    ForEach(recents, id: \.self) { e in
                        emojiCell(e)
                    }
                }
                .padding(.horizontal, 8)
            }
        }
        .padding(.bottom, 4)
    }

    private var searchField: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField("Search emoji", text: $query)
                .textFieldStyle(.plain)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
            if !query.isEmpty {
                Button { query = "" } label: {
                    Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(Capsule().fill(Color.avCard))
        .padding(.horizontal)
        .padding(.vertical, 8)
    }

    private var categoryStrip: some View {
        HStack(spacing: 0) {
            ForEach(EmojiCategory.allCases) { cat in
                Button { scrollTarget = cat } label: {
                    Image(systemName: cat.symbol)
                        .font(.body)
                        .foregroundStyle(scrollTarget == cat ? Color.avBrand : Color.avMuted)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 8)
    }

    @ViewBuilder
    private var grid: some View {
        if query.isEmpty {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8, pinnedViews: [.sectionHeaders]) {
                        ForEach(EmojiData.byCategory, id: \.0) { cat, emoji in
                            Section {
                                LazyVGrid(columns: columns, spacing: 4) {
                                    ForEach(emoji, id: \.self) { e in
                                        emojiCell(e)
                                    }
                                }
                                .padding(.horizontal, 8)
                            } header: {
                                Text(cat.rawValue)
                                    .font(.caption)
                                    .fontWeight(.semibold)
                                    .foregroundStyle(.secondary)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(.horizontal, 12)
                                    .padding(.vertical, 4)
                                    .background(Color.avPaper)
                                    .id(cat)
                            }
                        }
                    }
                    .padding(.bottom, 12)
                }
                .onChange(of: scrollTarget) { _, target in
                    guard let target else { return }
                    withAnimation { proxy.scrollTo(target, anchor: .top) }
                }
            }
        } else {
            let results = EmojiData.search(query)
            if results.isEmpty {
                ContentUnavailableView("No emoji", systemImage: "magnifyingglass")
            } else {
                ScrollView {
                    LazyVGrid(columns: columns, spacing: 4) {
                        ForEach(results, id: \.self) { e in
                            emojiCell(e)
                        }
                    }
                    .padding(.horizontal, 8)
                    .padding(.top, 4)
                }
            }
        }
    }

    private func emojiCell(_ emoji: String) -> some View {
        Button {
            EmojiRecents.record(emoji)
            onPick(emoji)
            dismiss()
        } label: {
            Text(emoji)
                .font(.system(size: 30))
                .frame(width: 44, height: 44)
        }
        .buttonStyle(.plain)
    }
}

#if DEBUG
#Preview {
    EmojiPickerView(onPick: { _ in })
}
#endif
