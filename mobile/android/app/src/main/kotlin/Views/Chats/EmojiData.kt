package net.theavalanche.app

import android.content.Context
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.DirectionsCar
import androidx.compose.material.icons.filled.DirectionsRun
import androidx.compose.material.icons.filled.EmojiEmotions
import androidx.compose.material.icons.filled.Fastfood
import androidx.compose.material.icons.filled.Flag
import androidx.compose.material.icons.filled.Lightbulb
import androidx.compose.material.icons.filled.Pets
import androidx.compose.material.icons.filled.Tag
import androidx.compose.ui.graphics.vector.ImageVector

/**
 * A curated, hand-maintained emoji catalog for the reaction picker (docs/33).
 * Mirrors mobile/ios/Actnet/Sources/Views/Chats/EmojiData.swift — keep the
 * categories, their order, and the keyword map in sync across platforms.
 */
enum class EmojiCategory(val displayName: String, val icon: ImageVector) {
    SMILEYS("Smileys & People", Icons.Filled.EmojiEmotions),
    ANIMALS("Animals & Nature", Icons.Filled.Pets),
    FOOD("Food & Drink", Icons.Filled.Fastfood),
    ACTIVITY("Activity", Icons.Filled.DirectionsRun),
    TRAVEL("Travel & Places", Icons.Filled.DirectionsCar),
    OBJECTS("Objects", Icons.Filled.Lightbulb),
    SYMBOLS("Symbols", Icons.Filled.Tag),
    FLAGS("Flags", Icons.Filled.Flag),
}

object EmojiData {
    /** The six quick-reaction emoji shown in the reaction bar. */
    val quick = listOf("👍", "❤️", "😂", "😮", "😢", "🙏")

