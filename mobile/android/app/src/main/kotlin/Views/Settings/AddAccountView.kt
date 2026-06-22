package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp

// Settings screen that lets the user add another account.
// Mirrors mobile/ios/Actnet/Sources/Views/Settings/AddAccountView.swift.
// Navigation is delegated to the caller via lambda params — the NavGraph wires
// these to the concrete destination composables (QRScannerView,
// InviteLinkEntryView, RecoveryExplainerView).
@Composable
fun AddAccountView(
    onScanInvite: () -> Unit = {},
    onEnterLink: () -> Unit = {},
    onRecover: () -> Unit = {},
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(AvalancheColors.Paper)
            .padding(horizontal = 32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(modifier = Modifier.weight(1f))

        Column(
            verticalArrangement = Arrangement.spacedBy(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Button(
                onClick = onScanInvite,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(52.dp),
            ) {
                Icon(Icons.Filled.QrCodeScanner, contentDescription = null)
                Spacer(modifier = Modifier.width(8.dp))
                Text("Scan Invite QR Code")
            }

            OutlinedButton(
                onClick = onEnterLink,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(52.dp),
                colors = ButtonDefaults.outlinedButtonColors(
                    contentColor = AvalancheColors.Brand,
                ),
            ) {
                Icon(Icons.Filled.Link, contentDescription = null)
                Spacer(modifier = Modifier.width(8.dp))
                Text("Enter Invite Link")
            }

            TextButton(
                onClick = onRecover,
                modifier = Modifier.padding(top = 8.dp),
            ) {
                Text(
                    text = "Recover a different identity",
                    color = AvalancheColors.Muted,
                )
            }
        }

        Spacer(modifier = Modifier.weight(1f))
    }
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun AddAccountPreview() {
    AvalancheTheme {
        AddAccountView()
    }
}
