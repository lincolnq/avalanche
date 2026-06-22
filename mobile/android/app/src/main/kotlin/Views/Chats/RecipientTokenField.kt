package net.theavalanche.app

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.drawable.BitmapDrawable
import android.text.Editable
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.TextWatcher
import android.text.style.ImageSpan
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.TextView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.core.graphics.TypefaceCompat

// ---------------------------------------------------------------------------
// RecipientTokenField — iMessage-style recipient pill field.
//
// Mirrors iOS Sources/Views/Chats/RecipientTokenField.swift.
//
// On iOS the chips are NSTextAttachments embedded in a UITextView so they
// behave as characters (backspace, selection all work natively). We mirror
// this on Android using ImageSpan inside an EditText — the chip IS a
// character (the Unicode object-replacement character U+FFFC), so all of the
// same backspace / selection semantics come for free.
//
// Public API surface:
//   - data class Chip  (id, did, displayName, label)
//   - class RecipientFieldHandle  (addChip)
//   - fun RecipientTokenField(chips, query, prefix, placeholder, handle, onSubmit, modifier)
//   - fun shortenDid(did)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Chip
// ---------------------------------------------------------------------------

/**
 * A confirmed recipient shown as a rounded pill in the [RecipientTokenField].
 *
 * Mirrors iOS `ComposeMessageView.Chip`.
 */
data class Chip(
    val id: String,       // == did
    val did: String,
    val displayName: String,
) {
    /** User-visible text for the chip. Never a raw full DID. */
    val label: String get() = if (displayName.isEmpty()) shortenDid(did) else displayName
}

// ---------------------------------------------------------------------------
// RecipientFieldHandle
// ---------------------------------------------------------------------------

/**
 * Lets Compose push a new chip into the [RecipientTokenEditText] without
 * owning its content. The field registers itself here when constructed.
 *
 * Mirrors iOS `RecipientFieldHandle`.
 */
class RecipientFieldHandle {
    // The field registers itself here; nullable because the view may not be
    // attached yet (or may have been recycled).
    internal var editText: RecipientTokenEditText? = null

    fun addChip(chip: Chip) {
        editText?.insertChip(chip)
    }
}

// ---------------------------------------------------------------------------
// RecipientTokenEditText
// ---------------------------------------------------------------------------

/**
 * [EditText] whose recipients are rendered as [ImageSpan] pill chips.
 *
 * Chips are encoded as the Unicode OBJECT REPLACEMENT CHARACTER (U+FFFC),
 * tagged with a [ChipSpan] so they can be identified and extracted. Text
 * after the last chip is the "query" the user is currently typing.
 *
 * Mirrors iOS `RecipientTokenTextView`.
 */
class RecipientTokenEditText(context: Context) : EditText(context) {

    var onChipsChanged: ((List<Chip>) -> Unit)? = null
    var onQueryChanged: ((String) -> Unit)? = null
    var onSubmit: (() -> Unit)? = null

    private var suppressTextWatcher = false

