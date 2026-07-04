import Foundation

/// A curated, hand-maintained emoji catalog for the reaction picker (docs/33).
/// Not the full Unicode set — a few hundred common emoji grouped into the same
/// categories on every platform, so the picker looks and behaves identically on
/// iOS / Android / Desktop. Keep the categories and their order in sync when
/// porting.
enum EmojiCategory: String, CaseIterable, Identifiable {
    case smileys = "Smileys & People"
    case animals = "Animals & Nature"
    case food    = "Food & Drink"
    case activity = "Activity"
    case travel  = "Travel & Places"
    case objects = "Objects"
    case symbols = "Symbols"
    case flags   = "Flags"

    var id: String { rawValue }

    /// Compact SF Symbol used for the category tab strip.
    var symbol: String {
        switch self {
        case .smileys:  return "face.smiling"
        case .animals:  return "pawprint"
        case .food:     return "fork.knife"
        case .activity: return "figure.run"
        case .travel:   return "car"
        case .objects:  return "lightbulb"
        case .symbols:  return "number"
        case .flags:    return "flag"
        }
    }
}

enum EmojiData {
    /// The six quick-reaction emoji shown in the reaction bar (matches the prior
    /// long-press palette). Kept here so every surface pulls from one place.
    static let quick = ["👍", "❤️", "😂", "😮", "😢", "🙏"]

