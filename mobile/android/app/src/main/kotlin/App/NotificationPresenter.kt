package net.theavalanche.app

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

/**
 * Schedules local notifications for inbound messages.
 *
 * Suppression rules (adapted from Signal-iOS via NotificationPresenter.swift):
 * - App active + viewing this conversation -> no banner (badge still updates).
 * - App active + viewing a different conversation -> banner + sound.
 * - App backgrounded/inactive -> banner + sound.
 *
 * Receipts and other non-Text envelope variants are filtered out in Rust
 * (AppCoreInner::receive_messages_ws_async) before they reach Kotlin, so
 * every call here corresponds to a real user-visible message.
 *
 * Mirrors iOS App/NotificationPresenter.swift.
 */
object NotificationPresenter {

    private const val CHANNEL_ID = "avalanche_messages"
    private const val CHANNEL_NAME = "Messages"

    /** Per-conversation timestamp (epoch ms) of the last notification sound.
     *  Used to throttle rapid-fire sounds when many messages arrive in a burst. */
    private val lastSoundAt: MutableMap<String, Long> = mutableMapOf()
    private const val SOUND_THROTTLE_MS = 3_000L

    /**
     * Ensure the notification channel exists (required on API 26+).
     * Call once on app startup from [MainActivity].
     */
    fun createNotificationChannel(context: Context) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val importance = NotificationManager.IMPORTANCE_HIGH
            val channel = NotificationChannel(CHANNEL_ID, CHANNEL_NAME, importance).apply {
                description = "Encrypted message notifications"
            }
            val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            manager.createNotificationChannel(channel)
        }
    }

    /**
     * Schedule a local notification for an inbound message and refresh the
     * app badge. No-op for outgoing messages — callers must only invoke this
     * for messages where senderDid != accountId.
     *
     * Mirrors iOS NotificationPresenter.present(message:conversation:senderDisplayName:appState:).
     */
    fun present(
        context: Context,
        message: Message,
        conversation: Conversation,
        senderDisplayName: String,
        appViewModel: AppViewModel,
    ) {
        updateBadge(context = context, appViewModel = appViewModel)

        // Empty body — nothing useful to show.
        val body = message.body.trim()
        if (body.isEmpty()) return

        // Suppress when the user is already reading this conversation.
        if (appViewModel.isAppActive.value && appViewModel.currentConversationId.value == conversation.id) {
            return
        }

        val playSound = shouldPlaySound(conversation.id)

        // Build a tap intent that deep-links back to the conversation.
        val tapIntent = Intent(context, MainActivity::class.java).apply {
            action = Intent.ACTION_VIEW
            data = android.net.Uri.parse("https://go.theavalanche.net/conversation/${conversation.recipientDid ?: conversation.id}")
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            putExtra("conversationId", conversation.id)
            putExtra("accountId", conversation.accountId)
        }
        val pendingIntent = PendingIntent.getActivity(
            context,
            conversation.id.hashCode(),
            tapIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        val notification = NotificationCompat.Builder(context, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_dialog_email) // TODO(opus): replace with branded icon R.drawable.ic_notification
            .setContentTitle(senderDisplayName)
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setGroup(conversation.id) // mirrors iOS threadIdentifier
            .setContentIntent(pendingIntent)
            .setAutoCancel(true)
            .apply {
                if (playSound) {
                    setDefaults(NotificationCompat.DEFAULT_SOUND or NotificationCompat.DEFAULT_VIBRATE)
                } else {
                    setSound(null)
                    setVibrate(null)
                }
            }
            .build()

        // Use the message id's hash as the notification id so rapid messages
        // from the same conversation stack rather than replace each other.
        val notificationId = message.id.hashCode()

        try {
            NotificationManagerCompat.from(context).notify(notificationId, notification)
        } catch (e: SecurityException) {
            // POST_NOTIFICATIONS permission not granted yet.
            AppLog.warn("NotificationPresenter", "POST_NOTIFICATIONS not granted: ${e.message}")
        }
    }

    /**
     * Recompute the app badge from in-memory unread counts.
     *
     * Android does not have a system-wide badge API before API 26 launcher
     * shortcuts, but NotificationManagerCompat provides a best-effort path.
     * Mirrors iOS NotificationPresenter.updateBadge(appState:).
     */
    fun updateBadge(context: Context, appViewModel: AppViewModel) {
        // TODO(opus): wire to ShortcutBadger or launcher-specific badge API if desired.
        // Android badge counts are derived from unread notification count automatically
        // on most launchers, so explicit badge setting is typically a no-op here.
        val total = appViewModel.conversations.value.sumOf { conv ->
            appViewModel.unreadCount(conv)
        }
        AppLog.info("NotificationPresenter", "unread badge total: $total")
    }

    /** Returns true if a sound should play for this conversation (throttled to once per 3 s). */
    private fun shouldPlaySound(conversationId: String): Boolean {
        val nowMs = System.currentTimeMillis()
        val last = lastSoundAt[conversationId]
        if (last != null && nowMs - last < SOUND_THROTTLE_MS) {
            return false
        }
        lastSoundAt[conversationId] = nowMs
        return true
    }
}
