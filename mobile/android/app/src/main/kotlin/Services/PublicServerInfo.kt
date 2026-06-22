package net.theavalanche.app

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * Client-side helper for a homeserver's public, unauthenticated `GET /v1/info`
 * endpoint (server name + optional privacy policy URL). Used by the onboarding
 * screens to surface the operator's privacy policy before a user joins or
 * creates an account on that server.
 *
 * Mirrors iOS Sources/Services/PublicServerInfo.swift.
 */
object PublicServerInfo {

    private data class Response(
        val serverName: String,
        val privacyPolicyUrl: String?,
    )

    /**
     * Best-effort fetch of the operator's privacy policy URL for [serverUrl].
     * Returns null if the server is unreachable, the endpoint errors, or no
     * policy is configured — callers just hide the link in that case.
     *
     * Must be called from a coroutine; switches to [Dispatchers.IO] internally.
     */
    suspend fun privacyPolicyUrl(serverUrl: String): String? = withContext(Dispatchers.IO) {
        try {
            val infoUrl = serverUrl.trimEnd('/') + "/v1/info"
            val connection = URL(infoUrl).openConnection() as HttpURLConnection
            connection.connectTimeout = 5_000
            connection.readTimeout = 5_000
            connection.requestMethod = "GET"

            if (connection.responseCode != HttpURLConnection.HTTP_OK) return@withContext null

            val body = connection.inputStream.bufferedReader().use { it.readText() }
            val json = JSONObject(body)
            json.optString("privacy_policy_url").takeIf { it.isNotEmpty() }
        } catch (_: Exception) {
            null
        }
    }
}