    /// Emoji grouped by category, in display order.
    static let byCategory: [(EmojiCategory, [String])] = [
        (.smileys, [
            "😀","😃","😄","😁","😆","😅","🤣","😂","🙂","🙃","😉","😊","😇","🥰","😍","🤩",
            "😘","😗","😚","😙","😋","😛","😜","🤪","😝","🤑","🤗","🤭","🤫","🤔","🤐","🤨",
            "😐","😑","😶","😏","😒","🙄","😬","🤥","😌","😔","😪","🤤","😴","😷","🤒","🤕",
            "🤢","🤮","🤧","🥵","🥶","🥴","😵","🤯","🤠","🥳","😎","🤓","🧐","😕","😟","🙁",
            "😮","😯","😲","😳","🥺","😦","😧","😨","😰","😥","😢","😭","😱","😖","😣","😞",
            "😓","😩","😫","🥱","😤","😡","😠","🤬","😈","👿","💀","💩","🤡","👹","👻","👽",
            "🙈","🙉","🙊","👋","🤚","✋","🖖","👌","🤏","✌️","🤞","🤟","🤘","🤙","👈","👉",
            "👆","👇","☝️","👍","👎","✊","👊","🤛","🤜","👏","🙌","👐","🤲","🙏","💪","🦾",
            "👀","👁️","👄","🧠","🫀","🦷","👶","🧒","👦","👧","🧑","👨","👩","🧓","👴","👵",
        ]),
        (.animals, [
            "🐶","🐱","🐭","🐹","🐰","🦊","🐻","🐼","🐨","🐯","🦁","🐮","🐷","🐸","🐵","🐔",
            "🐧","🐦","🐤","🦆","🦅","🦉","🦇","🐺","🐗","🐴","🦄","🐝","🐛","🦋","🐌","🐞",
            "🐜","🦗","🕷️","🦂","🐢","🐍","🦎","🦖","🐙","🦑","🦐","🦀","🐠","🐟","🐬","🐳",
            "🐋","🦈","🐊","🐅","🐆","🦓","🦍","🐘","🦏","🐪","🐫","🦒","🐃","🐂","🐄","🐎",
            "🐖","🐏","🐑","🐐","🦌","🐕","🐩","🐈","🐓","🦃","🕊️","🐇","🐁","🐀","🌵","🎄",
            "🌲","🌳","🌴","🌱","🌿","☘️","🍀","🎍","🍃","🍂","🍁","🌷","🌹","🌺","🌸","🌼",
            "🌻","🌞","🌝","🌚","🌙","⭐","🌟","✨","⚡","🔥","🌈","☀️","⛅","☁️","🌧️","❄️",
        ]),
        (.food, [
            "🍏","🍎","🍐","🍊","🍋","🍌","🍉","🍇","🍓","🫐","🍈","🍒","🍑","🥭","🍍","🥥",
            "🥝","🍅","🍆","🥑","🥦","🥬","🥒","🌶️","🌽","🥕","🧄","🧅","🥔","🍠","🥐","🍞",
            "🥖","🥨","🧀","🥚","🍳","🧇","🥞","🥓","🥩","🍗","🍖","🌭","🍔","🍟","🍕","🥪",
            "🌮","🌯","🥙","🧆","🥘","🍝","🍜","🍲","🍛","🍣","🍱","🥟","🍤","🍙","🍚","🍘",
            "🍥","🥠","🍢","🍡","🍧","🍨","🍦","🥧","🧁","🍰","🎂","🍮","🍭","🍬","🍫","🍿",
            "🍩","🍪","🌰","🥜","🍯","🥛","🍼","☕","🍵","🧃","🥤","🍶","🍺","🍻","🥂","🍷",
            "🥃","🍸","🍹","🍾","🧉","🧊",
        ]),
        (.activity, [
            "⚽","🏀","🏈","⚾","🥎","🎾","🏐","🏉","🥏","🎱","🪀","🏓","🏸","🏒","🏑","🥍",
            "🏏","🥅","⛳","🪁","🏹","🎣","🤿","🥊","🥋","🎽","🛹","🛷","⛸️","🥌","🎿","⛷️",
            "🏂","🪂","🏋️","🤼","🤸","⛹️","🤺","🤾","🏌️","🏇","🧘","🏄","🏊","🤽","🚣","🧗",
            "🚵","🚴","🏆","🥇","🥈","🥉","🏅","🎖️","🏵️","🎗️","🎫","🎟️","🎪","🤹","🎭","🎨",
            "🎬","🎤","🎧","🎼","🎹","🥁","🎷","🎺","🎸","🪕","🎻","🎲","♟️","🎯","🎳","🎮",
            "🎰","🧩",
        ]),
        (.travel, [
            "🚗","🚕","🚙","🚌","🚎","🏎️","🚓","🚑","🚒","🚐","🚚","🚛","🚜","🛴","🚲","🛵",
            "🏍️","🚨","🚔","🚍","🚘","🚖","🚡","🚠","🚟","🚃","🚋","🚞","🚝","🚄","🚅","🚈",
            "🚂","🚆","🚇","🚊","🚉","✈️","🛫","🛬","🛩️","💺","🚀","🛸","🚁","🛶","⛵","🚤",
            "🛥️","🛳️","⛴️","🚢","⚓","⛽","🚧","🚦","🚥","🗺️","🗿","🗽","🗼","🏰","🏯","🏟️",
            "🎡","🎢","🎠","⛲","⛱️","🏖️","🏝️","🏜️","🌋","⛰️","🏔️","🗻","🏕️","⛺","🏠","🏡",
            "🏘️","🏢","🏬","🏣","🏥","🏦","🏨","🏪","🏫","🏛️","⛪","🕌","🕍","🛕","🌃","🌆",
            "🌇","🌉","🌌","🌁",
        ]),
        (.objects, [
            "⌚","📱","💻","⌨️","🖥️","🖨️","🖱️","💽","💾","💿","📀","📷","📸","📹","🎥","📽️",
            "📞","☎️","📟","📠","📺","📻","🎙️","⏱️","⏲️","⏰","🕰️","⌛","⏳","🔋","🔌","💡",
            "🔦","🕯️","🧯","🛢️","💸","💵","💴","💶","💷","💰","💳","💎","⚖️","🔧","🔨","⚒️",
            "🛠️","⛏️","🔩","⚙️","🧰","🧲","🔫","💣","🧨","🔪","🗡️","⚔️","🛡️","🚬","⚰️","🏺",
            "🔮","📿","🧿","💈","🔭","🔬","🕳️","💊","💉","🩸","🌡️","🧹","🧺","🧻","🚽","🚰",
            "🛁","🛀","🧼","🪒","🧽","🔑","🗝️","🚪","🛋️","🛏️","🖼️","🛍️","🎁","🎈","🎏","🎀",
            "📦","📫","📮","📯","📜","📃","📑","📊","📈","📉","📇","🗂️","📅","📆","📋","📌",
            "📎","🖇️","📏","📐","✂️","🖊️","✏️","📝","🔍","🔒","🔓","📖","📚","🔔",
        ]),
        (.symbols, [
            "❤️","🧡","💛","💚","💙","💜","🖤","🤍","🤎","💔","❣️","💕","💞","💓","💗","💖",
            "💘","💝","💟","☮️","✝️","☪️","🕉️","☸️","✡️","🔯","🕎","☯️","☦️","🛐","⛎","♈",
            "♉","♊","♋","♌","♍","♎","♏","♐","♑","♒","♓","🆔","⚛️","🉑","☢️","☣️",
            "📴","📳","🈶","🈚","🈸","🈺","🈷️","✴️","🆚","💮","🉐","㊙️","㊗️","🈴","🈵","🈹",
            "❗","❕","❓","❔","‼️","⁉️","💯","🔅","🔆","〽️","⚠️","🚸","🔱","⚜️","🔰","♻️",
            "✅","🈯","💹","❇️","✳️","❎","🌐","💠","Ⓜ️","🌀","💤","🏧","🚾","♿","🅿️","🈳",
            "🚹","🚺","🚼","⚧️","🚻","🔞","📵","🚭","❌","⭕","🛑","⛔","📛","🚫","💢","♨️",
            "🔟","🔢","➕","➖","➗","✖️","♾️","💲","💱","™️","©️","®️","👁️‍🗨️","🔚","🔙","🔛",
        ]),
        (.flags, [
            "🏁","🚩","🎌","🏴","🏳️","🏳️‍🌈","🏳️‍⚧️","🏴‍☠️","🇺🇳","🇺🇸","🇨🇦","🇬🇧","🇮🇪","🇫🇷","🇩🇪","🇪🇸",
            "🇮🇹","🇵🇹","🇳🇱","🇧🇪","🇨🇭","🇦🇹","🇸🇪","🇳🇴","🇩🇰","🇫🇮","🇵🇱","🇺🇦","🇷🇺","🇬🇷","🇹🇷","🇮🇱",
            "🇸🇦","🇦🇪","🇪🇬","🇿🇦","🇳🇬","🇰🇪","🇮🇳","🇵🇰","🇧🇩","🇨🇳","🇯🇵","🇰🇷","🇹🇼","🇭🇰","🇹🇭","🇻🇳",
            "🇵🇭","🇮🇩","🇲🇾","🇸🇬","🇦🇺","🇳🇿","🇧🇷","🇦🇷","🇨🇱","🇨🇴","🇵🇪","🇲🇽",
        ]),
    ]

