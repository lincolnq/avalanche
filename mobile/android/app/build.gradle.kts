plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    // FCM (Firebase) — processes google-services.json into the build.
    alias(libs.plugins.google.services)
}

// Push relay base URL, baked into BuildConfig.RELAY_URL. Mirrors how iOS injects
// RELAY_URL into Info.plist from .env (see Makefile). Override per-build with
// `-PRELAY_URL=...` or a RELAY_URL env var; defaults to the live relay.
val relayUrl: String = (project.findProperty("RELAY_URL") as String?)
    ?: System.getenv("RELAY_URL")
    ?: "https://relay.theavalanche.net"

// App version, derived from git so it stays in lock-step with the iOS scheme
// (see the Makefile's MARKETING_VERSION / CURRENT_PROJECT_VERSION):
//   versionName ← latest reachable tag, leading `v` stripped  (iOS MARKETING_VERSION / CFBundleShortVersionString)
//   versionCode ← total commit count, monotonically climbing  (iOS CURRENT_PROJECT_VERSION / CFBundleVersion)
// `make android` passes both as -P properties (the same vars it hands xcodebuild),
// so iOS and Android always stamp identical numbers. A bare `./gradlew` build
// falls back to deriving them from git here, then to 0.0.0 / 1 outside a repo.
fun gitValue(vararg args: String): String? = runCatching {
    providers.exec {
        commandLine(listOf("git") + args)
        isIgnoreExitValue = true
    }.standardOutput.asText.get().trim().takeIf { it.isNotEmpty() }
}.getOrNull()

val appVersionName: String =
    (project.findProperty("MARKETING_VERSION") as String?)?.takeIf { it.isNotEmpty() }
        ?: System.getenv("MARKETING_VERSION")?.takeIf { it.isNotEmpty() }
        ?: gitValue("describe", "--tags", "--abbrev=0")?.removePrefix("v")
        ?: "0.0.0"

val appVersionCode: Int =
    ((project.findProperty("CURRENT_PROJECT_VERSION") as String?)?.takeIf { it.isNotEmpty() }
        ?: System.getenv("CURRENT_PROJECT_VERSION")?.takeIf { it.isNotEmpty() }
        ?: gitValue("rev-list", "--count", "HEAD"))
        ?.toIntOrNull() ?: 1

// Release signing credentials are supplied entirely via environment variables at
// build time — see `make android-release`, which materializes them from 1Password
// into a RAM disk (the keystore file) and the process environment (the passwords),
// builds, then tears the RAM disk down. Nothing is written to persistent disk and
// no credentials are committed. When the env vars are absent (CI, fresh checkout,
// debug-only work) the release signing config is left unconfigured and
// `assembleRelease` produces an unsigned APK; `assembleDebug` is unaffected and
// always uses the auto-generated debug key.
val releaseStoreFile: String? = System.getenv("RELEASE_KEYSTORE_FILE")
val hasReleaseSigning: Boolean = releaseStoreFile != null && file(releaseStoreFile).exists()

android {
    namespace = "net.theavalanche.app"
    // API 37 (major). The upgraded AndroidX libs (compose-bom 2026.06, core-ktx
    // 1.19, lifecycle 2.11, …) embed a minimum-compileSdk of 37 in their metadata,
    // so 36 no longer suffices. We pin the *major* level only — no minorApiLevel,
    // since no dependency requires a specific minor (37.1) and pinning one forces
    // installing that exact minor platform.
    compileSdk = 37

    defaultConfig {
        applicationId = "net.theavalanche.app"
        minSdk = 26
        targetSdk = 37
        versionCode = appVersionCode
        versionName = appVersionName

        buildConfigField("String", "RELAY_URL", "\"$relayUrl\"")

        // ABI selection is per build type below (debug ships x86_64 for the
        // emulator; release is arm64-only). An abiFilters is required in any case:
        // without it JNA's @aar drags in libjnidispatch.so for armeabi/mips/x86
        // too, and a device on one of those ABIs would load JNA but find no
        // libapp_core.so and crash at startup.
    }

    signingConfigs {
        create("release") {
            // Populated only when the RELEASE_* env vars are present (see above).
            if (hasReleaseSigning) {
                storeFile = file(releaseStoreFile!!)
                storePassword = System.getenv("RELEASE_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("RELEASE_KEY_ALIAS")
                keyPassword = System.getenv("RELEASE_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        debug {
            // Keep x86_64 so the app runs on the standard x86_64 emulator
            // alongside arm64 devices. (See ANDROID_ABIS in the Makefile — the
            // cross-compile produces a libapp_core.so for both.)
            ndk {
                abiFilters += listOf("arm64-v8a", "x86_64")
            }
        }
        release {
            // Distributable APK: arm64-v8a only. Every real Android device is
            // arm64; x86_64 is emulator-only, so shipping it just bloats the
            // download.
            ndk {
                abiFilters += "arm64-v8a"
            }
            optimization {
                enable = false
            }
            // Sign with the release key when configured; otherwise leave unsigned
            // (a debug-only checkout can't and shouldn't produce a signed release).
            signingConfig = if (hasReleaseSigning) {
                signingConfigs.getByName("release")
            } else {
                null
            }
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }

    // UniFFI-generated Kotlin glue lives outside src/ (it's a build artifact,
    // regenerated by `make android-bindings`). The matching native libraries are
    // cross-compiled into src/main/jniLibs/<abi>/libapp_core.so, which AGP picks
    // up automatically.
    sourceSets {
        getByName("main") {
            // Kotlin 2.x (K2) only compiles .kt files that live under a *Kotlin*
            // source root — a bare java.srcDir no longer pulls the UniFFI-generated
            // glue into the Kotlin compilation, so register it on the kotlin set.
            kotlin.directories.add("${rootProject.projectDir}/Generated")
        }
    }
}

dependencies {
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.extended)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)

    // Navigation
    implementation(libs.androidx.navigation.compose)

    // Coroutines
    implementation(libs.kotlinx.coroutines.android)

    // JSON persistence (SharedPreferences-backed metadata; mirrors iOS Codable)
    implementation(libs.kotlinx.serialization.json)

    // Rust core via UniFFI. The generated Kotlin binding (uniffi.app_core) calls
    // into libapp_core.so through JNA, so JNA's own native dispatch library
    // (libjnidispatch.so) must be bundled into the APK — which only happens when
    // the dependency is resolved as the Android .aar (the aar ships the jniLibs).
    // The artifact closure forces the aar variant; the version stays in the catalog.
    implementation(libs.jna) {
        artifact {
            type = "aar"
            extension = "aar"
        }
    }

    // Push notifications via Firebase Cloud Messaging. The relay only ever sees a
    // content-free wakeup addressed to a rotating pseudonym (docs/15) — FCM is the
    // transport, not a data sink. Requires app/google-services.json.
    implementation(platform(libs.firebase.bom))
    implementation(libs.firebase.messaging)

    // QR scanning for onboarding (CameraX + ZXing; ZXing avoids Google Play Services
    // so the app works on de-Googled Android)
    implementation(libs.androidx.camera.core)
    implementation(libs.androidx.camera.camera2)
    implementation(libs.androidx.camera.lifecycle)
    implementation(libs.androidx.camera.view)
    implementation(libs.zxing.android.embedded)

    testImplementation(libs.junit)
    debugImplementation(libs.androidx.compose.ui.tooling)
}