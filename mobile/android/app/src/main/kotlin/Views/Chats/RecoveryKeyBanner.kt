package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.GppBad
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// Mirrors mobile/ios/Actnet/Sources/Views/Chats/RecoveryKeyBanner.swift.
// Shown at the top of the chat list when the user has not yet set up a recovery key.
// The banner starts dismissed (hidden) matching the iOS default state.
@Composable
fun RecoveryKeyBanner(
    onSetUp: () -> Unit = {},
) {
    var dismissed by remember { mutableStateOf(true) }

    if (!dismissed) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(AvalancheColors.Warning.copy(alpha = 0.15f))
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = Icons.Filled.GppBad,
                contentDescription = null,
                tint = AvalancheColors.Warning,
                modifier = Modifier.size(20.dp),
            )
            Text(
                text = "Secure your account",
                fontSize = 14.sp,
                color = AvalancheColors.Ink,
                modifier = Modifier.padding(start = 8.dp),
            )
            Spacer(modifier = Modifier.weight(1f))
            TextButton(onClick = onSetUp) {
                Text(
                    text = "Set up",
                    fontSize = 14.sp,
                    fontWeight = FontWeight.Bold,
                    color = AvalancheColors.Ink,
                )
            }
            IconButton(
                onClick = { dismissed = true },
                modifier = Modifier.size(32.dp),
            ) {
                Icon(
                    imageVector = Icons.Filled.Close,
                    contentDescription = "Dismiss",
                    tint = AvalancheColors.Muted,
                    modifier = Modifier.size(12.dp),
                )
            }
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun RecoveryKeyBannerPreview() {
    AvalancheTheme {
        // Override dismissed state to show the banner in the preview
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(AvalancheColors.Warning.copy(alpha = 0.15f))
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = Icons.Filled.GppBad,
                contentDescription = null,
                tint = AvalancheColors.Warning,
                modifier = Modifier.size(20.dp),
            )
            Text(
                text = "Secure your account",
                fontSize = 14.sp,
                color = AvalancheColors.Ink,
                modifier = Modifier.padding(start = 8.dp),
            )
            Spacer(modifier = Modifier.weight(1f))
            TextButton(onClick = {}) {
                Text(
                    text = "Set up",
                    fontSize = 14.sp,
                    fontWeight = FontWeight.Bold,
                    color = AvalancheColors.Ink,
                )
            }
            IconButton(
                onClick = {},
                modifier = Modifier.size(32.dp),
            ) {
                Icon(
                    imageVector = Icons.Filled.Close,
                    contentDescription = "Dismiss",
                    tint = AvalancheColors.Muted,
                    modifier = Modifier.size(12.dp),
                )
            }
        }
    }
}
