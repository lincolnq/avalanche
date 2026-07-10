import SwiftUI
import PhotosUI

struct ConversationView: View {
    @EnvironmentObject var appState: AppState
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.dismiss) private var dismiss
    let conversation: Conversation

    /// The live row from `appState` so request/blocked state stays reactive
    /// after an Accept / Block / Report action; falls back to the passed-in
    /// value (e.g. previews) when not in the list.
    private var liveConv: Conversation {
        appState.conversations.first { $0.id == conversation.id } ?? conversation
    }

    @State private var messageText = ""
    @State private var errorMessage: String?
    @State private var scrollPosition = ScrollPosition(idType: Int64.self)
    /// Non-nil while editing an existing message (docs/36); the composer turns
    /// into an edit bar prefilled with its body.
    @State private var editingMessage: Message?
    /// The message whose edit-history sheet is showing, plus its loaded revisions.
    @State private var historyMessage: Message?
    @State private var historyRevisions: [MessageRevisionFfi] = []
    /// Whether we're still a member of this group (docs/53 §Leave). Non-members
    /// keep the readable transcript but lose the composer. Always true for DMs.
    /// Loaded on appear and after leaving.
    @State private var isGroupMember = true
    /// Photo picker selection (docs/35). On pick we *stage* the image in the
    /// composer (below) rather than sending immediately.
    @State private var photoItem: PhotosPickerItem?
    /// A staged image attachment waiting in the composer: the raw bytes (sent on
    /// Send) plus a small decoded thumbnail for the inline chip. `nil` = none.
    @State private var stagedImageData: Data?
    @State private var stagedImageThumb: UIImage?
    /// A staged link preview (docs/35) generated as you type a URL, shown as a
    /// card in the composer until you send or remove it.
    @State private var stagedPreview: LinkPreviewFfi?
    /// The URL `stagedPreview` was built for, to avoid re-fetching the same one.
    @State private var stagedPreviewURL: String?
    /// A URL the user explicitly removed (x'd) — suppresses auto re-adding its
    /// preview while that URL is still in the text.
    @State private var dismissedPreviewURL: String?
    /// In-flight debounced preview fetch, cancelled on each keystroke.
    @State private var previewTask: Task<Void, Never>?
    /// Whether the clipboard currently holds an image, gating the paste button's
    /// visibility (docs/35). Refreshed on appear, app-active, and pasteboard change.
    @State private var canPasteImage = false
    /// A staged shared contact card (docs/35), pasted from a "Copy contact"
    /// action, shown as a chip in the composer until you send or remove it.
    @State private var stagedContact: SharedContactFfi?
    /// Whether the clipboard holds a contact card, gating the paste button.
    @State private var canPasteContact = false
    /// DIDs already curated in the contact book (docs/35): a received contact
    /// card shows "Saved" for these, and an active Save button otherwise.
    /// Loaded on appear and updated optimistically when the user taps Save.
    @State private var savedContactDids: Set<String> = []
    /// The message under the Signal-style long-press actions overlay (docs/33),
    /// with its on-screen frame + resolved sender name so the overlay can lift
    /// the exact bubble to center. Nil when no overlay is showing.
    @State private var actionTarget: ActionTarget?
    /// Whether the full emoji picker sheet is up (opened from the overlay's "+").
    /// Keeps `actionTarget` alive underneath so the picked emoji has a target.
    @State private var showEmojiPicker = false
    /// The image attachment tapped to open the fullscreen viewer (docs/35). The
    /// viewer pages through every image in the conversation starting here.
    @State private var imageViewerStart: ImageViewerStart?

    /// Identifiable wrapper so `.fullScreenCover(item:)` can key on the tapped
    /// image's attachment id.
    private struct ImageViewerStart: Identifiable { let id: String }

    /// A message plus the context the actions overlay needs to reproduce and
    /// animate its bubble (docs/33).
    private struct ActionTarget {
        let message: Message
        /// Source bubble content frame in global coords (animation start point).
        let frame: CGRect
        /// The name shown above this bubble in the timeline, or nil.
        let senderName: String?
        /// Whether the source bubble showed its timestamp/delivery line — the
        /// overlay copy must match so its size/wrapping is identical.
        let isLastInRun: Bool
    }

    private var messages: [Message] {
        appState.messagesByConversation[conversation.id] ?? []
    }

    /// Every image attachment in the conversation, in timeline order (message
    /// order, then attachment order within a message) — the set the fullscreen
    /// viewer pages through (docs/35).
    private var conversationImages: [AttachmentFfi] {
        messages
            .flatMap(\.attachments)
            .filter { $0.contentType.hasPrefix("image/") }
    }

    /// Reactions/editing/deletion ride the `ContentMessage` envelope, which now
    /// wraps group content too — so the long-press actions work in both DMs and
    /// groups.
    private var actionsEnabled: Bool { true }

    /// Human edit/delete-for-everyone window (docs/36): 24h from send.
    private static let editWindowMs: Int64 = 24 * 60 * 60 * 1000

    private func canEdit(_ message: Message) -> Bool {
        message.senderAccountId == conversation.accountId
            && !message.isDeleted
            && (Int64(Date().timeIntervalSince1970 * 1000) - message.sentAtMs) <= Self.editWindowMs
    }

    /// Whether an incoming message's sender is a bot, for the octagon-ish
    /// bubble shape (docs/54-bot-presentation.md). Own messages are never bots.
    private func isBotSender(_ message: Message) -> Bool {
        message.senderAccountId != conversation.accountId
            && appState.isBot(message.senderAccountId, accountId: conversation.accountId)
    }

    /// Sender name to show above a group message bubble (Signal-style), or nil.
    /// Only for incoming group messages, and only on the first message of a
    /// consecutive run from the same sender (a system event also breaks a run).
    private func senderName(for message: Message, at index: Int) -> String? {
        guard conversation.isGroup,
              message.senderAccountId != conversation.accountId,
              !message.isSystemEvent else { return nil }
        if index > 0 {
            let prev = messages[index - 1]
            if !prev.isSystemEvent && prev.senderAccountId == message.senderAccountId {
                return nil
            }
        }
        return appState.resolvedName(for: message.senderAccountId, accountId: conversation.accountId)
    }

    /// Whether the message at `index` is the last of a consecutive run from the
    /// same sender — used to collapse the timestamp/delivery line to the last
    /// bubble of a run. A following system event, a different next sender, or
    /// being the final message all end the run.
    private func isLastInRun(at index: Int) -> Bool {
        guard index < messages.count - 1 else { return true }
        let next = messages[index + 1]
        if next.isSystemEvent { return true }
        return next.senderAccountId != messages[index].senderAccountId
    }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(Array(messages.enumerated()), id: \.element.id) { index, message in
                        if message.isSystemEvent {
                            // Group membership/metadata event (docs/03 §3.6) —
                            // a centered grey line, not a chat bubble.
                            GroupSystemEventRow(
                                text: appState.groupEventText(message, accountId: conversation.accountId)
                            )
                            .id(message.sentAtMs)
                        } else {
                            MessageBubble(
                                message: message,
                                isMe: message.senderAccountId == conversation.accountId,
                                isBot: isBotSender(message),
                                senderName: senderName(for: message, at: index),
                                isLastInRun: isLastInRun(at: index),
                                reactions: appState.reactions(for: message),
                                myDid: conversation.accountId,
                                actionsEnabled: actionsEnabled,
                                onToggleReaction: { emoji in
                                    appState.toggleReaction(message: message, emoji: emoji, conversation: conversation)
                                },
                                onLongPress: { frame in
                                    actionTarget = ActionTarget(
                                        message: message,
                                        frame: frame,
                                        senderName: senderName(for: message, at: index),
                                        isLastInRun: isLastInRun(at: index)
                                    )
                                },
                                attachmentLoader: { att in
                                    await appState.attachmentData(att, accountId: conversation.accountId)
                                },
                                onImageTap: { att in
                                    imageViewerStart = ImageViewerStart(id: att.id)
                                },
                                onSaveContact: { contact in
                                    // Optimistically mark curated so the card flips
                                    // to "Saved" immediately, then persist.
                                    savedContactDids.insert(contact.did)
                                    Task { await appState.saveSharedContact(contact, accountId: conversation.accountId) }
                                },
                                onMessageContact: { contact in
                                    // Open (or create) the DM with this person and
                                    // navigate to it, matching the group member menu.
                                    let conv = appState.findOrCreateDMConversation(
                                        recipientDid: contact.did, accountId: conversation.accountId
                                    )
                                    appState.selectedTab = .chats
                                    appState.navigateToConversation = conv
                                },
                                onCopyContact: { contact in
                                    ContactPasteboard.write(did: contact.did, name: contact.name)
                                },
                                savedContactDids: savedContactDids
                            )
                            .id(message.sentAtMs)
                        }
                    }
                }
                .padding()
            }
            .defaultScrollAnchor(.bottom)
            .scrollPosition($scrollPosition)
            // Swipe down on the thread to interactively "catch" and drag the
            // keyboard away with your finger (iMessage-style).
            .scrollDismissesKeyboard(.interactively)
            .onScrollTargetVisibilityChange(idType: Int64.self) { visibleIDs in
                guard scenePhase == .active, let threshold = visibleIDs.last else { return }
                appState.markMessagesReadUpTo(
                    sentAtMs: threshold,
                    conversationId: conversation.id,
                    accountId: conversation.accountId
                )
            }
            .onChange(of: messages.count) {
                guard !messages.isEmpty else { return }
                scrollPosition.scrollTo(edge: .bottom)
                appState.markAllMessagesRead(conversationId: conversation.id, accountId: conversation.accountId)
            }

            if let error = errorMessage {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(Color.avError)
                    .padding(.horizontal)
            }

            Divider()

            // Bottom bar: a blocked DM shows an unblock prompt, an un-accepted
            // request shows the Accept/Delete/Report gate (docs/12 §1), and an
            // accepted DM or group shows the normal composer.
            if liveConv.isBlocked, let did = liveConv.recipientDid {
                blockedBar(did: did)
            } else if liveConv.isRequest, let did = liveConv.recipientDid {
                messageRequestGate(did: did)
            } else if conversation.isGroup && !isGroupMember {
                leftGroupBar
            } else {
                composer
            }
        }
        .background(Color.avPaper)
        .navigationTitle(conversation.title)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            // For groups, the centered title + avatar is a tappable link into
            // the group detail screen. (DMs keep the plain navigationTitle.)
            if conversation.isGroup, let groupId = conversation.groupId {
                ToolbarItem(placement: .principal) {
                    NavigationLink {
                        GroupDetailView(groupId: groupId, accountId: conversation.accountId)
                    } label: {
                        HStack(spacing: 8) {
                            ContactAvatar(name: conversation.title, size: 28)
                            Text(conversation.title)
                                .font(.headline)
                                .foregroundStyle(.primary)
                        }
                    }
                }
            }
        }
        .sheet(item: $historyMessage) { msg in
            EditHistorySheet(current: msg, revisions: historyRevisions)
        }
        .fullScreenCover(item: $imageViewerStart) { start in
            ImageViewerView(
                images: conversationImages,
                startId: start.id,
                loader: { att in await appState.attachmentData(att, accountId: conversation.accountId) }
            )
            .presentationBackground(.clear)
        }
        .overlay {
            if let target = actionTarget {
                let msg = target.message
                MessageActionsOverlay(
                    message: msg,
                    isMe: msg.senderAccountId == conversation.accountId,
                    isBot: isBotSender(msg),
                    senderName: target.senderName,
                    isLastInRun: target.isLastInRun,
                    sourceFrame: target.frame,
                    reactions: appState.reactions(for: msg),
                    myDid: conversation.accountId,
                    canEdit: canEdit(msg),
                    onToggleReaction: { emoji in
                        appState.toggleReaction(message: msg, emoji: emoji, conversation: conversation)
                    },
                    onMore: { showEmojiPicker = true },
                    onEdit: { startEditing(msg) },
                    onDelete: { forEveryone in
                        appState.deleteMessage(message: msg, forEveryone: forEveryone, conversation: conversation)
                    },
                    onShowHistory: { showHistory(msg) },
                    attachmentLoader: { att in
                        await appState.attachmentData(att, accountId: conversation.accountId)
                    },
                    // The overlay plays its own exit animation, then calls this
                    // to remove itself — no extra animation needed here.
                    onDismiss: { actionTarget = nil }
                )
                .transition(.opacity)
            }
        }
        .sheet(isPresented: $showEmojiPicker) {
            EmojiPickerView { emoji in
                if let target = actionTarget {
                    appState.toggleReaction(message: target.message, emoji: emoji, conversation: conversation)
                }
                // Picked from the full picker: the sheet slides away over the
                // overlay, so fade the overlay out rather than pop it.
                withAnimation(.easeOut(duration: 0.25)) { actionTarget = nil }
            }
        }
        .onAppear {
            appState.currentConversationId = conversation.id
            // An image shared/routed into this chat (docs/35): pre-stage it in the
            // composer for review before sending.
            if let data = appState.pendingStagedImage[conversation.id] {
                appState.pendingStagedImage[conversation.id] = nil
                // The share extension hands off the original image bytes undecoded
                // (docs/35) to stay under its memory limit, so resize + strip
                // metadata here — off the main thread, matching the photo-picker
                // path (`stagePickedPhoto`).
                Task {
                    let processed = await Task.detached(priority: .userInitiated) {
                        prepareImageForSending(data)
                    }.value
                    stageImageData(processed)
                }
            }
            appState.loadMessagesFromStore(conversationId: conversation.id, accountId: conversation.accountId)
            appState.loadReactions(conversationId: conversation.id, accountId: conversation.accountId)
            appState.markAllMessagesRead(conversationId: conversation.id, accountId: conversation.accountId)
            loadSavedContactDids()
            // Re-fetch the contact's encrypted profile and update the cached
            // display name if it changed. Primary change-detection path.
            if let recipientDid = conversation.recipientDid {
                appState.refreshContactProfile(did: recipientDid, accountId: conversation.accountId)
            }
            if let groupId = conversation.groupId {
                appState.refreshGroupTitle(groupId: groupId, accountId: conversation.accountId)
                Task {
                    isGroupMember = await appState.isGroupMember(
                        groupId: groupId, accountId: conversation.accountId
                    )
                }
            }
        }
        .onDisappear {
            if appState.currentConversationId == conversation.id {
                appState.currentConversationId = nil
            }
        }
        .task(id: conversation.id) {
            // After messages load, scroll to first unread (or bottom if all read).
            try? await Task.sleep(nanoseconds: 100_000_000)
            let msgs = appState.messagesByConversation[conversation.id] ?? []
            if let firstUnread = msgs.first(where: {
                $0.readAtMs == nil && $0.senderAccountId != conversation.accountId
            }) {
                scrollPosition.scrollTo(id: firstUnread.sentAtMs)
            } else {
                scrollPosition.scrollTo(edge: .bottom)
            }
        }
    }

    /// Shown in place of the composer once you've left the group (docs/53 §Leave).
    /// The transcript stays readable above; you just can't post. The "You left
    /// the group" line is the last entry in the transcript itself.
    @ViewBuilder private var leftGroupBar: some View {
        Text("You left this group")
            .font(.caption)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity)
            .padding(.horizontal)
            .padding(.vertical, 12)
    }

    /// The normal text composer (with the inline edit bar when editing).
    @ViewBuilder private var composer: some View {
        if editingMessage != nil {
            HStack(spacing: 8) {
                Image(systemName: "pencil")
                    .foregroundStyle(Color.avBrand)
                Text("Editing message")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Button { cancelEdit() } label: {
                    Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                }
            }
            .padding(.horizontal)
            .padding(.top, 6)
        }

        // Staging strip (docs/35): pending image and/or link-preview card shown
        // above the input while editing == nil, each removable with an x.
        if editingMessage == nil, stagedImageData != nil || stagedPreview != nil || stagedContact != nil {
            stagingStrip
        }

        HStack(spacing: 12) {
            // Attachment picker (docs/35) — hidden while editing a message.
            if editingMessage == nil {
                PhotosPicker(selection: $photoItem, matching: .images) {
                    Image(systemName: "plus.circle.fill")
                        .font(.title)
                        .foregroundStyle(Color.avBrand)
                }
                // Paste from the clipboard (docs/35) — shown when it holds an
                // image or a copied contact card.
                if canPasteImage || canPasteContact {
                    Button { pasteFromClipboard() } label: {
                        Image(systemName: "doc.on.clipboard")
                            .font(.title3)
                            .foregroundStyle(Color.avBrand)
                    }
                }
            }

            TextField(editingMessage == nil ? "Message" : "Edit message", text: $messageText, axis: .vertical)
                .textFieldStyle(.plain)
                .lineLimit(1...5)
                // A rounded "pill" surrounding the text lifts it off the page.
                // Native Liquid Glass on iOS 26+, a card fill + gentle shadow on
                // earlier versions (see composerPillBackground).
                .padding(.horizontal, 12)
                .padding(.vertical, 7)
                .composerPillBackground()

            Button {
                if editingMessage != nil { applyEdit() } else { send() }
            } label: {
                Image(systemName: editingMessage != nil ? "checkmark.circle.fill" : "arrow.up.circle.fill")
                    .font(.title)
            }
            .disabled(!canSend)
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .onChange(of: messageText) {
            if !messages.isEmpty {
                scrollPosition.scrollTo(edge: .bottom)
            }
            schedulePreviewFetch()
        }
        .onChange(of: photoItem) {
            guard let item = photoItem else { return }
            photoItem = nil
            stagePickedPhoto(item)
        }
        .onAppear { refreshPasteAvailability() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { refreshPasteAvailability() }
        }
        .onReceive(NotificationCenter.default.publisher(for: UIPasteboard.changedNotification)) { _ in
            refreshPasteAvailability()
        }
    }

    /// Send is enabled when there's text, a staged image, or a staged preview —
    /// not just non-empty text (an image-only message is valid).
    private var canSend: Bool {
        !messageText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            || stagedImageData != nil
            || stagedPreview != nil
            || stagedContact != nil
    }

    /// The horizontal strip of staged attachments above the input.
    @ViewBuilder private var stagingStrip: some View {
        HStack(alignment: .top, spacing: 10) {
            if let stagedImageThumb {
                ZStack(alignment: .topTrailing) {
                    Image(uiImage: stagedImageThumb)
                        .resizable()
                        .scaledToFill()
                        .frame(width: 60, height: 60)
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                    removeButton { clearStagedImage() }
                }
            }
            if let stagedPreview {
                ZStack(alignment: .topTrailing) {
                    LinkPreviewCard(preview: stagedPreview, isMe: true) { att in
                        await appState.attachmentData(att, accountId: conversation.accountId)
                    }
                    removeButton {
                        dismissedPreviewURL = stagedPreviewURL
                        clearStagedPreview()
                    }
                }
            }
            if let stagedContact {
                ZStack(alignment: .topTrailing) {
                    SharedContactCard(contact: stagedContact, isMe: true)
                    removeButton { self.stagedContact = nil }
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal)
        .padding(.top, 6)
    }

    /// The small "x" overlay used to drop a staged item.
    @ViewBuilder private func removeButton(_ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: "xmark.circle.fill")
                .font(.body)
                .foregroundStyle(.white, .black.opacity(0.5))
        }
        .padding(4)
    }

    /// The message-request gate (docs/12 §1): a stranger's first contact is
    /// read-only until the user Accepts, Deletes, or Reports & Blocks. Reporting
    /// is exposed only here — not in established conversations.
    @ViewBuilder private func messageRequestGate(did: String) -> some View {
        VStack(spacing: 10) {
            Text("Let \(conversation.title) message you and share your name with them?")
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
            HStack(spacing: 12) {
                Button(role: .destructive) {
                    Task {
                        await appState.reportAndBlock(did: did, accountId: conversation.accountId)
                    }
                } label: {
                    Text("Block").frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)

                Button(role: .destructive) {
                    Task {
                        await appState.deleteRequest(did: did, accountId: conversation.accountId)
                        dismiss()
                    }
                } label: {
                    Text("Delete").frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)

                Button {
                    Task { await appState.acceptRequest(did: did, accountId: conversation.accountId) }
                } label: {
                    Text("Accept").frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 10)
    }

    /// Shown in place of the composer for a blocked DM (docs/12 §2).
    @ViewBuilder private func blockedBar(did: String) -> some View {
        HStack(spacing: 12) {
            Text("You blocked this contact.")
                .font(.caption)
                .foregroundStyle(.secondary)
            Spacer()
            Button("Unblock") {
                Task { await appState.unblockContact(did: did, accountId: conversation.accountId) }
            }
            .buttonStyle(.bordered)
        }
        .padding(.horizontal)
        .padding(.vertical, 10)
    }

    private func startEditing(_ message: Message) {
        editingMessage = message
        messageText = message.body
    }

    private func cancelEdit() {
        editingMessage = nil
        messageText = ""
    }

    private func applyEdit() {
        guard let message = editingMessage else { return }
        appState.editMessage(message: message, newBody: messageText, conversation: conversation)
        editingMessage = nil
        messageText = ""
    }

    private func showHistory(_ message: Message) {
        Task {
            historyRevisions = await appState.loadMessageRevisions(message: message, conversation: conversation)
            historyMessage = message
        }
    }

    /// Load the picked image and stage it in the composer (docs/35). Nothing is
    /// uploaded or sent until Send; picking again replaces the staged image.
    private func stagePickedPhoto(_ item: PhotosPickerItem) {
        Task {
            guard let data = try? await item.loadTransferable(type: Data.self) else { return }
            // Re-encode for sending off the main thread (docs/35): upright,
            // resolution-capped, metadata-stripped — Signal-style.
            let processed = await Task.detached(priority: .userInitiated) {
                prepareImageForSending(data)
            }.value
            stageImageData(processed)
        }
    }

    /// Stage raw image bytes in the composer — shared by the photo picker, the
    /// clipboard paste button, and an incoming share (docs/35). Nothing is
    /// uploaded or sent until Send; staging again replaces the current image.
    private func stageImageData(_ data: Data) {
        stagedImageData = data
        // A small decoded thumbnail just for the composer chip; the full bytes
        // (above) are what gets uploaded on Send.
        Task {
            stagedImageThumb = await Task.detached(priority: .userInitiated) {
                decodeDownsampledImage(data, maxPixel: 240)
            }.value
        }
    }

    /// Stage the current clipboard content in the composer (docs/35). A copied
    /// contact card takes priority over an image; re-encodes a pasted image to
    /// JPEG to match the photo-picker path (`sendComposed` uploads image/jpeg).
    private func pasteFromClipboard() {
        if let contact = ContactPasteboard.read() {
            stagedContact = contact
            return
        }
        guard let image = UIPasteboard.general.image,
              let data = image.preparedForSending() else { return }
        stageImageData(data)
    }

    /// Refresh whether the paste button should show — i.e. the clipboard holds an
    /// image or a copied contact card. Cheap; called on appear, on app-active,
    /// and on pasteboard change.
    private func refreshPasteAvailability() {
        canPasteImage = UIPasteboard.general.hasImages
        canPasteContact = ContactPasteboard.hasContact
    }

    /// Load the set of already-curated contact DIDs (docs/35) so a received
    /// contact card renders "Saved" for people already in the book, and an
    /// active Save button otherwise.
    private func loadSavedContactDids() {
        Task {
            let rows = await appState.listContacts(accountId: conversation.accountId)
            savedContactDids = Set(rows.filter { $0.isCurated }.map { $0.did })
        }
    }

    /// Debounced link-preview generation (docs/35): ~0.6s after the last
    /// keystroke, if the text contains a new URL we haven't staged or dismissed,
    /// fetch its preview and stage it. Clears the staged preview when the URL
    /// leaves the text.
    private func schedulePreviewFetch() {
        guard editingMessage == nil else { return }
        previewTask?.cancel()
        let text = messageText
        previewTask = Task {
            try? await Task.sleep(nanoseconds: 600_000_000)
            if Task.isCancelled { return }
            let url = AppState.firstURL(in: text)?.absoluteString
            // No URL in the text — drop any staged/dismissed preview state.
            guard let url else {
                clearStagedPreview()
                dismissedPreviewURL = nil
                return
            }
            // A different URL than the one we dismissed re-enables previews.
            if url != dismissedPreviewURL { dismissedPreviewURL = nil }
            // Already staged or explicitly dismissed — nothing to do.
            if url == stagedPreviewURL || url == dismissedPreviewURL { return }
            let previews = await appState.linkPreviews(for: text, accountId: conversation.accountId)
            if Task.isCancelled { return }
            // Only adopt it if the text still ends on this same URL and the user
            // hasn't since dismissed it.
            guard AppState.firstURL(in: messageText)?.absoluteString == url, url != dismissedPreviewURL else { return }
            if let first = previews.first {
                stagedPreview = first
                stagedPreviewURL = url
            }
        }
    }

    private func clearStagedImage() {
        stagedImageData = nil
        stagedImageThumb = nil
    }

    private func clearStagedPreview() {
        stagedPreview = nil
        stagedPreviewURL = nil
    }

    private func clearStaged() {
        clearStagedImage()
        clearStagedPreview()
        stagedContact = nil
        dismissedPreviewURL = nil
        previewTask?.cancel()
    }

    /// Unified composer send (docs/35): ships the text plus any staged image and
    /// link preview as one message via `AppState.sendComposed`. Inserts the
    /// optimistic row + chat-list bump here, then clears the composer.
    private func send() {
        let trimmed = messageText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty || stagedImageData != nil || stagedPreview != nil || stagedContact != nil else { return }
        let text = messageText
        let image = stagedImageData
        let preview = stagedPreview
        let contact = stagedContact
        messageText = ""
        clearStaged()
        errorMessage = nil

        // Optimistically add to UI.
        let messageId = UUID().uuidString
        let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
        let message = Message(
            id: messageId,
            conversationId: conversation.id,
            senderAccountId: conversation.accountId,
            body: text,
            sentAtMs: nowMs,
            readAtMs: nowMs,  // outgoing = immediately read
            deliveryStatus: .sending,
            contacts: contact.map { [$0] } ?? []
        )
        appState.messagesByConversation[conversation.id, default: []].append(message)
        scrollPosition.scrollTo(edge: .bottom)

        // Update conversation metadata for sorting + the chat-list preview.
        // Clear any stale system-event fields so the preview renders this new
        // message, not a prior "X joined" / metadata line.
        if let idx = appState.conversations.firstIndex(where: { $0.id == conversation.id }) {
            appState.conversations[idx].lastMessage = text
            // Reflect the staged image's type so the row previews "📷 Photo"
            // immediately; nil clears any prior attachment decoration.
            appState.conversations[idx].lastMessagePreview = image != nil ? .photo : (contact != nil ? .contact : nil)
            appState.conversations[idx].lastMessageDate = message.sentAt
            appState.conversations[idx].lastMessageSenderDid = conversation.accountId  // "You:"
            appState.conversations[idx].clearLastMessageEvent()
        }

        Task {
            do {
                try await appState.sendComposed(
                    conversation: conversation,
                    text: text,
                    imageData: image,
                    preview: preview,
                    contact: contact,
                    messageId: messageId,
                    sentAtMs: nowMs
                )
            } catch {
                errorMessage = "Failed to send: \(error.localizedDescription)"
            }
        }
    }
}

#if DEBUG
/// Wraps `ConversationView` in a preview-ready environment: one account, a
/// canned contact for name resolution, and pre-seeded messages (which survive
/// `loadMessagesFromStore`, since it only loads when the cache is empty).
@MainActor
private func conversationPreview(_ conversation: Conversation, _ messages: [Message]) -> some View {
    let me = Account(
        id: "did:plc:me",
        displayName: "Me",
        avatarData: nil,
        servers: [ServerInfo(
            id: "https://server.example",
            name: "Example",
            url: URL(string: "https://server.example")!
        )]
    )
    let state = AppState.preview(
        accounts: [me],
        contacts: [
            ContactRowFfi(did: "did:plc:bob", displayName: "Bob Chena", isCurated: true, lastInteractionAtMs: 0),
        ]
    )
    state.conversations = [conversation]
    state.messagesByConversation[conversation.id] = messages
    return NavigationStack {
        ConversationView(conversation: conversation)
            .environmentObject(state)
    }
}

#Preview("DM") {
    let conv = Conversation(
        id: "dm-bob",
        title: "Bob Chena",
        accountId: "did:plc:me",
        serverUrl: "https://server.example",
        recipientDid: "did:plc:bob",
        groupId: nil,
        lastMessage: nil,
        lastMessageDate: nil
    )
    return conversationPreview(conv, [
        Message(id: "m1", conversationId: conv.id, senderAccountId: "did:plc:bob",
                body: "Are we still meeting at noon?", sentAtMs: 1_700_000_000_000,
                editedAtMs: nil, readAtMs: 1_700_000_001_000, deliveryStatus: .delivered),
        Message(id: "m2", conversationId: conv.id, senderAccountId: "did:plc:me",
                body: "Yes — I'll be at the front entrance.", sentAtMs: 1_700_000_060_000,
                editedAtMs: nil, readAtMs: 1_700_000_061_000, deliveryStatus: .read),
    ])
}

#Preview("Message Request") {
    let conv = Conversation(
        id: "dm-stranger",
        title: "Jordan Vale",
        accountId: "did:plc:me",
        serverUrl: "https://server.example",
        recipientDid: "did:plc:stranger",
        groupId: nil,
        lastMessage: nil,
        lastMessageDate: nil,
        isRequest: true
    )
    return conversationPreview(conv, [
        Message(id: "m1", conversationId: conv.id, senderAccountId: "did:plc:stranger",
                body: "Hi! I saw you at the rally — want to join our organizing channel?",
                sentAtMs: 1_700_000_000_000,
                editedAtMs: nil, readAtMs: nil, deliveryStatus: .delivered),
    ])
}

