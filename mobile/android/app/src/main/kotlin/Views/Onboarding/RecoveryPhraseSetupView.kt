package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.app_core.AppErrorFfi
import uniffi.app_core.generateRecoveryPhrase
import uniffi.app_core.recoveryPhraseToSeed

// ---------------------------------------------------------------------------
// RecoveryPhraseSetupView
//
// Signup-time flow for the "recovery phrase" account mode. Generates a 12-word
// BIP39 phrase (via the Rust FFI), shows it alongside the home server URL for
// the user to write down, verifies they recorded it, then creates the account
// using the phrase-derived seed in place of a passkey PRF output.
//
// Mirrors iOS Sources/Views/Onboarding/RecoveryPhraseSetupView.swift.
// ---------------------------------------------------------------------------

private enum class RecoveryPhraseStage { DISPLAY, VERIFY }

@Composable
fun RecoveryPhraseSetupView(
    appViewModel: AppViewModel,
    inviteToken: InviteToken,
    displayName: String,
    onComplete: () -> Unit = {},
) {
    val coroutineScope = rememberCoroutineScope()

    var words by remember { mutableStateOf<List<String>>(emptyList()) }
    var stage by remember { mutableStateOf(RecoveryPhraseStage.DISPLAY) }
    // Three word positions (1-based, ascending) the user must re-enter.
    var quizPositions by remember { mutableStateOf<List<Int>>(emptyList()) }
    val quizAnswers = remember { mutableStateMapOf<Int, String>() }
    var isRegistering by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }

    // Generate the recovery phrase once on first composition.
    LaunchedEffect(Unit) {
        if (words.isNotEmpty()) return@LaunchedEffect
        try {
            val phrase = withContext(Dispatchers.IO) { generateRecoveryPhrase() }
            val parsed = phrase.trim().split("\\s+".toRegex())
            words = parsed
            quizPositions = pickQuizPositions(parsed.size)
        } catch (e: AppErrorFfi) {
            errorMessage = "Couldn't generate a recovery phrase: ${e.message}"
        } catch (e: Exception) {
            errorMessage = "Couldn't generate a recovery phrase: ${e.message}"
        }
    }

    when (stage) {
        RecoveryPhraseStage.DISPLAY -> {
            DisplayStage(
                words = words,
                inviteToken = inviteToken,
                errorMessage = errorMessage,
                onContinue = {
                    errorMessage = null
                    quizAnswers.clear()
                    stage = RecoveryPhraseStage.VERIFY
                },
            )
        }
        RecoveryPhraseStage.VERIFY -> {
            VerifyStage(
                quizPositions = quizPositions,
                quizAnswers = quizAnswers,
                isRegistering = isRegistering,
                errorMessage = errorMessage,
                onAnswerChanged = { pos, answer -> quizAnswers[pos] = answer },
                onBack = {
                    errorMessage = null
                    stage = RecoveryPhraseStage.DISPLAY
                },
                onVerifyAndCreate = {
                    // Case-insensitive, whitespace-trimmed match.
                    val allCorrect = quizPositions.all { pos ->
                        val expected = words.getOrNull(pos - 1)?.lowercase() ?: ""
                        val got = (quizAnswers[pos] ?: "").trim().lowercase()
                        expected == got
                    }
                    if (!allCorrect) {
                        errorMessage = "Those words don't match. Double-check what you wrote down."
                        return@VerifyStage
                    }

                    isRegistering = true
                    errorMessage = null
                    coroutineScope.launch {
                        try {
                            val phrase = words.joinToString(" ")
                            val seed = withContext(Dispatchers.IO) {
                                recoveryPhraseToSeed(phrase = phrase)
                            }
                            appViewModel.createAccount(
                                serverUrl = inviteToken.serverUrl,
                                serverName = inviteToken.serverName,
                                displayName = displayName,
                                inviteToken = inviteToken.token,
                                prfOutput = seed,
                            )
                            // createAccount sets isOnboarding = false → main tab view.
                            // Follow the invite's post-onboarding redirect if present.
                            val redirect = inviteToken.postOnboardingRedirect
                            if (redirect != null) {
                                kotlinx.coroutines.delay(300)
                                val uri = android.net.Uri.parse(redirect)
                                appViewModel.handleDeepLink(uri)
                            }
                            onComplete()
                        } catch (e: AppErrorFfi) {
                            errorMessage = e.message ?: "An error occurred"
                            isRegistering = false
                        } catch (e: Exception) {
                            errorMessage = e.message ?: "An error occurred"
                            isRegistering = false
                        }
                    }
                },
            )
        }
    }
}

