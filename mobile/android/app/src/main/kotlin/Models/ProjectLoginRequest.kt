package net.theavalanche.app

import java.util.UUID

/// Which OAuth front-end initiated a "Sign in with Avalanche" login (docs/25).
sealed class ProjectLoginFlow {
    /// Same-device authorization-code + PKCE: on approval we mint a code and
    /// redirect the browser back to `redirectUri?code=...&state=...`.
    data class AuthorizationCode(
        val redirectUri: String,
        val codeChallenge: String,
        val codeChallengeMethod: String,
        val state: String?,
    ) : ProjectLoginFlow()

    /// Cross-device device grant: on approval we approve the `userCode` a
    /// Project started on another device (desktop browser).
    data class Device(val userCode: String) : ProjectLoginFlow()
}

/// A pending "Sign in with Avalanche" request parsed from an `authorize` deep
/// link (docs/25-project-login.md). Drives the consent dialog. Mirrors iOS
/// `ProjectLoginRequest`.
data class ProjectLoginRequest(
    val clientId: String,
    val serverUrl: String,
    /// The local account (on `serverUrl`) that will authorize this login.
    val accountId: String,
    val scope: String?,
    val flow: ProjectLoginFlow,
    /// Resolved from the homeserver's Projects list (trustworthy); null until
    /// resolved or if the client isn't in the registry.
    val projectName: String? = null,
    val projectUrl: String? = null,
    val official: Boolean = false,
    val id: String = UUID.randomUUID().toString(),
) {
    /// True for the cross-device (QR/desktop) front-end — drives the extra
    /// "signing in on another device" consent warning.
    val isCrossDevice: Boolean get() = flow is ProjectLoginFlow.Device

    /// A user-facing label: the verified Project name if resolved, else the
    /// client id.
    val displayLabel: String get() = projectName ?: clientId
}

/// A structured login failure the app surfaces so it (or a Project) can hook an
/// invitation/onboarding flow. `onboarding` itself is out of scope (docs/25).
sealed class ProjectLoginError {
    /// The user has no account on the homeserver the login targets.
    data class NoAccountOnServer(val serverUrl: String) : ProjectLoginError()
    /// The approval/mint call failed.
    data class Failed(val message: String) : ProjectLoginError()
}
