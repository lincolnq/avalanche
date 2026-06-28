import SwiftUI

/// Signup-time flow for the "recovery phrase" account mode — the alternative to
/// a WebAuthn passkey. Generates a 12-word BIP39 phrase (via the Rust FFI),
/// shows it alongside the home server URL for the user to write down, verifies
/// they recorded it, then creates the account using the phrase-derived seed in
/// place of a passkey PRF output.
struct RecoveryPhraseSetupView: View {
    @EnvironmentObject var appState: AppState
    let inviteToken: InviteToken
    let displayName: String

    private enum Stage { case display, verify }

    @State private var words: [String] = []
    @State private var stage: Stage = .display
    /// Three word positions (1-based, ascending) the user must re-enter to prove
    /// they wrote the phrase down. Chosen once when the phrase is generated.
    @State private var quizPositions: [Int] = []
    @State private var quizAnswers: [Int: String] = [:]
    @State private var isRegistering = false
    @State private var errorMessage: String?

    var body: some View {
        Group {
            switch stage {
            case .display: displayStage
            case .verify: verifyStage
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.avPaper)
        .navigationTitle("Recovery Phrase")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            guard words.isEmpty else { return }
            do {
                let phrase = try generateRecoveryPhrase()
                words = phrase.split(separator: " ").map(String.init)
                quizPositions = Self.pickQuizPositions(count: words.count)
            } catch {
                errorMessage = "Couldn't generate a recovery phrase: \(error.localizedDescription)"
            }
        }
    }

    // MARK: - Display stage

    private var displayStage: some View {
        ScrollView {
            VStack(spacing: 24) {
                Text("Write down your recovery phrase")
                    .font(.title2)
                    .fontWeight(.semibold)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 32)

                Text("These 12 words and your home server are the only way to recover this identity. Store them somewhere safe — anyone with them can access your account.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 32)

                serverCard

                wordGrid

                if let error = errorMessage {
                    Text(error)
                        .foregroundStyle(Color.avError)
                        .font(.callout)
                        .padding(.horizontal, 32)
                }

                Button {
                    errorMessage = nil
                    quizAnswers = [:]
                    stage = .verify
                } label: {
                    Text("I've written it down")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(words.isEmpty)
                .padding(.horizontal, 32)
                .padding(.bottom, 32)
            }
            .padding(.top, 24)
        }
    }

    private var serverCard: some View {
        VStack(spacing: 4) {
            Text("HOME SERVER")
                .font(.caption2)
                .foregroundStyle(.secondary)
            Text(inviteToken.serverName)
                .font(.headline)
            Text(inviteToken.serverUrl.absoluteString)
                .font(.system(.caption, design: .monospaced))
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
        }
        .frame(maxWidth: .infinity)
        .padding()
        .background(Color.avCard.opacity(0.5))
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .padding(.horizontal, 32)
    }

    private var wordGrid: some View {
        let columns = [GridItem(.flexible()), GridItem(.flexible())]
        return LazyVGrid(columns: columns, spacing: 10) {
            ForEach(Array(words.enumerated()), id: \.offset) { index, word in
                HStack(spacing: 8) {
                    Text("\(index + 1).")
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(.secondary)
                        .frame(width: 24, alignment: .trailing)
                    Text(word)
                        .font(.system(.body, design: .monospaced))
                        .fontWeight(.medium)
                    Spacer()
                }
                .padding(.vertical, 8)
                .padding(.horizontal, 10)
                .background(Color.avCard.opacity(0.5))
                .clipShape(RoundedRectangle(cornerRadius: 8))
            }
        }
        .padding(.horizontal, 32)
    }

    // MARK: - Verify stage

    private var verifyStage: some View {
        ScrollView {
            VStack(spacing: 24) {
                Text("Confirm your recovery phrase")
                    .font(.title2)
                    .fontWeight(.semibold)
                    .multilineTextAlignment(.center)

                Text("Enter the following words from the phrase you just wrote down.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 32)

                VStack(spacing: 12) {
                    ForEach(quizPositions, id: \.self) { pos in
                        HStack {
                            Text("Word #\(pos)")
                                .font(.subheadline)
                                .frame(width: 80, alignment: .leading)
                            TextField("", text: Binding(
                                get: { quizAnswers[pos] ?? "" },
                                set: { quizAnswers[pos] = $0 }
                            ))
                            .textFieldStyle(.roundedBorder)
                            .autocapitalization(.none)
                            .autocorrectionDisabled()
                        }
                    }
                }
                .padding(.horizontal, 32)

                if let error = errorMessage {
                    Text(error)
                        .foregroundStyle(Color.avError)
                        .font(.callout)
                        .padding(.horizontal, 32)
                }

                VStack(spacing: 12) {
                    Button {
                        verifyAndCreate()
                    } label: {
                        if isRegistering {
                            ProgressView().frame(maxWidth: .infinity)
                        } else {
                            Text("Verify & Create Account").frame(maxWidth: .infinity)
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .disabled(isRegistering || quizPositions.contains { (quizAnswers[$0] ?? "").isEmpty })

                    Button {
                        errorMessage = nil
                        stage = .display
                    } label: {
                        Text("Show the phrase again").font(.subheadline)
                    }
                    .disabled(isRegistering)
                }
                .padding(.horizontal, 32)
                .padding(.bottom, 32)
            }
            .padding(.top, 24)
        }
    }

    // MARK: - Actions

    private func verifyAndCreate() {
        // Case-insensitive, whitespace-trimmed match against the words shown.
        let allCorrect = quizPositions.allSatisfy { pos in
            let expected = words[pos - 1].lowercased()
            let got = (quizAnswers[pos] ?? "").trimmingCharacters(in: .whitespaces).lowercased()
            return expected == got
        }
        guard allCorrect else {
            errorMessage = "Those words don't match. Double-check what you wrote down."
            return
        }

        isRegistering = true
        errorMessage = nil
        Task {
            do {
                // The phrase-derived seed plays the role a passkey's PRF output
                // would: Rust runs it through the same HKDF → rotation key + blob
                // key pipeline (see createAccount → prepareAccount).
                let seed = try recoveryPhraseToSeed(phrase: words.joined(separator: " "))
                try await appState.createAccount(
                    serverUrl: inviteToken.serverUrl.absoluteString,
                    serverName: inviteToken.serverName,
                    displayName: displayName,
                    inviteToken: inviteToken.token,
                    prfOutput: seed
                )
                // createAccount flips isOnboarding = false → MainTabView. Follow
                // the invite's post-onboarding redirect if it carries one.
                if let redirect = inviteToken.postOnboardingRedirect,
                   let url = URL(string: redirect) {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                        appState.handleDeepLink(url)
                    }
                }
            } catch {
                errorMessage = error.localizedDescription
                isRegistering = false
            }
        }
    }

    /// Pick three distinct word positions (1-based) in ascending order.
    private static func pickQuizPositions(count: Int) -> [Int] {
        guard count >= 3 else { return Array(1...max(count, 1)) }
        var chosen = Set<Int>()
        while chosen.count < 3 {
            chosen.insert(Int.random(in: 1...count))
        }
        return chosen.sorted()
    }
}