#Preview("Blocked") {
    let conv = Conversation(
        id: "dm-blocked",
        title: "Jordan Vale",
        accountId: "did:plc:me",
        serverUrl: "https://server.example",
        recipientDid: "did:plc:stranger",
        groupId: nil,
        lastMessage: nil,
        lastMessageDate: nil,
        isBlocked: true
    )
    return conversationPreview(conv, [
        Message(id: "m1", conversationId: conv.id, senderAccountId: "did:plc:stranger",
                body: "Hi! I saw you at the rally — want to join our organizing channel?",
                sentAtMs: 1_700_000_000_000,
                editedAtMs: nil, readAtMs: nil, deliveryStatus: .delivered),
    ])
}

#Preview("Group") {
    let gid = "grp1"
    let conv = Conversation(
        id: groupConversationId(gid),
        title: "March Logistics",
        accountId: "did:plc:me",
        serverUrl: "https://server.example",
        recipientDid: nil,
        groupId: gid,
        lastMessage: nil,
        lastMessageDate: nil,
        isGroup: true
    )
    return conversationPreview(conv, [
        Message(id: "m1", conversationId: conv.id, senderAccountId: "did:plc:bob",
                body: "Crew — check in when you arrive.", sentAtMs: 1_700_000_000_000,
                editedAtMs: nil, readAtMs: 1_700_000_001_000, deliveryStatus: .delivered),
        // Consecutive message from Bob: no second name label (same run).
        Message(id: "m2", conversationId: conv.id, senderAccountId: "did:plc:bob",
                body: "Bring water, it's hot out.", sentAtMs: 1_700_000_010_000,
                editedAtMs: nil, readAtMs: 1_700_000_011_000, deliveryStatus: .delivered),
        // Different sender: gets its own (differently colored) name label.
        Message(id: "m3", conversationId: conv.id, senderAccountId: "did:plc:carol",
                body: "Almost there!", sentAtMs: 1_700_000_020_000,
                editedAtMs: nil, readAtMs: 1_700_000_021_000, deliveryStatus: .delivered),
        Message(id: "m4", conversationId: conv.id, senderAccountId: "did:plc:me",
                body: "On site 👍", sentAtMs: 1_700_000_060_000,
                editedAtMs: nil, readAtMs: 1_700_000_061_000, deliveryStatus: .read),
    ])
}
#endif

/// A centered grey system line in the conversation timeline for a group
/// membership/metadata event (docs/03 §3.6) — "Alice added Bob", "Bob left", etc.
struct GroupSystemEventRow: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 4)
            .accessibilityAddTraits(.isStaticText)
    }
}

private extension View {
    /// Background for the message-composer input pill. On iOS 26+ this is the
    /// native Liquid Glass material (`glassEffect`), which adapts to light/dark
    /// and the content behind it. On iOS 18–25, where Liquid Glass isn't
    /// available, it falls back to a solid `avCard` fill with a gentle drop
    /// shadow so the pill still lifts off the page.
    @ViewBuilder
    func composerPillBackground() -> some View {
        if #available(iOS 26.0, *) {
            self.glassEffect(.regular, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        } else {
            self.background(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .fill(Color.avCard)
                    .shadow(color: .black.opacity(0.12), radius: 3, x: 0, y: 1)
            )
        }
    }
}