    /** Emoji grouped by category, in display order. */
    val byCategory: List<Pair<EmojiCategory, List<String>>> = listOf(
        EmojiCategory.SMILEYS to listOf(
            "😀","😃","😄","😁","😆","😅","🤣","😂","🙂","🙃","😉","😊","😇","🥰","😍","🤩",
            "😘","😗","😚","😙","😋","😛","😜","🤪","😝","🤑","🤗","🤭","🤫","🤔","🤐","🤨",
            "😐","😑","😶","😏","😒","🙄","😬","🤥","😌","😔","😪","🤤","😴","😷","🤒","🤕",
            "🤢","🤮","🤧","🥵","🥶","🥴","😵","🤯","🤠","🥳","😎","🤓","🧐","😕","😟","🙁",
            "😮","😯","😲","😳","🥺","😦","😧","😨","😰","😥","😢","😭","😱","😖","😣","😞",
            "😓","😩","😫","🥱","😤","😡","😠","🤬","😈","👿","💀","💩","🤡","👹","👻","👽",
            "🙈","🙉","🙊","👋","🤚","✋","🖖","👌","🤏","✌️","🤞","🤟","🤘","🤙","👈","👉",
            "👆","👇","☝️","👍","👎","✊","👊","🤛","🤜","👏","🙌","👐","🤲","🙏","💪","🦾",
            "👀","👁️","👄","🧠","🫀","🦷","👶","🧒","👦","👧","🧑","👨","👩","🧓","👴","👵",
        ),
        EmojiCategory.ANIMALS to listOf(
            "🐶","🐱","🐭","🐹","🐰","🦊","🐻","🐼","🐨","🐯","🦁","🐮","🐷","🐸","🐵","🐔",
            "🐧","🐦","🐤","🦆","🦅","🦉","🦇","🐺","🐗","🐴","🦄","🐝","🐛","🦋","🐌","🐞",
            "🐜","🦗","🕷️","🦂","🐢","🐍","🦎","🦖","🐙","🦑","🦐","🦀","🐠","🐟","🐬","🐳",
            "🐋","🦈","🐊","🐅","🐆","🦓","🦍","🐘","🦏","🐪","🐫","🦒","🐃","🐂","🐄","🐎",
            "🐖","🐏","🐑","🐐","🦌","🐕","🐩","🐈","🐓","🦃","🕊️","🐇","🐁","🐀","🌵","🎄",
            "🌲","🌳","🌴","🌱","🌿","☘️","🍀","🎍","🍃","🍂","🍁","🌷","🌹","🌺","🌸","🌼",
            "🌻","🌞","🌝","🌚","🌙","⭐","🌟","✨","⚡","🔥","🌈","☀️","⛅","☁️","🌧️","❄️",
        ),
        EmojiCategory.FOOD to listOf(
            "🍏","🍎","🍐","🍊","🍋","🍌","🍉","🍇","🍓","🫐","🍈","🍒","🍑","🥭","🍍","🥥",
            "🥝","🍅","🍆","🥑","🥦","🥬","🥒","🌶️","🌽","🥕","🧄","🧅","🥔","🍠","🥐","🍞",
            "🥖","🥨","🧀","🥚","🍳","🧇","🥞","🥓","🥩","🍗","🍖","🌭","🍔","🍟","🍕","🥪",
            "🌮","🌯","🥙","🧆","🥘","🍝","🍜","🍲","🍛","🍣","🍱","🥟","🍤","🍙","🍚","🍘",
            "🍥","🥠","🍢","🍡","🍧","🍨","🍦","🥧","🧁","🍰","🎂","🍮","🍭","🍬","🍫","🍿",
            "🍩","🍪","🌰","🥜","🍯","🥛","🍼","☕","🍵","🧃","🥤","🍶","🍺","🍻","🥂","🍷",
            "🥃","🍸","🍹","🍾","🧉","🧊",
        ),
        EmojiCategory.ACTIVITY to listOf(
            "⚽","🏀","🏈","⚾","🥎","🎾","🏐","🏉","🥏","🎱","🪀","🏓","🏸","🏒","🏑","🥍",
            "🏏","🥅","⛳","🪁","🏹","🎣","🤿","🥊","🥋","🎽","🛹","🛷","⛸️","🥌","🎿","⛷️",
            "🏂","🪂","🏋️","🤼","🤸","⛹️","🤺","🤾","🏌️","🏇","🧘","🏄","🏊","🤽","🚣","🧗",
            "🚵","🚴","🏆","🥇","🥈","🥉","🏅","🎖️","🏵️","🎗️","🎫","🎟️","🎪","🤹","🎭","🎨",
            "🎬","🎤","🎧","🎼","🎹","🥁","🎷","🎺","🎸","🪕","🎻","🎲","♟️","🎯","🎳","🎮",
            "🎰","🧩",
        ),
        EmojiCategory.TRAVEL to listOf(
            "🚗","🚕","🚙","🚌","🚎","🏎️","🚓","🚑","🚒","🚐","🚚","🚛","🚜","🛴","🚲","🛵",
            "🏍️","🚨","🚔","🚍","🚘","🚖","🚡","🚠","🚟","🚃","🚋","🚞","🚝","🚄","🚅","🚈",
            "🚂","🚆","🚇","🚊","🚉","✈️","🛫","🛬","🛩️","💺","🚀","🛸","🚁","🛶","⛵","🚤",
            "🛥️","🛳️","⛴️","🚢","⚓","⛽","🚧","🚦","🚥","🗺️","🗿","🗽","🗼","🏰","🏯","🏟️",
            "🎡","🎢","🎠","⛲","⛱️","🏖️","🏝️","🏜️","🌋","⛰️","🏔️","🗻","🏕️","⛺","🏠","🏡",
            "🏘️","🏢","🏬","🏣","🏥","🏦","🏨","🏪","🏫","🏛️","⛪","🕌","🕍","🛕","🌃","🌆",
            "🌇","🌉","🌌","🌁",
        ),
        EmojiCategory.OBJECTS to listOf(
            "⌚","📱","💻","⌨️","🖥️","🖨️","🖱️","💽","💾","💿","📀","📷","📸","📹","🎥","📽️",
            "📞","☎️","📟","📠","📺","📻","🎙️","⏱️","⏲️","⏰","🕰️","⌛","⏳","🔋","🔌","💡",
            "🔦","🕯️","🧯","🛢️","💸","💵","💴","💶","💷","💰","💳","💎","⚖️","🔧","🔨","⚒️",
            "🛠️","⛏️","🔩","⚙️","🧰","🧲","🔫","💣","🧨","🔪","🗡️","⚔️","🛡️","🚬","⚰️","🏺",
            "🔮","📿","🧿","💈","🔭","🔬","🕳️","💊","💉","🩸","🌡️","🧹","🧺","🧻","🚽","🚰",
            "🛁","🛀","🧼","🪒","🧽","🔑","🗝️","🚪","🛋️","🛏️","🖼️","🛍️","🎁","🎈","🎏","🎀",
            "📦","📫","📮","📯","📜","📃","📑","📊","📈","📉","📇","🗂️","📅","📆","📋","📌",
            "📎","🖇️","📏","📐","✂️","🖊️","✏️","📝","🔍","🔒","🔓","📖","📚","🔔",
        ),
        EmojiCategory.SYMBOLS to listOf(
            "❤️","🧡","💛","💚","💙","💜","🖤","🤍","🤎","💔","❣️","💕","💞","💓","💗","💖",
            "💘","💝","💟","☮️","✝️","☪️","🕉️","☸️","✡️","🔯","🕎","☯️","☦️","🛐","⛎","♈",
            "♉","♊","♋","♌","♍","♎","♏","♐","♑","♒","♓","🆔","⚛️","🉑","☢️","☣️",
            "📴","📳","🈶","🈚","🈸","🈺","🈷️","✴️","🆚","💮","🉐","㊙️","㊗️","🈴","🈵","🈹",
            "❗","❕","❓","❔","‼️","⁉️","💯","🔅","🔆","〽️","⚠️","🚸","🔱","⚜️","🔰","♻️",
            "✅","🈯","💹","❇️","✳️","❎","🌐","💠","Ⓜ️","🌀","💤","🏧","🚾","♿","🅿️","🈳",
            "🚹","🚺","🚼","⚧️","🚻","🔞","📵","🚭","❌","⭕","🛑","⛔","📛","🚫","💢","♨️",
            "🔟","🔢","➕","➖","➗","✖️","♾️","💲","💱","™️","©️","®️","👁️‍🗨️","🔚","🔙","🔛",
        ),
        EmojiCategory.FLAGS to listOf(
            "🏁","🚩","🎌","🏴","🏳️","🏳️‍🌈","🏳️‍⚧️","🏴‍☠️","🇺🇳","🇺🇸","🇨🇦","🇬🇧","🇮🇪","🇫🇷","🇩🇪","🇪🇸",
            "🇮🇹","🇵🇹","🇳🇱","🇧🇪","🇨🇭","🇦🇹","🇸🇪","🇳🇴","🇩🇰","🇫🇮","🇵🇱","🇺🇦","🇷🇺","🇬🇷","🇹🇷","🇮🇱",
            "🇸🇦","🇦🇪","🇪🇬","🇿🇦","🇳🇬","🇰🇪","🇮🇳","🇵🇰","🇧🇩","🇨🇳","🇯🇵","🇰🇷","🇹🇼","🇭🇰","🇹🇭","🇻🇳",
            "🇵🇭","🇮🇩","🇲🇾","🇸🇬","🇦🇺","🇳🇿","🇧🇷","🇦🇷","🇨🇱","🇨🇴","🇵🇪","🇲🇽",
        ),
    )