    init {
        isSingleLine = false
        maxLines = Int.MAX_VALUE
        imeOptions = EditorInfo.IME_FLAG_NO_ENTER_ACTION
        inputType = android.text.InputType.TYPE_CLASS_TEXT or
                android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE or
                android.text.InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        background = null
        setPadding(0, 12, 0, 12)

        // Brand tint for cursor / selection handles.
        // TODO(opus): tinting the cursor color via DrawableCompat requires API 29+
        //             on older APIs the default accent color is used.

        addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(s: Editable?) {
                if (suppressTextWatcher) return
                val newChips = currentChips
                val newQuery = currentQuery
                onChipsChanged?.invoke(newChips)
                onQueryChanged?.invoke(newQuery)
            }
        })

        // Handle Enter → submit, and two-stage backspace.
        setOnKeyListener { _, keyCode, event ->
            if (event.action == KeyEvent.ACTION_DOWN && keyCode == KeyEvent.KEYCODE_ENTER) {
                onSubmit?.invoke()
                true
            } else {
                false
            }
        }

        // EditorActionListener as a safety net for software keyboards that send
        // IME_ACTION_DONE / IME_ACTION_NEXT instead of a key event.
        setOnEditorActionListener { _, actionId, _ ->
            if (actionId == EditorInfo.IME_ACTION_DONE ||
                actionId == EditorInfo.IME_ACTION_NEXT ||
                actionId == EditorInfo.IME_ACTION_SEND
            ) {
                onSubmit?.invoke()
                true
            } else {
                false
            }
        }
    }

    // -----------------------------------------------------------------------
    // Two-stage backspace: first backspace after a chip selects it; second
    // backspace (now a selection) deletes it — exactly matching iOS behaviour.
    // -----------------------------------------------------------------------

    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        if (keyCode == KeyEvent.KEYCODE_DEL) {
            val sel = selectionStart
            if (selectionStart == selectionEnd && sel > 0) {
                val s = text ?: return super.onKeyDown(keyCode, event)
                val spans = s.getSpans(sel - 1, sel, ChipSpan::class.java)
                if (spans.isNotEmpty()) {
                    // Chip sits right before the cursor — select it instead of deleting.
                    setSelection(sel - 1, sel)
                    return true
                }
            }
        }
        return super.onKeyDown(keyCode, event)
    }

    // -----------------------------------------------------------------------
    // Reading state
    // -----------------------------------------------------------------------

    val currentChips: List<Chip>
        get() {
            val s = text ?: return emptyList()
            return s.getSpans(0, s.length, ChipSpan::class.java)
                .sortedBy { s.getSpanStart(it) }
                .map { it.chip }
        }

    /** Text after the last chip — what the user is currently typing. */
    val currentQuery: String
        get() {
            val s = text ?: return ""
            val spans = s.getSpans(0, s.length, ChipSpan::class.java)
            val end = if (spans.isEmpty()) 0
            else spans.maxOf { s.getSpanEnd(it) }
            return s.substring(end)
        }

    // -----------------------------------------------------------------------
    // Mutation
    // -----------------------------------------------------------------------

    /** Replace all content with the given chips (used to seed the field). */
    fun setChips(chips: List<Chip>) {
        suppressTextWatcher = true
        try {
            val ssb = SpannableStringBuilder()
            for (chip in chips) ssb.appendChip(chip)
            setText(ssb)
            setSelection(length())
        } finally {
            suppressTextWatcher = false
        }
    }

    /**
     * Append a chip after the last chip, clearing any in-progress typed query.
     * No-op (beyond clearing the query) if the recipient is already present.
     */
    fun insertChip(chip: Chip) {
        val s = text ?: return
        // Remove any trailing query text (text after the last chip).
        val spans = s.getSpans(0, s.length, ChipSpan::class.java)
        val trailingStart = if (spans.isEmpty()) 0
        else spans.maxOf { s.getSpanEnd(it) }
        if (trailingStart < s.length) s.delete(trailingStart, s.length)

        val alreadyPresent = currentChips.any { it.did == chip.did }
        if (!alreadyPresent) {
            suppressTextWatcher = true
            try {
                s.appendChip(chip)
            } finally {
                suppressTextWatcher = false
            }
        }

        setSelection(length())
        // Notify observers manually (watcher was suppressed).
        onChipsChanged?.invoke(currentChips)
        onQueryChanged?.invoke(currentQuery)
    }

    // -----------------------------------------------------------------------
    // Chip rendering
    // -----------------------------------------------------------------------

    private fun Editable.appendChip(chip: Chip) {
        val bitmap = renderChipBitmap(chip.label)
        val drawable = BitmapDrawable(resources, bitmap).apply {
            setBounds(0, 0, bitmap.width, bitmap.height)
        }
        val span = ChipSpan(drawable, chip)
        val start = length
        append(CHIP_CHAR)
        setSpan(span, start, start + 1, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
    }

    private fun SpannableStringBuilder.appendChip(chip: Chip) {
        val bitmap = renderChipBitmap(chip.label)
        val drawable = BitmapDrawable(resources, bitmap).apply {
            setBounds(0, 0, bitmap.width, bitmap.height)
        }
        val span = ChipSpan(drawable, chip)
        val start = length
        append(CHIP_CHAR)
        setSpan(span, start, start + 1, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
    }

    /**
     * Render a chip pill bitmap with the same geometry as iOS:
     * hPad=9dp, vPad=3dp, trailing gap=6dp, cornerRadius = height/2.
     * Normal state: brand fill at 15% opacity, Ink text.
     * (Selected state uses brand fill + white text — Android's selection
     * highlight covers the chip, so we don't need to swap images ourselves.)
     */
    private fun renderChipBitmap(label: String): Bitmap {
        val density = resources.displayMetrics.density
        val hPad = (9 * density)
        val vPad = (3 * density)
        val trailingGap = (6 * density)

        val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            textSize = textSize.coerceAtLeast(14 * density)
            color = AvalancheColors.Ink.toArgb()
        }
        // Use the EditText's current text paint size for consistency.
        textPaint.textSize = paint.textSize

        val textWidth = textPaint.measureText(label)
        val fm = textPaint.fontMetrics
        val textHeight = fm.descent - fm.ascent

        val pillWidth = textWidth + hPad * 2
        val pillHeight = textHeight + vPad * 2
        val totalWidth = pillWidth + trailingGap

        val bmp = Bitmap.createBitmap(
            totalWidth.toInt().coerceAtLeast(1),
            pillHeight.toInt().coerceAtLeast(1),
            Bitmap.Config.ARGB_8888,
        )
        val canvas = Canvas(bmp)

        val brandColor = AvalancheColors.Brand.toArgb()
        val bgColor = android.graphics.Color.argb(
            (0.15f * 255).toInt(),
            android.graphics.Color.red(brandColor),
            android.graphics.Color.green(brandColor),
            android.graphics.Color.blue(brandColor),
        )

        val fillPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = bgColor }
        val pill = RectF(0f, 0f, pillWidth, pillHeight)
        canvas.drawRoundRect(pill, pillHeight / 2, pillHeight / 2, fillPaint)

        // Draw label text centered in the pill.
        val textX = hPad
        val textY = vPad - fm.ascent
        textPaint.color = AvalancheColors.Ink.toArgb()
        canvas.drawText(label, textX, textY, textPaint)

        return bmp
    }

    companion object {
        /** Unicode OBJECT REPLACEMENT CHARACTER — the "character" for each chip. */
        const val CHIP_CHAR = '￼'
    }
}

