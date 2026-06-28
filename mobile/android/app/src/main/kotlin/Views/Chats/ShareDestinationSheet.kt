package net.theavalanche.app

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

/** An image shared into the app from another app (docs/35), awaiting a chat. */
data class PendingSharedImage(val data: ByteArray, val contentType: String)

// ---------------------------------------------------------------------------
// ShareDestinationSheet
//
// Shown when an image is shared into the app from another app (ACTION_SEND).
// Reuses the conversation list as a destination picker; picking a chat stages
// the shared image in that conversation's composer for review (caption / remove)
// before sending. Android-only for now — the iOS equivalent is blocked on the
// share-extension sandbox until the shared-DB foundation exists (docs/02).
// ---------------------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShareDestinationSheet(
    viewModel: AppViewModel,
    onDismiss: () -> Unit,
) {
    val conversations by viewModel.conversations.collectAsState()
    val accounts by viewModel.accounts.collectAsState()
    val sortedConversations = remember(conversations) {
        conversations.sortedByDescending { it.lastMessageDate?.time ?: Long.MIN_VALUE }
    }

    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text("Share to…") },
                    navigationIcon = {
                        IconButton(onClick = onDismiss) {
                            Icon(imageVector = Icons.Filled.Close, contentDescription = "Cancel")
                        }
                    },
                    colors = TopAppBarDefaults.topAppBarColors(
                        containerColor = LocalAvalancheColors.current.paper,
                        titleContentColor = LocalAvalancheColors.current.ink,
                        navigationIconContentColor = LocalAvalancheColors.current.ink,
                    ),
                )
            },
            containerColor = LocalAvalancheColors.current.paper,
        ) { innerPadding ->
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(innerPadding),
            ) {
                if (sortedConversations.isEmpty()) {
                    Column(
                        modifier = Modifier.fillMaxSize().padding(32.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center,
                    ) {
                        Text(
                            text = "No conversations",
                            color = LocalAvalancheColors.current.ink,
                            fontSize = 18.sp,
                            fontWeight = FontWeight.SemiBold,
                        )
                        Text(
                            text = "Start a conversation first, then share to it.",
                            color = LocalAvalancheColors.current.muted,
                            fontSize = 14.sp,
                        )
                    }
                } else {
                    LazyColumn(
                        modifier = Modifier
                            .fillMaxSize()
                            .background(LocalAvalancheColors.current.paper),
                    ) {
                        items(items = sortedConversations, key = { it.id }) { conversation ->
                            val account = accounts.firstOrNull { it.id == conversation.accountId }
                            ConversationRow(
                                conversation = conversation,
                                account = account,
                                accounts = accounts,
                                unreadCount = viewModel.unreadCount(conversation),
                                isBotConversation = false,
                                previewText = null,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clickable { viewModel.routeSharedImage(conversation) }
                                    .padding(horizontal = 16.dp, vertical = 10.dp),
                            )
                            HorizontalDivider(
                                color = LocalAvalancheColors.current.divider.copy(alpha = 0.5f),
                                modifier = Modifier.padding(start = 76.dp),
                            )
                        }
                    }
                }
            }
        }
    }
}