    /// Flat list for search.
    static let all: [String] = byCategory.flatMap { $0.1 }

    /// Search keywords for the commonly-searched emoji. Emoji without an entry
    /// stay browsable by category but won't surface in text search — extend this
    /// map as needed. Keep in sync across platforms.
    static let keywords: [String: String] = [
        "😀": "grin happy smile", "😃": "happy smile joy", "😄": "happy smile laugh",
        "😁": "grin beam happy", "😆": "laugh happy", "😅": "sweat laugh nervous",
        "🤣": "rofl laugh lol", "😂": "joy laugh cry lol tears", "🙂": "slight smile",
        "🙃": "upside down silly", "😉": "wink", "😊": "blush smile happy",
        "😇": "angel innocent halo", "🥰": "love hearts adore", "😍": "love heart eyes",
        "🤩": "star struck wow", "😘": "kiss blow", "😋": "yum tasty tongue",
        "😜": "wink tongue silly", "🤪": "zany crazy silly", "🤔": "think hmm",
        "🤐": "zipper quiet secret", "😐": "neutral meh", "😑": "expressionless meh",
        "🙄": "eye roll", "😏": "smirk", "😴": "sleep tired zzz", "😪": "sleepy tired",
        "😷": "mask sick", "🤒": "sick fever", "🤢": "sick nausea gross", "🤮": "vomit sick",
        "🥵": "hot heat sweat", "🥶": "cold freeze", "🤯": "mind blown shock",
        "🥳": "party celebrate", "😎": "cool sunglasses", "🤓": "nerd geek",
        "😕": "confused", "🙁": "frown sad", "😮": "wow surprised open mouth",
        "😲": "shocked astonished", "🥺": "pleading puppy eyes beg", "😢": "cry sad tear",
        "😭": "sob cry bawl", "😱": "scream fear shock", "😤": "huff angry",
        "😡": "angry mad rage", "😠": "angry mad", "🤬": "swearing curse angry",
        "💀": "skull dead", "💩": "poop", "🤡": "clown", "👻": "ghost boo",
        "👽": "alien", "👋": "wave hi hello bye", "👌": "ok perfect", "✌️": "peace victory",
        "🤞": "fingers crossed luck", "🤟": "love you", "🤘": "rock horns",
        "👈": "point left", "👉": "point right", "👆": "point up", "👇": "point down",
        "👍": "thumbs up yes like good approve", "👎": "thumbs down no dislike bad",
        "✊": "fist power", "👊": "fist bump punch", "👏": "clap applause",
        "🙌": "raise hands celebrate praise", "🙏": "pray thanks please hope namaste",
        "💪": "muscle strong flex", "👀": "eyes look", "🧠": "brain",
        "🐶": "dog puppy", "🐱": "cat kitten", "🦊": "fox", "🐻": "bear", "🐼": "panda",
        "🦁": "lion", "🐷": "pig", "🐸": "frog", "🐵": "monkey", "🦄": "unicorn",
        "🐝": "bee", "🦋": "butterfly", "🐢": "turtle", "🐍": "snake", "🐙": "octopus",
        "🐬": "dolphin", "🐳": "whale", "🦈": "shark", "🐘": "elephant", "🦒": "giraffe",
        "🌵": "cactus", "🌲": "tree pine", "🌴": "palm tree", "🌱": "seedling plant grow",
        "🍀": "clover luck", "🌹": "rose flower", "🌸": "cherry blossom flower",
        "🌻": "sunflower", "🌙": "moon night", "⭐": "star", "✨": "sparkles shine",
        "⚡": "lightning bolt zap", "🔥": "fire lit hot flame", "🌈": "rainbow",
        "☀️": "sun sunny", "❄️": "snow cold", "🍎": "apple", "🍌": "banana",
        "🍉": "watermelon", "🍓": "strawberry", "🍑": "peach", "🍕": "pizza",
        "🍔": "burger hamburger", "🍟": "fries", "🌮": "taco", "🍦": "ice cream",
        "🎂": "cake birthday", "🍰": "cake", "🍪": "cookie", "☕": "coffee tea",
        "🍺": "beer", "🍻": "cheers beer", "🍷": "wine", "🥂": "cheers champagne toast",
        "⚽": "soccer football", "🏀": "basketball", "🏈": "football", "⚾": "baseball",
        "🎾": "tennis", "🏆": "trophy win", "🥇": "gold medal first win", "🎯": "target bullseye",
        "🎮": "game controller", "🎲": "dice", "🎨": "art paint", "🎤": "mic sing karaoke",
        "🎸": "guitar", "🚗": "car", "✈️": "plane flight travel", "🚀": "rocket launch",
        "🚲": "bike bicycle", "🏠": "home house", "⛺": "camp tent", "🌋": "volcano",
        "🏖️": "beach", "📱": "phone mobile", "💻": "laptop computer", "📷": "camera photo",
        "💡": "idea lightbulb", "💰": "money bag", "💵": "money dollar cash", "💳": "credit card",
        "💎": "diamond gem", "🔑": "key", "🔒": "lock secure", "🎁": "gift present",
        "🎈": "balloon party", "🎉": "party celebrate tada", "🎊": "confetti party",
        "🔔": "bell notification", "📌": "pin", "📝": "note memo write", "✂️": "scissors cut",
        "✏️": "pencil write", "🔍": "search magnify find", "❤️": "heart love red",
        "🧡": "orange heart", "💛": "yellow heart", "💚": "green heart", "💙": "blue heart",
        "💜": "purple heart", "🖤": "black heart", "🤍": "white heart", "💔": "broken heart",
        "💕": "hearts love", "💯": "hundred perfect score", "✅": "check done yes correct",
        "❌": "x no wrong cross", "⭕": "circle o", "🛑": "stop", "❗": "exclamation",
        "❓": "question", "⚠️": "warning caution", "♻️": "recycle", "🏁": "checkered flag finish",
        "🚩": "red flag", "🏳️‍🌈": "pride rainbow flag lgbtq", "🇺🇸": "usa america flag",
    ]

    /// Emoji whose keyword string contains `query` (case-insensitive). Empty
    /// query returns nothing — callers show the full category grid instead.
    static func search(_ query: String) -> [String] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return [] }
        return all.filter { keywords[$0]?.contains(q) ?? false }
    }
}

/// Most-recently-used reaction emoji, persisted locally (a device-local UI
/// preference, not synced). Shown as a row atop the picker; recorded whenever a
/// reaction is *added*.
enum EmojiRecents {
    private static let key = "recentReactionEmoji"
    private static let cap = 16

    static func all() -> [String] {
        UserDefaults.standard.stringArray(forKey: key) ?? []
    }

    /// Move `emoji` to the front, dedupe, cap the list.
    static func record(_ emoji: String) {
        var list = all()
        list.removeAll { $0 == emoji }
        list.insert(emoji, at: 0)
        UserDefaults.standard.set(Array(list.prefix(cap)), forKey: key)
    }
}
