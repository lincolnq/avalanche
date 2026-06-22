package net.theavalanche.app

import android.annotation.SuppressLint
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowForwardIos
import androidx.compose.material.icons.filled.Dns
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.launch

// ---------------------------------------------------------------------------
// NetworkView
//
// Mirrors iOS Sources/Views/Network/NetworkView.swift.
// Shows all servers across all accounts, and lists each server's Projects.
// Tapping a project fetches a project token and opens the project URL in a
// WebView sheet.
// ---------------------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NetworkView(
    appViewModel: AppViewModel = viewModel(),
) {
    val accounts by appViewModel.accounts.collectAsState()
    val scope = rememberCoroutineScope()

    // projectsByServer: serverUrl -> list of ProjectInfo
    val projectsByServer = remember { mutableStateMapOf<String, List<ProjectInfo>>() }

    // Loading state — tracks which project is being opened.
    var loadingProjectId by remember { mutableStateOf<String?>(null) }

    // WebView sheet state
    var webViewEntry by remember { mutableStateOf<WebViewEntry?>(null) }

    // Deduplicated, sorted server list across all accounts.
    val allServers: List<ServerInfo> = remember(accounts) {
        accounts.flatMap { it.servers }
            .associateBy { it.id }
            .values
            .sortedBy { it.name }
    }

    // Load projects for each server on first composition.
    LaunchedEffect(allServers) {
        for (server in allServers) {
            val projects = appViewModel.fetchProjects(serverUrl = server.id)
            projectsByServer[server.id] = projects
        }
    }

    // Show WebView sheet when a project token is ready.
    if (webViewEntry != null) {
        val entry = webViewEntry!!
        ProjectWebViewSheet(
            projectName = entry.projectName,
            url = entry.url,
            onDismiss = { webViewEntry = null },
        )
        return
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Network") },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = AvalancheColors.Paper,
                    titleContentColor = AvalancheColors.Ink,
                ),
            )
        },
        containerColor = AvalancheColors.Paper,
    ) { paddingValues ->
        if (allServers.isEmpty()) {
            // Empty state — mirrors iOS ContentUnavailableView.
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(paddingValues)
                    .background(AvalancheColors.Paper),
                contentAlignment = Alignment.Center,
            ) {
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Icon(
                        imageVector = Icons.Filled.Dns,
                        contentDescription = null,
                        tint = AvalancheColors.Muted,
                    )
                    Text(
                        text = "No servers",
                        style = MaterialTheme.typography.titleMedium,
                        color = AvalancheColors.Ink,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        text = "Servers and their Projects will appear here.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = AvalancheColors.Muted,
                    )
                }
            }
        } else {
            LazyColumn(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(paddingValues)
                    .background(AvalancheColors.Paper),
            ) {
                for (server in allServers) {
                    // Section header — server name.
                    item(key = "header-${server.id}") {
                        Text(
                            text = server.name,
                            style = MaterialTheme.typography.labelMedium,
                            color = AvalancheColors.Muted,
                            fontSize = 13.sp,
                            modifier = Modifier
                                .fillMaxWidth()
                                .background(AvalancheColors.Paper)
                                .padding(horizontal = 16.dp, vertical = 8.dp),
                        )
                    }

                    val projects = projectsByServer[server.id]
                    if (projects == null) {
                        // Still loading.
                        item(key = "loading-${server.id}") {
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .background(AvalancheColors.Sand50)
                                    .padding(horizontal = 16.dp, vertical = 12.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                CircularProgressIndicator(
                                    color = AvalancheColors.Brand,
                                    strokeWidth = 2.dp,
                                )
                            }
                        }
                    } else if (projects.isEmpty()) {
                        item(key = "empty-${server.id}") {
                            Text(
                                text = "No Projects",
                                color = AvalancheColors.Muted,
                                style = MaterialTheme.typography.bodyMedium,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .background(AvalancheColors.Sand50)
                                    .padding(horizontal = 16.dp, vertical = 12.dp),
                            )
                        }
                    } else {
                        items(
                            items = projects,
                            key = { "project-${it.id}" },
                        ) { project ->
                            ProjectRow(
                                project = project,
                                isLoading = loadingProjectId == project.id,
                                onClick = {
                                    // Find an account that belongs to this server.
                                    val account = accounts.firstOrNull { acct ->
                                        acct.servers.any { s -> s.id == server.id }
                                    } ?: return@ProjectRow

                                    loadingProjectId = project.id
                                    scope.launch {
                                        runCatching {
                                            val token = appViewModel.requestProjectToken(
                                                accountId = account.id,
                                                projectUrl = project.url,
                                            )
                                            val urlString = "${project.url}?token=$token"
                                            webViewEntry = WebViewEntry(
                                                projectName = project.name,
                                                url = urlString,
                                            )
                                        }.onFailure { error ->
                                            AppLog.error("NetworkView", "Failed to get project token: ${error.message}")
                                        }
                                        loadingProjectId = null
                                    }
                                },
                            )
                            HorizontalDivider(color = AvalancheColors.Sand200)
                        }
                    }

                    // Section footer spacer.
                    item(key = "footer-${server.id}") {
                        Spacer(modifier = Modifier.padding(bottom = 8.dp))
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ProjectRow
// ---------------------------------------------------------------------------

@Composable
private fun ProjectRow(
    project: ProjectInfo,
    isLoading: Boolean,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(AvalancheColors.Sand50)
            .clickable(enabled = !isLoading, onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            Text(
                text = project.name,
                style = MaterialTheme.typography.bodyMedium,
                color = AvalancheColors.Ink,
            )
            Text(
                text = project.description,
                style = MaterialTheme.typography.bodySmall,
                color = AvalancheColors.Muted,
            )
        }
        if (isLoading) {
            CircularProgressIndicator(
                color = AvalancheColors.Brand,
                strokeWidth = 2.dp,
            )
        } else {
            Icon(
                imageVector = Icons.AutoMirrored.Filled.ArrowForwardIos,
                contentDescription = null,
                tint = AvalancheColors.Muted,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// WebViewEntry — holds the data needed to show the project WebView sheet.
// ---------------------------------------------------------------------------

private data class WebViewEntry(
    val projectName: String,
    val url: String,
)

// ---------------------------------------------------------------------------
// ProjectWebViewSheet
//
// Mirrors iOS Sources/Views/Network/ProjectWebView.swift.
// Shown as a full-screen replacement when a project is tapped (the caller
// returns early so this composable fills the whole slot).
// ---------------------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@SuppressLint("SetJavaScriptEnabled")
@Composable
fun ProjectWebViewSheet(
    projectName: String,
    url: String,
    onDismiss: () -> Unit,
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(projectName) },
                navigationIcon = {
                    androidx.compose.material3.TextButton(onClick = onDismiss) {
                        Text("Done", color = AvalancheColors.Brand)
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = AvalancheColors.Paper,
                    titleContentColor = AvalancheColors.Ink,
                ),
            )
        },
    ) { paddingValues ->
        AndroidView(
            factory = { context ->
                WebView(context).apply {
                    settings.javaScriptEnabled = true
                    settings.domStorageEnabled = true
                    webViewClient = WebViewClient()
                    loadUrl(url)
                }
            },
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues),
        )
    }
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

@Preview(showBackground = true)
@Composable
private fun NetworkViewEmptyPreview() {
    AvalancheTheme {
        // Empty state — no accounts, no servers.
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(AvalancheColors.Paper),
            contentAlignment = Alignment.Center,
        ) {
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Icon(
                    imageVector = Icons.Filled.Dns,
                    contentDescription = null,
                    tint = AvalancheColors.Muted,
                )
                Text(
                    text = "No servers",
                    style = MaterialTheme.typography.titleMedium,
                    color = AvalancheColors.Ink,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = "Servers and their Projects will appear here.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = AvalancheColors.Muted,
                )
            }
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun ProjectRowPreview() {
    AvalancheTheme {
        Column {
            ProjectRow(
                project = ProjectInfo(
                    name = "Voter Registration Drive",
                    url = "https://example.com/projects/voter-reg",
                    description = "Sign up new voters in the district.",
                ),
                isLoading = false,
                onClick = {},
            )
            HorizontalDivider(color = AvalancheColors.Sand200)
            ProjectRow(
                project = ProjectInfo(
                    name = "Phone Banking",
                    url = "https://example.com/projects/phone-bank",
                    description = "Call volunteers for the event.",
                ),
                isLoading = true,
                onClick = {},
            )
        }
    }
}
