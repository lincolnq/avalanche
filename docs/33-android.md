# Android App Implementation Guide

> **Status: Design document — not yet implemented.**
> The iOS app (`mobile/ios/`) is the reference implementation. This guide specifies how to build the Android counterpart using the same structure.
> See `docs/02-todos-deferred.md` for the itemized checklist.

---

## 1. Overview

The Android app is a structural mirror of the iOS app (Swift/SwiftUI → Kotlin/Jetpack Compose). Everything above the UniFFI boundary is rewritten in Kotlin with Android-idiomatic APIs; everything below it — encryption, storage, networking — is the same Rust code.

**Goals:**
- Kotlin/Jetpack Compose UI, matching the iOS feature set screen-for-screen
- Same `ActnetService` / `AppCoreInterface` / mock-vs-real split as iOS
- `AppViewModel` mirrors `AppState.swift` in state surface and method names
- Cross-platform interop: iOS and Android encrypt/decrypt each other's messages via the shared Rust core
- Minimum SDK: API 26 (Android 8.0), matching Signal's floor
- Target SDK: API 35

---

## 2. iOS → Android Mapping

Quick reference for contributors who know the iOS code.

| Concept | iOS (Swift) | Android (Kotlin) |
|---------|-------------|-----------------|
| State container | `AppState: ObservableObject` | `AppViewModel : AndroidViewModel` |
| Reactive field | `@Published var foo: T` | `private val _foo = MutableStateFlow<T>(…)` + `val foo = _foo.asStateFlow()` |
| Observe in composable | `@EnvironmentObject var appState` | `val vm: AppViewModel = viewModel()` + `.collectAsState()` |
| Background / IO work | `Task.detached { … }.value` | `viewModelScope.launch(Dispatchers.IO) { … }` |
| WebSocket loop | `Task { }` stored in dict | `viewModelScope.launch { }` stored as `Job` |
| Persistence | `UserDefaults` | Jetpack `DataStore<Preferences>` |
| Navigation | `NavigationStack` + `.navigationDestination` | `NavHost` + `NavController` + `composable("route")` |
| Tabs | `TabView` | `Scaffold` + `NavigationBar` + `NavigationBarItem` |
| Sheet | `.sheet(isPresented:)` | `ModalBottomSheet` composable |
| Serialization | `Codable` (Swift) | `kotlinx.serialization` (`@Serializable`) |
| DB file directory | `~/Library/Application Support/actnet/` | `context.filesDir/actnet/` |
| Native library | `AppCoreFFI.xcframework` | `libapp_core.so` in `app/src/main/jniLibs/<abi>/` |
| Native lib loader | Swift auto-loads `.xcframework` | UniFFI uses JNA; `.so` placed in `jniLibs/` |
| QR scanner | `AVFoundation` + `AVCaptureSession` | CameraX + ML Kit `BarcodeScanning` |
| WebView | `WKWebView` | Android `WebView` via `AndroidView` composable |
| Secure storage (future) | Secure Enclave | Android Keystore |

---

## 3. Directory Structure

```
mobile/android/
├── Generated/                              # from `make bindings` — do not edit
│   └── uniffi/
│       └── app_core/
│           └── app_core.kt                 # UniFFI-generated Kotlin bindings
├── app/
│   ├── build.gradle.kts
│   ├── src/
│   │   ├── main/
│   │   │   ├── AndroidManifest.xml
│   │   │   ├── kotlin/org/actnet/app/
│   │   │   │   ├── ActnetApplication.kt        # Application subclass
│   │   │   │   ├── MainActivity.kt             # setContent { ActnetApp() }
│   │   │   │   ├── AppViewModel.kt             # ← mirrors AppState.swift
│   │   │   │   ├── models/
│   │   │   │   │   ├── Account.kt
│   │   │   │   │   ├── Conversation.kt
│   │   │   │   │   ├── Message.kt              # includes DeliveryStatus enum
│   │   │   │   │   ├── ProjectInfo.kt
│   │   │   │   │   └── InviteToken.kt
│   │   │   │   ├── services/
│   │   │   │   │   ├── ActnetService.kt        # interface (mirrors ActnetService.swift)
│   │   │   │   │   ├── AppCoreInterface.kt     # interface (mirrors AppCoreProtocol)
│   │   │   │   │   ├── MockActnetService.kt    # mock impl for UI dev (no Rust needed)
│   │   │   │   │   └── DevServerActnetService.kt  # wraps UniFFI AppCore
│   │   │   │   └── ui/
│   │   │   │       ├── ActnetApp.kt            # NavHost root (mirrors RootView.swift)
│   │   │   │       ├── MainScreen.kt           # Scaffold + BottomNav (mirrors MainTabView)
│   │   │   │       ├── common/
│   │   │   │       │   ├── AccountAvatar.kt
│   │   │   │       │   └── DevSettingsScreen.kt
│   │   │   │       ├── onboarding/
│   │   │   │       │   ├── SplashScreen.kt
│   │   │   │       │   ├── IdentityPickerScreen.kt
│   │   │   │       │   ├── NewAccountScreen.kt
│   │   │   │       │   ├── JoiningServerScreen.kt
│   │   │   │       │   ├── InviteLinkEntryScreen.kt
│   │   │   │       │   └── QRScannerScreen.kt
│   │   │   │       ├── chats/
│   │   │   │       │   ├── ChatsScreen.kt
│   │   │   │       │   ├── ConversationScreen.kt
│   │   │   │       │   ├── MessageBubble.kt
│   │   │   │       │   └── ComposeMessageScreen.kt
│   │   │   │       ├── calls/
│   │   │   │       │   └── CallsScreen.kt      # placeholder
│   │   │   │       └── network/
│   │   │   │           ├── NetworkScreen.kt
│   │   │   │           └── ProjectWebScreen.kt
│   │   │   ├── jniLibs/                        # from `make android-ndk`
│   │   │   │   ├── arm64-v8a/
│   │   │   │   │   └── libapp_core.so          # physical devices
│   │   │   │   └── x86_64/
│   │   │   │       └── libapp_core.so          # emulator
│   │   │   └── res/
│   │   │       └── values/
│   │   │           └── strings.xml
│   │   └── test/
│   │       └── kotlin/org/actnet/app/
│   │           └── MockServiceTest.kt
├── build.gradle.kts                        # root Gradle config
├── settings.gradle.kts                     # includes :app, declares Generated source dir
└── gradle/
    └── libs.versions.toml                  # version catalog
```

