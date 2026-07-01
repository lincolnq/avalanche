package net.theavalanche.app

import android.net.Uri
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

/// Consent dialog for "Sign in with Avalanche" (docs/25-project-login.md).
///
/// This is the user's act of *choosing to sign in* to a Project as this
/// identity — not a per-scope approval (scopes are admin-granted). The granted
/// capabilities are shown for legibility. For the cross-device (device-grant)
/// front-end it adds the "signing in on another device" phishing warning.
/// Mirrors iOS `ProjectLoginConsentView`.
@Composable
fun ProjectLoginConsentDialog(
    request: ProjectLoginRequest,
    onApprove: () -> Unit,
    onCancel: () -> Unit,
) {
    val serverHost = Uri.parse(request.serverUrl).host ?: request.serverUrl
    val projectHost = request.projectUrl?.let { Uri.parse(it).host }
    val badge = if (request.official) "  ✓" else ""

    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text("Sign in with Avalanche") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(
                    text = request.displayLabel + badge,
                    fontWeight = FontWeight.SemiBold,
                    style = MaterialTheme.typography.titleMedium,
                )
                if (projectHost != null) {
                    Text(projectHost, style = MaterialTheme.typography.bodySmall)
                }
                Text("You'll sign in with your account on $serverHost.")

                if (request.isCrossDevice) {
                    Surface(
                        color = MaterialTheme.colorScheme.errorContainer,
                        shape = RoundedCornerShape(10.dp),
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text(
                            text = "You're signing in on another device. Only continue if you started this on your other device.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onErrorContainer,
                            modifier = Modifier.padding(12.dp),
                        )
                    }
                }

                Text(
                    "This Project can message you and add you to groups.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
        confirmButton = { TextButton(onClick = onApprove) { Text("Sign In") } },
        dismissButton = { TextButton(onClick = onCancel) { Text("Cancel") } },
    )
}
