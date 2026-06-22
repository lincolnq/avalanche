package net.theavalanche.app

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview

/// Reusable disappearing-messages timer picker. Mirrors
/// mobile/ios/Actnet/Sources/Views/Common/DisappearingMessagesPicker.swift.
///
/// Binds to a duration in seconds (0 = off), matching the expiry_seconds the
/// Rust core stores for groups (create_group) and DMs (set_conversation_timer).
/// The option set mirrors Signal's standard durations.
object DisappearingMessagesPicker {
    /// (label, seconds). 0 is the "Off" sentinel.
    val options: List<Pair<String, UInt>> = listOf(
        Pair("Off", 0u),
        Pair("30 seconds", 30u),
        Pair("5 minutes", (5 * 60).toUInt()),
        Pair("1 hour", (60 * 60).toUInt()),
        Pair("8 hours", (8 * 60 * 60).toUInt()),
        Pair("1 day", (24 * 60 * 60).toUInt()),
        Pair("1 week", (7 * 24 * 60 * 60).toUInt()),
        Pair("4 weeks", (4 * 7 * 24 * 60 * 60).toUInt()),
    )

    /// Human label for an arbitrary stored value; falls back to the raw second
    /// count if it isn't one of the canonical options (e.g. a value set by a
    /// future custom-duration UI).
    fun label(seconds: UInt): String =
        options.firstOrNull { it.second == seconds }?.first ?: "${seconds}s"
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DisappearingMessagesPickerView(
    seconds: UInt,
    onSecondsChange: (UInt) -> Unit,
    modifier: Modifier = Modifier,
) {
    var expanded by remember { mutableStateOf(false) }

    ExposedDropdownMenuBox(
        expanded = expanded,
        onExpandedChange = { expanded = it },
        modifier = modifier,
    ) {
        OutlinedTextField(
            value = DisappearingMessagesPicker.label(seconds),
            onValueChange = {},
            readOnly = true,
            label = { Text("Disappearing messages") },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = expanded) },
            colors = ExposedDropdownMenuDefaults.outlinedTextFieldColors(),
            modifier = Modifier
                .fillMaxWidth()
                .menuAnchor(androidx.compose.material3.ExposedDropdownMenuAnchorType.PrimaryNotEditable),
        )

        ExposedDropdownMenu(
            expanded = expanded,
            onDismissRequest = { expanded = false },
        ) {
            DisappearingMessagesPicker.options.forEach { (label, value) ->
                DropdownMenuItem(
                    text = { Text(label) },
                    onClick = {
                        onSecondsChange(value)
                        expanded = false
                    },
                )
            }
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun DisappearingMessagesPickerPreview() {
    var seconds by remember { mutableStateOf(0u) }
    AvalancheTheme {
        DisappearingMessagesPickerView(
            seconds = seconds,
            onSecondsChange = { seconds = it },
        )
    }
}
