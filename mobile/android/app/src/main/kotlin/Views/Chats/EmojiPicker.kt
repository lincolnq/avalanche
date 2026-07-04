package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items as gridItems
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch

// One entry in the picker grid: a full-width category header or a single emoji.
private sealed interface GridEntry {
    data class Header(val category: EmojiCategory) : GridEntry
    data class Emoji(val value: String) : GridEntry
}

/**
 * The full emoji picker (docs/33), opened from the reaction bar's "+" button.
 * A half-height bottom sheet (draggable to full) with a search field, a
 * recently-used row, a category tab strip, and a categorized scrolling grid.
 * Selecting an emoji calls [onPick] then dismisses. System emoji only. Mirrors
 * mobile/ios/Actnet/Sources/Views/Chats/EmojiPickerView.swift.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EmojiPickerSheet(
    onDismiss: () -> Unit,
    onPick: (String) -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = false)
    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    fun close() {
        scope.launch { sheetState.hide() }.invokeOnCompletion { onDismiss() }
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = LocalAvalancheColors.current.paper,
    ) {
        var query by remember { mutableStateOf("") }
        val recents = remember { EmojiRecents.all(context) }
        val gridState = rememberLazyGridState()

        // Flatten categories into header + emoji entries, remembering the grid
        // index of each header so the tab strip can scroll to it.
        val entries = remember { buildEntries() }
        val headerIndexByCategory = remember {
            entries.withIndex()
                .filter { it.value is GridEntry.Header }
                .associate { (it.value as GridEntry.Header).category to it.index }
        }

        val results = if (query.isBlank()) emptyList() else EmojiData.search(query)

        fun pick(emoji: String) {
            EmojiRecents.record(context, emoji)
            onPick(emoji)
            close()
        }

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .fillMaxHeight(0.92f),
        ) {
            // Search field
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 4.dp),
                placeholder = { Text("Search emoji", color = LocalAvalancheColors.current.muted) },
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null, tint = LocalAvalancheColors.current.muted) },
                trailingIcon = {
                    if (query.isNotEmpty()) {
                        IconButton(onClick = { query = "" }) {
                            Icon(Icons.Filled.Close, contentDescription = "Clear", tint = LocalAvalancheColors.current.muted)
                        }
                    }
                },
                singleLine = true,
                shape = RoundedCornerShape(24.dp),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedContainerColor = LocalAvalancheColors.current.card,
                    unfocusedContainerColor = LocalAvalancheColors.current.card,
                    focusedBorderColor = Color.Transparent,
                    unfocusedBorderColor = Color.Transparent,
                    cursorColor = LocalAvalancheColors.current.brand,
                    focusedTextColor = LocalAvalancheColors.current.ink,
                    unfocusedTextColor = LocalAvalancheColors.current.ink,
                ),
            )

            if (query.isBlank()) {
                // Recently-used row
                if (recents.isNotEmpty()) {
                    Text(
                        text = "Recently Used",
                        style = MaterialTheme.typography.labelSmall,
                        fontWeight = FontWeight.SemiBold,
                        color = LocalAvalancheColors.current.muted,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 2.dp),
                    )
                    LazyRow(
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp),
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        items(recents) { e -> EmojiCell(e) { pick(e) } }
                    }
                }

                // Category tab strip
                Row(modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp)) {
                    EmojiCategory.entries.forEach { cat ->
                        IconButton(
                            onClick = {
                                headerIndexByCategory[cat]?.let { idx ->
                                    scope.launch { gridState.animateScrollToItem(idx) }
                                }
                            },
                            modifier = Modifier.weight(1f),
                        ) {
                            Icon(
                                imageVector = cat.icon,
                                contentDescription = cat.displayName,
                                tint = LocalAvalancheColors.current.muted,
                                modifier = Modifier.size(20.dp),
                            )
                        }
                    }
                }
            }

            // Grid: categorized when browsing, flat results when searching.
            if (query.isNotBlank() && results.isEmpty()) {
                Box(modifier = Modifier.fillMaxWidth().weight(1f), contentAlignment = Alignment.Center) {
                    Text("No emoji", color = LocalAvalancheColors.current.muted)
                }
            } else {
                LazyVerticalGrid(
                    columns = GridCells.Adaptive(minSize = 44.dp),
                    state = gridState,
                    modifier = Modifier.fillMaxWidth().weight(1f).padding(horizontal = 8.dp),
                ) {
                    if (query.isBlank()) {
                        gridItems(
                            items = entries,
                            span = { entry ->
                                if (entry is GridEntry.Header) GridItemSpan(maxLineSpan) else GridItemSpan(1)
                            },
                        ) { entry ->
                            when (entry) {
                                is GridEntry.Header -> Text(
                                    text = entry.category.displayName,
                                    style = MaterialTheme.typography.labelSmall,
                                    fontWeight = FontWeight.SemiBold,
                                    color = LocalAvalancheColors.current.muted,
                                    modifier = Modifier.padding(horizontal = 4.dp, vertical = 6.dp),
                                )
                                is GridEntry.Emoji -> EmojiCell(entry.value) { pick(entry.value) }
                            }
                        }
                    } else {
                        gridItems(results) { e -> EmojiCell(e) { pick(e) } }
                    }
                }
            }
        }
    }
}

@Composable
private fun EmojiCell(emoji: String, onClick: () -> Unit) {
    Box(
        modifier = Modifier
            .size(44.dp)
            .clip(CircleShape)
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(text = emoji, fontSize = 26.sp)
    }
}

private fun buildEntries(): List<GridEntry> {
    val out = mutableListOf<GridEntry>()
    for ((cat, emoji) in EmojiData.byCategory) {
        out.add(GridEntry.Header(cat))
        emoji.forEach { out.add(GridEntry.Emoji(it)) }
    }
    return out
}
