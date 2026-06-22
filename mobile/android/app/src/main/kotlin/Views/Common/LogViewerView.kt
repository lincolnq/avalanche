package net.theavalanche.app

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// General-purpose log viewer. Reads from AppLog and shows newest entries at the
// bottom. Mirrors mobile/ios/Actnet/Sources/Views/Common/LogViewerView.swift.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LogViewerView(
    onDismiss: () -> Unit = {},
) {
    val allEntries by AppLog.entries.collectAsState()
    var filter by remember { mutableStateOf("") }
    var menuExpanded by remember { mutableStateOf(false) }
    val context = LocalContext.current

    val visible = remember(allEntries, filter) {
        if (filter.isEmpty()) {
            allEntries
        } else {
            val lower = filter.lowercase()
            allEntries.filter {
                it.message.lowercase().contains(lower) || it.category.lowercase().contains(lower)
            }
        }
    }

    val listState = rememberLazyListState()

    // Scroll to bottom when entries appear or new entries arrive.
    LaunchedEffect(visible.size) {
        if (visible.isNotEmpty()) {
            listState.scrollToItem(visible.size - 1)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Logs") },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = AvalancheColors.Paper,
                    titleContentColor = AvalancheColors.Ink,
                ),
                navigationIcon = {
                    TextButton(onClick = onDismiss) {
                        Text("Close", color = AvalancheColors.Brand)
                    }
                },
                actions = {
                    Box {
                        IconButton(onClick = { menuExpanded = true }) {
                            Icon(
                                Icons.Filled.MoreVert,
                                contentDescription = "More options",
                                tint = AvalancheColors.Brand,
                            )
                        }
                        DropdownMenu(
                            expanded = menuExpanded,
                            onDismissRequest = { menuExpanded = false },
                        ) {
                            DropdownMenuItem(
                                text = { Text("Copy all") },
                                onClick = {
                                    menuExpanded = false
                                    copyAllToClipboard(context, visible)
                                },
                            )
                            DropdownMenuItem(
                                text = { Text("Clear", color = AvalancheColors.Error) },
                                onClick = {
                                    menuExpanded = false
                                    AppLog.clear()
                                },
                            )
                        }
                    }
                },
            )
        },
        containerColor = AvalancheColors.Paper,
    ) { innerPadding ->
        androidx.compose.foundation.layout.Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
        ) {
            OutlinedTextField(
                value = filter,
                onValueChange = { filter = it },
                placeholder = { Text("Filter…", color = AvalancheColors.Muted) },
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 8.dp, vertical = 4.dp),
                singleLine = true,
            )

            SelectionContainer {
                LazyColumn(
                    state = listState,
                    modifier = Modifier
                        .fillMaxSize()
                        .background(AvalancheColors.Paper)
                        .padding(8.dp),
                ) {
                    items(visible, key = { it.id }) { entry ->
                        Text(
                            text = entry.formatted,
                            fontSize = 10.sp,
                            fontFamily = FontFamily.Monospace,
                            color = colorForLevel(entry.level),
                            overflow = TextOverflow.Visible,
                            softWrap = true,
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = 1.dp),
                        )
                    }
                }
            }
        }
    }
}

private fun colorForLevel(level: LogEntry.Level): Color = when (level) {
    LogEntry.Level.info -> AvalancheColors.Ink
    LogEntry.Level.warn -> AvalancheColors.Warning
    LogEntry.Level.error -> AvalancheColors.Error
    LogEntry.Level.ok -> AvalancheColors.Brand
}

private fun copyAllToClipboard(context: Context, entries: List<LogEntry>) {
    val text = entries.joinToString("\n") { it.formatted }
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    clipboard.setPrimaryClip(ClipData.newPlainText("Logs", text))
}

@Preview(showBackground = true)
@Composable
private fun LogViewerPreview() {
    AvalancheTheme {
        LogViewerView()
    }
}