// ---------------------------------------------------------------------------
// DisplayStage — show the 12 words + server card + "I've written it down"
// ---------------------------------------------------------------------------

@Composable
private fun DisplayStage(
    words: List<String>,
    inviteToken: InviteToken,
    errorMessage: String?,
    onContinue: () -> Unit,
) {
    val scrollState = rememberScrollState()

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(LocalAvalancheColors.current.paper)
            .verticalScroll(scrollState)
            .padding(top = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(24.dp),
    ) {
        Text(
            text = "Write down your recovery phrase",
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.SemiBold,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(horizontal = 32.dp),
        )

        Text(
            text = "These 12 words and your home server are the only way to recover this identity. " +
                    "Store them somewhere safe — anyone with them can access your account.",
            style = MaterialTheme.typography.bodySmall,
            color = LocalAvalancheColors.current.muted,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(horizontal = 32.dp),
        )

        // Server card
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 32.dp)
                .clip(RoundedCornerShape(12.dp))
                .background(LocalAvalancheColors.current.card.copy(alpha = 0.5f))
                .padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(
                text = "HOME SERVER",
                style = MaterialTheme.typography.labelSmall,
                color = LocalAvalancheColors.current.muted,
            )
            Text(
                text = inviteToken.serverName,
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.Medium,
            )
            SelectionContainer {
                Text(
                    text = inviteToken.serverUrl,
                    style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                    color = LocalAvalancheColors.current.muted,
                    textAlign = TextAlign.Center,
                )
            }
        }

        // Word grid — 2 columns
        WordGrid(
            words = words,
            modifier = Modifier.padding(horizontal = 32.dp),
        )

        errorMessage?.let { err ->
            Text(
                text = err,
                color = LocalAvalancheColors.current.error,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 32.dp),
            )
        }

        Button(
            onClick = onContinue,
            enabled = words.isNotEmpty(),
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 32.dp)
                .padding(bottom = 32.dp)
                .height(52.dp),
        ) {
            Text("I've written it down")
        }
    }
}

