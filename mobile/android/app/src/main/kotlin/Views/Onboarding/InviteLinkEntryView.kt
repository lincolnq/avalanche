package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/**
 * Screen where the user pastes or types an invite link.
 *
 * Mirrors iOS InviteLinkEntryView.swift. The nav destination
 * (IdentityPickerView) is surfaced via [onInviteTokenResolved] so a central
 * NavGraph can wire the transition.
 *
 * @param onInviteTokenResolved Called with the validated [InviteToken] when
 *   the link is confirmed; the caller should push IdentityPickerView.
 */
@Composable
fun InviteLinkEntryView(
    onInviteTokenResolved: (InviteToken) -> Unit = {},
) {
    var linkText by remember { mutableStateOf("") }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    var isValidating by remember { mutableStateOf(false) }

    val scope = rememberCoroutineScope()

    fun validateLink() {
        errorMessage = null
        isValidating = true
        scope.launch {
            runCatching {
                val trimmed = linkText.trim()
                // Mirror iOS logic: full URL vs. bare token.
                if (trimmed.startsWith("http://") || trimmed.startsWith("https://")) {
                    InviteToken.fromUrl(trimmed)
                } else {
                    InviteToken.fromToken(trimmed)
                }
            }.onSuccess { token ->
                isValidating = false
                onInviteTokenResolved(token)
            }.onFailure { error ->
                isValidating = false
                errorMessage = error.localizedMessage ?: error.message ?: "Unknown error"
            }
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(AvalancheColors.Paper)
            .windowInsetsPadding(WindowInsets.systemBars)
            .padding(top = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        OutlinedTextField(
            value = linkText,
            onValueChange = { linkText = it },
            label = { Text("Paste invite link") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Uri,
                capitalization = KeyboardCapitalization.None,
                imeAction = ImeAction.Done,
            ),
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 32.dp),
        )

        if (errorMessage != null) {
            Text(
                text = errorMessage!!,
                color = AvalancheColors.Error,
                modifier = Modifier.padding(top = 8.dp, start = 32.dp, end = 32.dp),
            )
        }

        Button(
            onClick = { validateLink() },
            enabled = linkText.isNotEmpty() && !isValidating,
            modifier = Modifier
                .padding(top = 16.dp)
                .padding(horizontal = 32.dp)
                .fillMaxWidth(),
        ) {
            if (isValidating) {
                CircularProgressIndicator(
                    color = AvalancheColors.Paper,
                    strokeWidth = 2.dp,
                    modifier = Modifier.padding(horizontal = 8.dp),
                )
            } else {
                Text("Continue")
            }
        }

        Spacer(modifier = Modifier.weight(1f))
    }
}

@Preview(showBackground = true)
@Composable
private fun InviteLinkEntryPreview() {
    AvalancheTheme {
        InviteLinkEntryView()
    }
}