---

## 4. Build System

### 4.1 Prerequisites

| Tool | Install |
|------|---------|
| Android Studio (Ladybug or later) | [developer.android.com/studio](https://developer.android.com/studio) |
| Android NDK r27+ | Android Studio → SDK Manager → SDK Tools → NDK |
| `ANDROID_NDK_HOME` env var | Point to NDK directory |
| `cargo-ndk` | `cargo install cargo-ndk` |

### 4.2 Makefile targets to add

Add these to `Makefile` alongside the existing `ios` and `bindings` targets:

```makefile
.PHONY: android android-ndk   # add to existing .PHONY line

android-ndk:
	cd core && cargo ndk \
	    -t arm64-v8a \
	    -t x86_64 \
	    -o ../mobile/android/app/src/main/jniLibs \
	    build -p app-core --release

android: bindings android-ndk
```

`make android` is the Android equivalent of `make ios`: generates Kotlin bindings and compiles the Rust `.so` files for each ABI. The Gradle build (`./gradlew assembleDebug`) is a separate step run from Android Studio or CI.

### 4.3 `settings.gradle.kts`

The UniFFI-generated Kotlin lives outside the standard `app/src/main` tree. Tell Gradle where to find it:

```kotlin
rootProject.name = "Actnet"
include(":app")

// The Generated/ directory is at the same level as app/.
// Its contents are added as a source set in app/build.gradle.kts.
```

### 4.4 `app/build.gradle.kts` (key excerpts)

```kotlin
plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.compose.compiler)
}

android {
    namespace = "org.actnet.app"
    compileSdk = 35
    defaultConfig {
        applicationId = "org.actnet.app"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }
    sourceSets {
        getByName("main") {
            // Pull in UniFFI-generated Kotlin alongside hand-written sources
            kotlin.srcDir("../../Generated")
        }
    }
}

dependencies {
    val composeBom = platform(libs.compose.bom)
    implementation(composeBom)
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.navigation)
    implementation(libs.viewmodel.compose)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.datastore.preferences)
    implementation(libs.jna) { artifact { type = "aar" } }  // UniFFI runtime
    implementation(libs.camerax.core)
    implementation(libs.camerax.camera2)
    implementation(libs.camerax.lifecycle)
    implementation(libs.camerax.view)
    implementation(libs.mlkit.barcode)
}
```

### 4.5 `gradle/libs.versions.toml`

```toml
[versions]
agp = "8.7.0"
kotlin = "2.0.21"
compose-bom = "2024.09.00"
navigation-compose = "2.8.0"
lifecycle = "2.8.0"
kotlinx-serialization = "1.7.1"
datastore = "1.1.1"
jna = "5.14.0"
camerax = "1.3.4"
mlkit-barcode = "17.3.0"

[plugins]
android-application = { id = "com.android.application", version.ref = "agp" }
kotlin-android = { id = "org.jetbrains.kotlin.android", version.ref = "kotlin" }
kotlin-serialization = { id = "org.jetbrains.kotlin.plugin.serialization", version.ref = "kotlin" }
compose-compiler = { id = "org.jetbrains.kotlin.plugin.compose", version.ref = "kotlin" }

[libraries]
compose-bom = { group = "androidx.compose", name = "compose-bom", version.ref = "compose-bom" }
compose-ui = { group = "androidx.compose.ui", name = "ui" }
compose-material3 = { group = "androidx.compose.material3", name = "material3" }
compose-navigation = { group = "androidx.navigation", name = "navigation-compose", version.ref = "navigation-compose" }
viewmodel-compose = { group = "androidx.lifecycle", name = "lifecycle-viewmodel-compose", version.ref = "lifecycle" }
kotlinx-serialization-json = { group = "org.jetbrains.kotlinx", name = "kotlinx-serialization-json", version.ref = "kotlinx-serialization" }
datastore-preferences = { group = "androidx.datastore", name = "datastore-preferences", version.ref = "datastore" }
jna = { group = "net.java.dev.jna", name = "jna", version.ref = "jna" }
camerax-core = { group = "androidx.camera", name = "camera-core", version.ref = "camerax" }
camerax-camera2 = { group = "androidx.camera", name = "camera-camera2", version.ref = "camerax" }
camerax-lifecycle = { group = "androidx.camera", name = "camera-lifecycle", version.ref = "camerax" }
camerax-view = { group = "androidx.camera", name = "camera-view", version.ref = "camerax" }
mlkit-barcode = { group = "com.google.mlkit", name = "barcode-scanning", version.ref = "mlkit-barcode" }
```

---

## 5. UniFFI / Native Library Integration

### How it works

`make bindings` (already implemented in the top-level `Makefile`) generates `mobile/android/Generated/uniffi/app_core/app_core.kt`. This file is a complete Kotlin wrapper over the Rust `AppCore` struct; it uses **JNA** (Java Native Access) to load `libapp_core.so` at runtime and call into it via the C ABI.

`make android-ndk` compiles `libapp_core.so` for each Android ABI using `cargo-ndk` and places the output in `app/src/main/jniLibs/<abi>/`. Android's packaging step bundles these `.so` files into the APK automatically.

### Calling convention

UniFFI-generated Kotlin methods are synchronous and block the calling thread — identical to iOS. Always call from `Dispatchers.IO`:

```kotlin
viewModelScope.launch(Dispatchers.IO) {
    val core = service.createAccount(serverUrl, dbPath, dbKey)
    withContext(Dispatchers.Main) {
        cores[did] = core
        _accounts.update { it + newAccount }
    }
}
```

The `receiveMessagesWs()` method blocks until a message arrives (or a timeout elapses). Run it in a loop inside `Dispatchers.IO`, checking `currentCoroutineContext().isActive` for cancellation.

---

## 6. Service & Interface Layer

### 6.1 `ActnetService` interface

Mirrors `ActnetService.swift` exactly:

```kotlin
interface ActnetService {
    fun createAccount(serverUrl: String, dbPath: String, dbKey: String): AppCoreInterface
    fun login(dbPath: String, dbKey: String): AppCoreInterface
}
```

### 6.2 `AppCoreInterface`

Mirrors `AppCoreProtocol` from `mobile/ios/Actnet/Sources/Services/ActnetService.swift`. All methods are synchronous (FFI constraint):

```kotlin
interface AppCoreInterface {
    fun did(): String
    fun deviceId(): UInt
    fun sendDm(recipientDid: String, recipientDeviceId: UInt, plaintext: String, sentAtMs: Long)
    fun sendReadReceipt(recipientDid: String, recipientDeviceId: UInt, timestamps: List<Long>)
    fun receiveMessages(): List<DecryptedMessage>
    fun receiveMessagesWs(): List<DecryptedMessage>
    fun fetchProjects(): List<ProjectInfoFfi>
    fun requestProjectToken(projectUrl: String): String
    fun saveMessage(msg: StoredMessageFfi)
    fun loadMessages(conversationId: String): List<StoredMessageFfi>
    fun markMessagesRead(conversationId: String, upToSentAtMs: Long): ULong
    fun unreadCount(conversationId: String): ULong
    fun drainReceiptUpdates(): List<DeliveryStatusUpdate>
    fun getAccountInfo(did: String): AccountInfoFfi
}
```

`DecryptedMessage`, `StoredMessageFfi`, `DeliveryStatusUpdate`, `AccountInfoFfi`, `ProjectInfoFfi` are the UniFFI-generated Kotlin data classes; they are identical in structure to the Swift Records.

### 6.3 `MockAppCore`

Mirrors `MockActnetService.swift` behavior:

- Pending messages stored in a `LinkedList<DecryptedMessage>` guarded by `ReentrantLock`
- `sendDm(…)`: `Thread.sleep(100)` to simulate network, then schedules an echo reply 1.5 s later via a background thread
- `receiveMessagesWs()`: polls `pendingMessages` every 40 ms for up to 2 s, then returns whatever it has (including empty list)
- `createAccount` seeds a few placeholder conversations identical to the iOS mock

`MockActnetService` simply returns a `MockAppCore()` from `createAccount` and `login`. No Rust library is needed in mock mode — identical to iOS.

### 6.4 `DevServerActnetService`

Wraps the UniFFI-generated `AppCore` class:

```kotlin
class DevServerActnetService : ActnetService {
    override fun createAccount(serverUrl: String, dbPath: String, dbKey: String): AppCoreInterface =
        AppCore.createAccount(serverUrl = serverUrl, dbPath = dbPath, dbKey = dbKey)

    override fun login(dbPath: String, dbKey: String): AppCoreInterface =
        AppCore.login(dbPath = dbPath, dbKey = dbKey)
}
```

`AppCore` here is the UniFFI-generated class from `app_core.kt`. It already implements every method in `AppCoreInterface` with matching signatures (Kotlin name-mangling may require `@JvmName` annotations if needed).

### 6.5 `PasskeyManager`

Mirrors `PasskeyManager.swift`. Manages WebAuthn passkey registration and authentication using Android's Credential Manager (`androidx.credentials`).

```kotlin
class PasskeyManager(private val activity: Activity) {
    companion object {
        const val RELYING_PARTY = "theavalanche.net"
        val PRF_SALT = "actnet-recovery-v1".toByteArray()
    }

    data class RegistrationResult(val recoveryKey: ByteArray, val userHandle: ByteArray)
    data class AuthenticationResult(val recoveryKey: ByteArray, val did: String)

    suspend fun register(did: String): RegistrationResult {
        // Uses CredentialManager createCredential() with PRF extension
        // (or synthetic password fallback on Android versions without PRF).
        // Derives 32-byte recovery key from PRF output + PRF_SALT.
        TODO("WebAuthn registration via CredentialManager")
    }

    suspend fun authenticate(): AuthenticationResult {
        // Uses CredentialManager getCredential() to retrieve existing passkey
        // and re-derive the same recovery key.
        TODO("WebAuthn authentication via CredentialManager")
    }
}
```

> **Platform note:** Android Credential Manager does not yet support the PRF extension uniformly across OEMs. On devices without PRF, fall back to a synthetic password derived from the credential ID + PRF_SALT, or prompt for a separate recovery phrase. Document the chosen fallback in `docs/33-identity-auth-recovery.md`.

---

## 7. State Management — `AppViewModel`

`AppViewModel` is the direct counterpart of `AppState.swift`. It owns all reactive state, all `AppCoreInterface` instances, and all WebSocket coroutine jobs.

```kotlin
class AppViewModel(application: Application) : AndroidViewModel(application) {

    // --- Reactive state (mirrors @Published vars in AppState.swift) ---
    private val _accounts = MutableStateFlow<List<Account>>(emptyList())
    val accounts: StateFlow<List<Account>> = _accounts.asStateFlow()

    private val _isOnboarding = MutableStateFlow(true)
    val isOnboarding: StateFlow<Boolean> = _isOnboarding.asStateFlow()

    private val _conversations = MutableStateFlow<List<Conversation>>(emptyList())
    val conversations: StateFlow<List<Conversation>> = _conversations.asStateFlow()

    private val _messagesByConversation = MutableStateFlow<Map<String, List<Message>>>(emptyMap())
    val messagesByConversation: StateFlow<Map<String, List<Message>>> = _messagesByConversation.asStateFlow()

    private val _serviceMode = MutableStateFlow(ServiceMode.MOCK)
    val serviceMode: StateFlow<ServiceMode> = _serviceMode.asStateFlow()

    // --- Internal (mirrors private vars in AppState.swift) ---
    private val cores = mutableMapOf<String, AppCoreInterface>()
    private val wsJobs = mutableMapOf<String, Job>()  // mirrors wsLoopTasks
    private var service: ActnetService = MockActnetService()

    // Display name resolution cache (mirrors displayNameCache / displayNameInFlight)
    private val displayNameCache = mutableMapOf<String, String>()
    private val displayNameInFlight = mutableSetOf<String>()

    // Navigation / lifecycle state (mirrors iOS navigation + app-active tracking)
    private val _navigateToConversation = MutableStateFlow<Conversation?>(null)
    val navigateToConversation: StateFlow<Conversation?> = _navigateToConversation.asStateFlow()

    private val _currentConversationId = MutableStateFlow<String?>(null)
    val currentConversationId: StateFlow<String?> = _currentConversationId.asStateFlow()

    private val _isAppActive = MutableStateFlow(true)
    val isAppActive: StateFlow<Boolean> = _isAppActive.asStateFlow()

    // Deep link state (mirrors pendingInviteToken)
    private val _pendingInviteToken = MutableStateFlow<String?>(null)
    val pendingInviteToken: StateFlow<String?> = _pendingInviteToken.asStateFlow()

    // --- DataStore for persistence (mirrors UserDefaults) ---
    private val prefs = application.applicationContext
        .createDataStore(name = "actnet_prefs")

    init {
        restoreAccounts()
        loadConversationsFromStore()
    }

    // mirrors AppState.restoreAccounts()
    fun restoreAccounts() {
        viewModelScope.launch(Dispatchers.IO) {
            // Read persisted accounts from DataStore
            // Restore cores via service.login(…)
            // Start WS loops
        }
    }

    // mirrors AppState.loadConversationsFromStore()
    private fun loadConversationsFromStore() {
        viewModelScope.launch(Dispatchers.IO) {
            for ((accountId, core) in cores) {
                val summaries = core.loadConversations()
                val convs = summaries.map { summary ->
                    Conversation(
                        id = summary.conversationId,
                        title = summary.title,
                        accountId = accountId,
                        serverUrl = summary.serverUrl,
                        recipientDid = summary.recipientDid,
                        lastMessageDateMs = summary.lastMessageDateMs
                    )
                }
                _conversations.update { it + convs }
                // Load preview messages for each conversation
                convs.forEach { conv ->
                    loadMessagesFromStore(conv.id, accountId)
                }
            }
        }
    }

    // mirrors AppState.handleDeepLink(_:)
    fun handleDeepLink(url: String) {
        if (url.startsWith("actnet://invite/")) {
            val token = url.removePrefix("actnet://invite/")
            _pendingInviteToken.value = token
        }
    }

    // mirrors AppState.displayName(for:accountId:)
    fun displayName(did: String, accountId: String): String {
        return displayNameCache[did] ?: did  // fallback to raw DID
    }

    // mirrors AppState.resolveDisplayName(did:accountId:)
    private fun resolveDisplayName(did: String, accountId: String) {
        if (displayNameInFlight.contains(did)) return
        displayNameInFlight.add(did)
        viewModelScope.launch(Dispatchers.IO) {
            val core = cores[accountId] ?: return@launch
            try {
                val info = core.getAccountInfo(did)
                val name = info.profileBlob?.let { blob ->
                    // decrypt / parse profile blob to extract display name
                    // TODO: wire up profile decryption once Rust FFI exposes it
                    null
                } ?: did
                withContext(Dispatchers.Main) {
                    displayNameCache[did] = name
                    displayNameInFlight.remove(did)
                    // trigger recomposition
                    _accounts.update { it.toList() }
                }
            } catch (_: Exception) {
                displayNameInFlight.remove(did)
            }
        }
    }

    // mirrors AppState.refreshContactProfile(did:accountId:)
    fun refreshContactProfile(did: String, accountId: String) {
        resolveDisplayName(did, accountId)
    }

    // mirrors AppState.createAccount(serverUrl:displayName:)
    fun createAccount(serverUrl: String, displayName: String) {
        viewModelScope.launch(Dispatchers.IO) {
            val dbPath = newDbPath()
            val core = service.createAccount(serverUrl, dbPath, DB_KEY_PLACEHOLDER)
            withContext(Dispatchers.Main) {
                cores[core.did()] = core
                _accounts.update { it + Account(id = core.did(), displayName = displayName) }
                _isOnboarding.value = false
                persistAccounts()
                startWsLoop(core.did())
            }
        }
    }

    // mirrors AppState.sendMessage(…)
    fun sendMessage(conversationId: String, text: String, recipientDid: String, accountId: String) {
        val sentAtMs = System.currentTimeMillis()
        // Optimistic UI update on Main, then IO send
        viewModelScope.launch {
            val optimistic = Message(…, deliveryStatus = DeliveryStatus.SENDING)
            appendMessage(conversationId, optimistic)
            viewModelScope.launch(Dispatchers.IO) {
                cores[accountId]?.sendDm(recipientDid, recipientDeviceId, text, sentAtMs)
                withContext(Dispatchers.Main) {
                    updateDeliveryStatus(conversationId, sentAtMs, DeliveryStatus.SENT)
                }
            }
        }
    }

    private fun startWsLoop(accountId: String) {
        wsJobs[accountId]?.cancel()
        wsJobs[accountId] = viewModelScope.launch(Dispatchers.IO) {
            while (isActive) {
                val core = cores[accountId] ?: break
                try {
                    val messages = core.receiveMessagesWs()
                    if (messages.isNotEmpty()) {
                        withContext(Dispatchers.Main) { ingestMessages(messages, accountId) }
                    }
                    val updates = core.drainReceiptUpdates()
                    if (updates.isNotEmpty()) {
                        withContext(Dispatchers.Main) { applyReceiptUpdates(updates) }
                    }
                } catch (e: Exception) {
                    delay(2_000)  // mirrors 2s backoff in iOS wsLoop
                }
            }
        }
    }

    // Called from MainActivity.onResume() / onPause() to mirror iOS scene phase
    fun setAppActive(active: Boolean) {
        _isAppActive.value = active
    }

    companion object {
        private const val DB_KEY_PLACEHOLDER = "dev-placeholder-key"
    }
}
```

`ServiceMode` is a Kotlin enum (`MOCK`, `DEV_SERVER`) mirroring the iOS `ServiceMode` enum. Switching modes clears all state and restarts — identical to `AppState.switchMode(_:)`.

---

## 8. Models

Each model mirrors the corresponding iOS Swift struct. Use `kotlinx.serialization` (`@Serializable`) instead of `Codable`.

### `Account.kt`
```kotlin
data class Account(
    val id: String,             // DID
    val displayName: String,
    val avatarData: ByteArray? = null,
    val servers: List<ServerInfo> = emptyList(),
)
```

### `Conversation.kt`
```kotlin
@Serializable
data class Conversation(
    val id: String,
    val title: String,
    val accountId: String,
    val serverUrl: String,
    val recipientDid: String? = null,
    // lastMessage excluded from serialization (plaintext security — mirrors iOS)
    @Transient val lastMessage: String? = null,
    val lastMessageDateMs: Long? = null,
    val isGroup: Boolean = false,
)
```

### `Message.kt`
```kotlin
enum class DeliveryStatus(val raw: Int) {
    SENDING(0), SENT(1), DELIVERED(2), READ(3)
}

data class Message(
    val id: String,
    val conversationId: String,
    val senderAccountId: String,
    val body: String,
    val sentAtMs: Long,
    val editedAtMs: Long? = null,
    val readAtMs: Long? = null,
    var deliveryStatus: DeliveryStatus = DeliveryStatus.SENDING,
) {
    val isEdited get() = editedAtMs != null
    val isRead get() = readAtMs != null
}
```

### `ProjectInfo.kt`
```kotlin
data class ProjectInfo(
    val name: String,
    val url: String,
    val description: String,
) {
    val id get() = url
}
```

### `InviteToken.kt`
```kotlin
data class InviteToken(
    val serverUrl: String,
    val serverName: String,
    val token: String,
) {
    val id get() = token
}
```

---

## 9. UI / Jetpack Compose

### 9.1 Root routing (mirrors `RootView.swift`)

```kotlin
@Composable
fun ActnetApp(vm: AppViewModel = viewModel()) {
    val isOnboarding by vm.isOnboarding.collectAsState()
    MaterialTheme {
        if (isOnboarding) {
            SplashScreen(vm)
        } else {
            MainScreen(vm)
        }
    }
}
```

### 9.2 Tab scaffold (mirrors `MainTabView.swift`)

Three tabs: Calls, Chats, Network. Same ordering and icons as iOS.

```kotlin
@Composable
fun MainScreen(vm: AppViewModel) {
    val navController = rememberNavController()
    var selectedTab by remember { mutableIntStateOf(1) }  // Chats default

    Scaffold(
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    selected = selectedTab == 0,
                    onClick = { selectedTab = 0 },
                    icon = { Icon(Icons.Filled.Call, contentDescription = "Calls") },
                    label = { Text("Calls") },
                )
                NavigationBarItem(
                    selected = selectedTab == 1,
                    onClick = { selectedTab = 1 },
                    icon = { Icon(Icons.Filled.Chat, contentDescription = "Chats") },
                    label = { Text("Chats") },
                )
                NavigationBarItem(
                    selected = selectedTab == 2,
                    onClick = { selectedTab = 2 },
                    icon = { Icon(Icons.Filled.Hub, contentDescription = "Network") },
                    label = { Text("Network") },
                )
            }
        }
    ) { innerPadding ->
        when (selectedTab) {
            0 -> CallsScreen(vm, Modifier.padding(innerPadding))
            1 -> ChatsNavGraph(vm, Modifier.padding(innerPadding))
            2 -> NetworkNavGraph(vm, Modifier.padding(innerPadding))
        }
    }
}
```

### 9.3 Navigation within tabs

Each tab that has sub-screens (Chats, Network) uses its own `NavHost`:

```kotlin
@Composable
fun ChatsNavGraph(vm: AppViewModel, modifier: Modifier) {
    val navController = rememberNavController()
    NavHost(navController, startDestination = "chats", modifier = modifier) {
        composable("chats") {
            ChatsScreen(vm, onOpenConversation = { convId ->
                navController.navigate("conversation/$convId")
            }, onCompose = {
                navController.navigate("compose")
            })
        }
        composable("conversation/{conversationId}") { backStack ->
            val convId = backStack.arguments!!.getString("conversationId")!!
            ConversationScreen(vm, conversationId = convId)
        }
        composable("compose") {
            ComposeMessageScreen(vm, onDone = { navController.popBackStack() })
        }
    }
}
```

### 9.4 Onboarding screens

Mirrors the iOS onboarding flow from `docs/30-mobile-ux.md` exactly:

- `SplashScreen` — two buttons: "Scan QR code" and "Enter invite link"
- `InviteLinkEntryScreen` — text field accepting `actnet://invite/<server>/<token>` deep links
- `IdentityPickerScreen` — list of existing accounts + "Create fresh identity" — shown when accounts already exist
- `NewAccountScreen` — display name field, optional avatar, calls `vm.createAccount(…)`
- `JoiningServerScreen` — join a new server with an existing account
- `PasskeyExplainerScreen` — explains WebAuthn passkey creation before registration
- `RecoveryExplainerScreen` — explains recovery key backup after account creation
- `RecoveryConsoleScreen` — nerdly scrolling log for device replacement / recovery operations
- `QRScannerScreen` — CameraX preview with ML Kit `BarcodeScanning.getClient()` overlay

Android receives `actnet://` deep links via the intent filter in `AndroidManifest.xml` (see Section 11). The `MainActivity` intercepts the intent and passes the URL to `AppViewModel`.

### 9.5 `ChatsScreen`

Mirrors `ChatsView.swift`:
- `LazyColumn` of conversations sorted by `lastMessageDateMs` descending
- Each row shows title, last message preview, timestamp, unread badge
- FAB or top-right action to open `ComposeMessageScreen`
- Long-press row → context menu (future: delete, mute)
- Dev settings accessible via overflow menu icon (mirrors gear icon in iOS `ChatsView`)

### 9.6 `ConversationScreen`

Mirrors `ConversationView.swift`:
- `LazyColumn` of messages, reversed scroll (newest at bottom)
- Calls `vm.loadMessagesFromStore(conversationId, accountId)` on first appear
- Marks messages read via `vm.markAllMessagesRead(…)` on appear
- `TextField` + send button at bottom (mirrors `HStack` input in iOS)
- Auto-scroll to bottom on new message (mirrors `ScrollViewReader` in iOS)
- `RecoveryKeyBanner` stub at top (always hidden initially, mirrors iOS)

### 9.7 `MessageBubble`

Mirrors `MessageBubble.swift` exactly:

- **Sent**: blue background, right-aligned, timestamp + delivery status icon
- **Received**: gray background, left-aligned, timestamp
- Delivery status icons:
  - `SENDING` → clock icon (⏱)
  - `SENT` → single checkmark (✓)
  - `DELIVERED` → double checkmark, gray (✓✓)
  - `READ` → double checkmark, blue (✓✓)
- Show "Edited" label when `isEdited` is true

### 9.8 `NetworkScreen` and `ProjectWebScreen`

`NetworkScreen` mirrors `NetworkView.swift`:
- Groups projects by server URL
- Async load via `vm.fetchProjects(serverUrl)`
- Tap project → request token via `vm.requestProjectToken(…)` → open `ProjectWebScreen`

`ProjectWebScreen` uses `AndroidView { WebView(context) }` — the Android equivalent of `WKWebView`. Load the project URL with the auth token injected as a cookie or query parameter (mirror iOS behavior).

### 9.9 `DevSettingsScreen`

Mirrors `DevSettingsView.swift`:
- Toggle between Mock and DevServer modes (clears all state on switch)
- Show account count, conversation count
- Show current server URL

### 9.10 `MyQRCodeScreen`

Mirrors `MyQRCodeView.swift`:
- Generates a QR code from `https://go.theavalanche.net/invite/<token>` for the active account
- Displays account display name and "Scan to start a conversation" helper text
- Share button using Android's `ShareCompat.IntentBuilder`

```kotlin
@Composable
fun MyQRCodeScreen(vm: AppViewModel) {
    val account = vm.accounts.value.firstOrNull()
    val server = account?.servers?.firstOrNull()
    if (account != null && server != null) {
        val token = remember { generateInviteToken(server.url, account.id) }
        val url = "https://go.theavalanche.net/invite/$token"
        // Use ZXing or ML Kit to generate QR bitmap
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            QRCodeImage(url, modifier = Modifier.size(250.dp))
            Text(account.displayName, style = MaterialTheme.typography.titleLarge)
            Text("Scan to start a conversation", style = MaterialTheme.typography.bodyMedium, color = Color.Gray)
            Button(onClick = {
                val intent = ShareCompat.IntentBuilder(context)
                    .setType("text/plain")
                    .setText(url)
                    .intent
                context.startActivity(Intent.createChooser(intent, "Share invite link"))
            }) {
                Text("Share Invite Link")
            }
        }
    }
}
```

### 9.11 Local Notifications

Mirrors `NotificationPresenter.swift`. Android does not need a custom presenter for foreground notifications in the same way iOS does, because the system notification shade is always available. However, the following behaviors should match:

- **App active + viewing current conversation** → suppress notification sound/banner (badge still updates via existing unread count)
- **App active + viewing different conversation** → post notification via `NotificationManager`
- **App backgrounded** → post notification via `NotificationManager`
- **Sound throttling** — max one sound per conversation per 3 seconds during bursts

```kotlin
object NotificationPresenter {
    private val lastSoundAt = mutableMapOf<String, Long>()
    private const val SOUND_THROTTLE_MS = 3000L

    fun present(
        context: Context,
        message: Message,
        conversation: Conversation,
        senderDisplayName: String,
        isAppActive: Boolean,
        currentConversationId: String?
    ) {
        updateBadge(context, conversation.accountId)

        if (message.body.isBlank()) return

        // Suppress when viewing this conversation
        if (isAppActive && currentConversationId == conversation.id) return

        val now = System.currentTimeMillis()
        val last = lastSoundAt[conversation.id] ?: 0
        val shouldSound = (now - last) > SOUND_THROTTLE_MS
        if (shouldSound) lastSoundAt[conversation.id] = now

        val notification = NotificationCompat.Builder(context, MESSAGE_CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(senderDisplayName)
            .setContentText(message.body)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setOnlyAlertOnce(!shouldSound)
            .build()

        NotificationManagerCompat.from(context).notify(conversation.id.hashCode(), notification)
    }

    private fun updateBadge(context: Context, accountId: String) {
        // Update launcher badge via ShortcutManager or Notification badge
    }
}
```

---

## 10. Persistence

### DataStore (replaces UserDefaults)

```kotlin
// Keys
private val KEY_ACCOUNTS = stringPreferencesKey("accounts_json")
private val KEY_SERVICE_MODE = stringPreferencesKey("service_mode")
private val KEY_CONVERSATIONS = stringPreferencesKey("conversations_json")

// Write
suspend fun persistAccounts() {
    prefs.edit { prefs ->
        prefs[KEY_ACCOUNTS] = Json.encodeToString(persistedAccounts)
    }
}

// Read
val storedJson = prefs.data.first()[KEY_ACCOUNTS] ?: return
val accounts = Json.decodeFromString<List<PersistedAccount>>(storedJson)
```

`Conversation.lastMessage` is excluded from JSON serialization (`@Transient`) for the same reason as iOS: no plaintext body on disk outside the encrypted SQLCipher DB.

### SQLCipher DB path

```kotlin
private fun dbPath(uuid: String): String =
    File(application.filesDir, "actnet/account-$uuid.db").absolutePath
```

The Rust core handles all SQLCipher I/O. Android only provides the path and the key (placeholder for now; Android Keystore integration is a future TODO matching the iOS Secure Enclave TODO).

---

## 11. `AndroidManifest.xml`

```xml
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">

    <uses-permission android:name="android.permission.INTERNET" />
    <uses-permission android:name="android.permission.CAMERA" />
    <!-- POST_NOTIFICATIONS required on API 33+ for push -->
    <uses-permission android:name="android.permission.POST_NOTIFICATIONS" />

    <application
        android:name=".ActnetApplication"
        android:label="@string/app_name"
        android:theme="@style/Theme.Actnet">

        <activity
            android:name=".MainActivity"
            android:exported="true"
            android:launchMode="singleTop">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>

            <!-- Deep link: actnet://invite/<server>/<token> -->
            <intent-filter>
                <action android:name="android.intent.action.VIEW" />
                <category android:name="android.intent.category.DEFAULT" />
                <category android:name="android.intent.category.BROWSABLE" />
                <data android:scheme="actnet" android:host="invite" />
            </intent-filter>
        </activity>

        <!-- FCM push (stub — implement when push notifications are added) -->
        <service
            android:name=".FcmService"
            android:exported="false">
            <intent-filter>
                <action android:name="com.google.firebase.MESSAGING_EVENT" />
            </intent-filter>
        </service>

    </application>
</manifest>
```

---

## 12. Testing

### Mock mode unit test

```kotlin
class MockServiceTest {
    @Test
    fun echoReplyArrivesAfter1500ms() {
        val core = MockAppCore()
        val sentAt = System.currentTimeMillis()
        core.sendDm("did:example:recipient", 1u, "hello", sentAt)
        Thread.sleep(200)
        // No reply yet
        assert(core.receiveMessagesWs().isEmpty())
        Thread.sleep(1400)
        // Echo reply should have arrived
        val messages = core.receiveMessagesWs()
        assert(messages.size == 1)
        assert(messages[0].plaintext == "hello")
    }
}
```

Mock mode does not require `libapp_core.so` to be present — same advantage as iOS.

### Cross-platform interop test

From `docs/01-technical-implementation.md` Stage 3 test plan:

> "Cross-platform interop test: iOS device sends an encrypted message, Android device decrypts it correctly, and vice versa — run against a real test homeserver in CI."

This test should live in `core/crates/app-core/tests/` as an integration test using the existing `test-utils` harness, not in the Android project itself.

---

## 13. Known TODOs at scaffold time

These mirror the iOS app's current state and should be resolved in later stages:

- **DB encryption key**: hardcoded `"dev-placeholder-key"` → Android Keystore in Stage 3
- **QR scanner**: `QRScannerScreen` scaffold only; wire up CameraX + ML Kit
- **Avatar picker**: photo selection not implemented
- **Invite link validation**: validate token against server before proceeding (iOS also TODO)
- **Push notifications**: `FcmService` stub; full implementation tracked in `docs/02-todos-deferred.md` under Push Notifications
- **Calls tab**: placeholder only; WebRTC tracked separately

> **Note:** The following were previously TODOs but are now fully specified in this doc:
> - Recovery key UI (`RecoveryExplainerScreen`, `RecoveryConsoleScreen`)
> - Passkey registration (`PasskeyExplainerScreen`, `PasskeyManager`)
> - Local notifications (`NotificationPresenter`)
> - QR invite generation (`MyQRCodeScreen`)
> - Contact display name resolution / refresh (`AppViewModel`)
> - Deep link handling + app lifecycle tracking (`AppViewModel`)
> - Loading conversations + message previews from DB at startup (`AppViewModel`)

---

## 14. Adding a New Feature (Android workflow)

The full cycle for a feature that touches Rust + Android mirrors the iOS workflow from `CLAUDE.md`:

1. Add Rust FFI method to `core/crates/app-core/src/lib.rs` (sync, `#[uniffi::export]`)
2. `make bindings` — regenerates `mobile/android/Generated/uniffi/app_core/app_core.kt`
3. Add method to `AppCoreInterface.kt` and `MockAppCore`
4. Call from `AppViewModel` via `viewModelScope.launch(Dispatchers.IO)`

Use `/new-ffi-method <name>` to scaffold the Rust + iOS side; then mirror it on Android manually (or extend the slash command to cover Android too).