@Composable
private fun WordGrid(
    words: List<String>,
    modifier: Modifier = Modifier,
) {
    // LazyVerticalGrid requires a fixed height when inside a vertically scrolling parent.
    // We compute a fixed height: (rows * itemHeight) + (rows-1 * spacing).
    val rows = (words.size + 1) / 2
    val itemHeightDp = 40
    val spacingDp = 10
    val gridHeight = (rows * itemHeightDp + (rows - 1) * spacingDp).dp

    LazyVerticalGrid(
        columns = GridCells.Fixed(2),
        verticalArrangement = Arrangement.spacedBy(spacingDp.dp),
        horizontalArrangement = Arrangement.spacedBy(spacingDp.dp),
        modifier = modifier
            .fillMaxWidth()
            .height(gridHeight),
        userScrollEnabled = false,
    ) {
        itemsIndexed(words) { index, word ->
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(8.dp))
                    .background(LocalAvalancheColors.current.card.copy(alpha = 0.5f))
                    .padding(vertical = 8.dp, horizontal = 10.dp),
            ) {
                Text(
                    text = "${index + 1}.",
                    style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                    color = LocalAvalancheColors.current.muted,
                    modifier = Modifier.width(24.dp),
                    textAlign = TextAlign.End,
                )
                Spacer(Modifier.width(8.dp))
                Text(
                    text = word,
                    style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                    fontWeight = FontWeight.Medium,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// VerifyStage — quiz the user on 3 random words before creating the account
// ---------------------------------------------------------------------------

@Composable
private fun VerifyStage(
    quizPositions: List<Int>,
    quizAnswers: Map<Int, String>,
    isRegistering: Boolean,
    errorMessage: String?,
    onAnswerChanged: (pos: Int, answer: String) -> Unit,
    onBack: () -> Unit,
    onVerifyAndCreate: () -> Unit,
) {
    val scrollState = rememberScrollState()

    val allAnswered = quizPositions.isNotEmpty() &&
            quizPositions.all { pos -> (quizAnswers[pos] ?: "").isNotEmpty() }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(LocalAvalancheColors.current.paper)
            .verticalScroll(scrollState)
            .padding(top = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(24.dp),
    ) {
        Text(
            text = "Confirm your recovery phrase",
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.SemiBold,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(horizontal = 32.dp),
        )

        Text(
            text = "Enter the following words from the phrase you just wrote down.",
            style = MaterialTheme.typography.bodySmall,
            color = LocalAvalancheColors.current.muted,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(horizontal = 32.dp),
        )

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 32.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            for (pos in quizPositions) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(
                        text = "Word #$pos",
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.width(80.dp),
                    )
                    OutlinedTextField(
                        value = quizAnswers[pos] ?: "",
                        onValueChange = { onAnswerChanged(pos, it) },
                        singleLine = true,
                        modifier = Modifier.weight(1f),
                        // Disable autocorrect/autocap to match iOS .autocapitalization(.none) /
                        // .autocorrectionDisabled()
                        keyboardOptions = KeyboardOptions(
                            capitalization = KeyboardCapitalization.None,
                            autoCorrectEnabled = false,
                        ),
                    )
                }
            }
        }

        errorMessage?.let { err ->
            Text(
                text = err,
                color = LocalAvalancheColors.current.error,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 32.dp),
            )
        }

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 32.dp)
                .padding(bottom = 32.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Button(
                onClick = onVerifyAndCreate,
                enabled = !isRegistering && allAnswered,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(52.dp),
            ) {
                if (isRegistering) {
                    CircularProgressIndicator(
                        modifier = Modifier
                            .width(24.dp)
                            .height(24.dp),
                        color = MaterialTheme.colorScheme.onPrimary,
                        strokeWidth = 2.dp,
                    )
                } else {
                    Text("Verify & Create Account")
                }
            }

            TextButton(
                onClick = onBack,
                enabled = !isRegistering,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = "Show the phrase again",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Pick three distinct word positions (1-based) in ascending order.
 * Mirrors iOS RecoveryPhraseSetupView.pickQuizPositions(count:).
 */
private fun pickQuizPositions(count: Int): List<Int> {
    if (count < 3) return (1..maxOf(count, 1)).toList()
    val chosen = mutableSetOf<Int>()
    while (chosen.size < 3) {
        chosen.add((1..count).random())
    }
    return chosen.sorted()
}

// ---------------------------------------------------------------------------
// Previews
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun DisplayStagePreview() {
    AvalancheTheme {
        DisplayStage(
            words = listOf(
                "abandon", "ability", "able", "about",
                "above", "absent", "absorb", "abstract",
                "absurd", "abuse", "access", "accident",
            ),
            inviteToken = InviteToken(
                token = "preview",
                serverUrl = "https://home.example.com",
                serverName = "Example Server",
                inviterDid = null,
                postOnboardingRedirect = null,
                privacyPolicyUrl = null,
            ),
            errorMessage = null,
            onContinue = {},
        )
    }
}

@Preview(showBackground = true)
@Composable
private fun VerifyStagePreview() {
    AvalancheTheme {
        VerifyStage(
            quizPositions = listOf(2, 7, 11),
            quizAnswers = mapOf(2 to "ability"),
            isRegistering = false,
            errorMessage = null,
            onAnswerChanged = { _, _ -> },
            onBack = {},
            onVerifyAndCreate = {},
        )
    }
}