    /** Flat list for search. */
    val all: List<String> = byCategory.flatMap { it.second }

    /**
     * Search keywords for the commonly-searched emoji. Emoji without an entry
     * stay browsable by category but won't surface in text search. Keep in sync
     * across platforms.
     */
    val keywords: Map<String, String> = mapOf(
        "😀" to "grin happy smile", "😃" to "happy smile joy", "😄" to "happy smile laugh",
        "😁" to "grin beam happy", "😆" to "laugh happy", "😅" to "sweat laugh nervous",
        "🤣" to "rofl laugh lol", "😂" to "joy laugh cry lol tears", "🙂" to "slight smile",
        "🙃" to "upside down silly", "😉" to "wink", "😊" to "blush smile happy",
        "😇" to "angel innocent halo", "🥰" to "love hearts adore", "😍" to "love heart eyes",
        "🤩" to "star struck wow", "😘" to "kiss blow", "😋" to "yum tasty tongue",
        "😜" to "wink tongue silly", "🤪" to "zany crazy silly", "🤔" to "think hmm",
        "🤐" to "zipper quiet secret", "😐" to "neutral meh", "😑" to "expressionless meh",
        "🙄" to "eye roll", "😏" to "smirk", "😮" to "wow surprised open mouth",
        "😲" to "shocked astonished", "🥺" to "pleading puppy eyes beg", "😢" to "cry sad tear",
        "😭" to "sob cry bawl", "😱" to "scream fear shock", "😤" to "huff angry",
        "😡" to "angry mad rage", "😠" to "angry mad", "🤬" to "swearing curse angry",
        "💀" to "skull dead", "💩" to "poop", "🤡" to "clown", "👻" to "ghost boo",
        "👽" to "alien", "👋" to "wave hi hello bye", "👌" to "ok perfect", "✌️" to "peace victory",
        "🤞" to "fingers crossed luck", "🤟" to "love you", "🤘" to "rock horns",
        "👈" to "point left", "👉" to "point right", "👆" to "point up", "👇" to "point down",
        "👍" to "thumbs up yes like good approve", "👎" to "thumbs down no dislike bad",
        "✊" to "fist power", "👊" to "fist bump punch", "👏" to "clap applause",
        "🙌" to "raise hands celebrate praise", "🙏" to "pray thanks please hope namaste",
        "💪" to "muscle strong flex", "👀" to "eyes look", "🧠" to "brain",
        "🐶" to "dog puppy", "🐱" to "cat kitten", "🦊" to "fox", "🐻" to "bear", "🐼" to "panda",
        "🦁" to "lion", "🐷" to "pig", "🐸" to "frog", "🐵" to "monkey", "🦄" to "unicorn",
        "🐝" to "bee", "🦋" to "butterfly", "🐢" to "turtle", "🐍" to "snake", "🐙" to "octopus",
        "🐬" to "dolphin", "🐳" to "whale", "🦈" to "shark", "🐘" to "elephant", "🦒" to "giraffe",
        "🌵" to "cactus", "🌲" to "tree pine", "🌴" to "palm tree", "🌱" to "seedling plant grow",
        "🍀" to "clover luck", "🌹" to "rose flower", "🌸" to "cherry blossom flower",
        "🌻" to "sunflower", "🌙" to "moon night", "⭐" to "star", "✨" to "sparkles shine",
        "⚡" to "lightning bolt zap", "🔥" to "fire lit hot flame", "🌈" to "rainbow",
        "☀️" to "sun sunny", "❄️" to "snow cold", "🍎" to "apple", "🍌" to "banana",
        "🍉" to "watermelon", "🍓" to "strawberry", "🍑" to "peach", "🍕" to "pizza",
        "🍔" to "burger hamburger", "🍟" to "fries", "🌮" to "taco", "🍦" to "ice cream",
        "🎂" to "cake birthday", "🍰" to "cake", "🍪" to "cookie", "☕" to "coffee tea",
        "🍺" to "beer", "🍻" to "cheers beer", "🍷" to "wine", "🥂" to "cheers champagne toast",
        "⚽" to "soccer football", "🏀" to "basketball", "🏈" to "football", "⚾" to "baseball",
        "🎾" to "tennis", "🏆" to "trophy win", "🥇" to "gold medal first win", "🎯" to "target bullseye",
        "🎮" to "game controller", "🎲" to "dice", "🎨" to "art paint", "🎤" to "mic sing karaoke",
        "🎸" to "guitar", "🚗" to "car", "✈️" to "plane flight travel", "🚀" to "rocket launch",
        "🚲" to "bike bicycle", "🏠" to "home house", "⛺" to "camp tent", "🌋" to "volcano",
        "🏖️" to "beach", "📱" to "phone mobile", "💻" to "laptop computer", "📷" to "camera photo",
        "💡" to "idea lightbulb", "💰" to "money bag", "💵" to "money dollar cash", "💳" to "credit card",
        "💎" to "diamond gem", "🔑" to "key", "🔒" to "lock secure", "🎁" to "gift present",
        "🎈" to "balloon party", "🎉" to "party celebrate tada", "🎊" to "confetti party",
        "🔔" to "bell notification", "📌" to "pin", "📝" to "note memo write", "✂️" to "scissors cut",
        "✏️" to "pencil write", "🔍" to "search magnify find", "❤️" to "heart love red",
        "🧡" to "orange heart", "💛" to "yellow heart", "💚" to "green heart", "💙" to "blue heart",
        "💜" to "purple heart", "🖤" to "black heart", "🤍" to "white heart", "💔" to "broken heart",
        "💕" to "hearts love", "💯" to "hundred perfect score", "✅" to "check done yes correct",
        "❌" to "x no wrong cross", "⭕" to "circle o", "🛑" to "stop", "❗" to "exclamation",
        "❓" to "question", "⚠️" to "warning caution", "♻️" to "recycle", "🏁" to "checkered flag finish",
        "🚩" to "red flag", "🏳️‍🌈" to "pride rainbow flag lgbtq", "🇺🇸" to "usa america flag",
    )

    /**
     * Emoji whose keyword string contains [query] (case-insensitive). Empty
     * query returns nothing — callers show the full category grid instead.
     */
    fun search(query: String): List<String> {
        val q = query.trim().lowercase()
        if (q.isEmpty()) return emptyList()
        return all.filter { keywords[it]?.contains(q) == true }
    }
}

/**
 * Most-recently-used reaction emoji, persisted in SharedPreferences (a
 * device-local UI preference, not synced). Shown as a row atop the picker;
 * recorded whenever a reaction is added. Mirrors iOS EmojiRecents.
 */
object EmojiRecents {
    private const val PREFS = "emoji_recents"
    private const val KEY = "recent"
    private const val CAP = 16
    private const val SEP = " "

    fun all(context: Context): List<String> {
        val raw = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY, null)
        return if (raw.isNullOrEmpty()) emptyList() else raw.split(SEP)
    }

    /** Move [emoji] to the front, dedupe, cap the list. */
    fun record(context: Context, emoji: String) {
        val list = all(context).toMutableList()
        list.remove(emoji)
        list.add(0, emoji)
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .putString(KEY, list.take(CAP).joinToString(SEP))
            .apply()
    }
}
