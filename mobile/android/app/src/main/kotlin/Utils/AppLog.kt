package net.theavalanche.app

import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.UUID

data class LogEntry(
    val id: String = UUID.randomUUID().toString(),
    val timestamp: Date = Date(),
    val category: String,
    val message: String,
    val level: Level,
) {
    enum class Level { info, warn, error, ok }

    val formatted: String
        get() {
            val f = formatter.format(timestamp)
            return "$f [$category] $message"
        }

    companion object {
        private val formatter = SimpleDateFormat("HH:mm:ss.SSS", Locale.US)
    }
}

/// In-memory ring buffer of log entries, observable from Compose.
/// Thread-safe: StateFlow updates are atomic; appends are synchronized.
object AppLog {
    private const val CAPACITY = 1000

    private val _entries = MutableStateFlow<List<LogEntry>>(emptyList())
    val entries: StateFlow<List<LogEntry>> = _entries.asStateFlow()

    fun info(category: String, message: String) {
        append(LogEntry.Level.info, category, message)
    }

    fun warn(category: String, message: String) {
        append(LogEntry.Level.warn, category, message)
    }

    fun error(category: String, message: String) {
        append(LogEntry.Level.error, category, message)
    }

    fun ok(category: String, message: String) {
        append(LogEntry.Level.ok, category, message)
    }

    private fun append(level: LogEntry.Level, category: String, message: String) {
        val entry = LogEntry(timestamp = Date(), category = category, message = message, level = level)
        val tag = "[$category]"
        when (level) {
            LogEntry.Level.info -> Log.i(tag, message)
            LogEntry.Level.warn -> Log.w(tag, message)
            LogEntry.Level.error -> Log.e(tag, message)
            LogEntry.Level.ok -> Log.i(tag, message)
        }
        _entries.update { current ->
            val updated = current + entry
            if (updated.size > CAPACITY) updated.drop(updated.size - CAPACITY) else updated
        }
    }

    fun clear() {
        _entries.value = emptyList()
    }
}