// ---------------------------------------------------------------------------
// ChipSpan
// ---------------------------------------------------------------------------

/**
 * Tags a chip character with its [Chip] data so we can extract recipients
 * back out of the [Editable]. Extends [ImageSpan] so the pill bitmap is
 * drawn in-line, exactly as [NSTextAttachment] does on iOS.
 */
class ChipSpan(
    drawable: android.graphics.drawable.Drawable,
    val chip: Chip,
) : ImageSpan(drawable, ALIGN_BASELINE)

// ---------------------------------------------------------------------------
// RecipientTokenField Composable
// ---------------------------------------------------------------------------

/**
 * iMessage-style recipient token field backed by [RecipientTokenEditText].
 *
 * Mirrors iOS `RecipientTokenField` (a `UIViewRepresentable`). The chips and
 * query are reported back via [onChipsChanged] / [onQueryChanged]; new chips
 * can be pushed in programmatically via [handle].
 *
 * @param chips         Current list of recipient chips (read-only; use [handle] to add).
 * @param query         Current typed text (the search query following the chips).
 * @param prefix        Inline label before the first line (e.g. "To:").
 * @param placeholder   Hint text shown when the field is empty.
 * @param handle        Imperative handle for pushing chips in from autocomplete.
 * @param onChipsChanged Called when the chip list changes (chip removed by backspace, etc.).
 * @param onQueryChanged Called when the trailing text changes.
 * @param onSubmit      Called when the user presses Enter/Done.
 * @param modifier      Standard Compose modifier.
 */
@Composable
fun RecipientTokenField(
    chips: List<Chip>,
    query: String,
    prefix: String = "",
    placeholder: String = "",
    handle: RecipientFieldHandle,
    onChipsChanged: (List<Chip>) -> Unit = {},
    onQueryChanged: (String) -> Unit = {},
    onSubmit: () -> Unit = {},
    modifier: Modifier = Modifier,
) {
    // We keep a reference to the underlying EditText so we can update prefix /
    // placeholder when Compose recomposes without re-creating the view.
    val editTextRef = remember { mutableListOf<RecipientTokenEditText>() }

    AndroidView(
        modifier = modifier,
        factory = { ctx ->
            RecipientTokenEditText(ctx).also { et ->
                et.hint = buildPrefixedHint(prefix, placeholder)
                et.onChipsChanged = onChipsChanged
                et.onQueryChanged = onQueryChanged
                et.onSubmit = onSubmit
                et.setChips(chips)
                handle.editText = et

                // Auto-focus the field when it first appears, mirroring iOS
                // `didMoveToWindow` auto-focus.
                et.requestFocus()

                editTextRef.clear()
                editTextRef.add(et)
            }
        },
        update = { et ->
            // Update prefix / placeholder on recomposition. Do NOT overwrite
            // the chip content — the view owns it, matching the iOS design.
            val newHint = buildPrefixedHint(prefix, placeholder)
            if (et.hint?.toString() != newHint) et.hint = newHint
            // Ensure the handle is wired (survives configuration changes).
            handle.editText = et
        },
    )
}

/**
 * Build a hint string combining the optional [prefix] (e.g. "To: ") with
 * the [placeholder]. Android's hint is a single string; we concatenate them
 * with a space separator to approximate the iOS layout where the prefix
 * label sits inline and the placeholder follows it.
 *
 * A full-fidelity prefix (label floated to the left, chips indented on the
 * first line only) would require a custom `Layout` or `StaticLayout` with
 * exclusion rects — marked as a deferred improvement.
 * // TODO(opus): implement a proper prefix label with first-line exclusion
 *               rect, matching iOS RecipientTokenTextView.prefixText layout.
 */
private fun buildPrefixedHint(prefix: String, placeholder: String): String =
    if (prefix.isNotEmpty()) "$prefix $placeholder" else placeholder

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

// shortenDid lives in ComposeMessageView.kt (same package) — single definition shared across Chats.
