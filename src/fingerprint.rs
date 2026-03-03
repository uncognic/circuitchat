const WORDS: &[&str] = &[
    "apple", "arrow", "atlas", "basin", "bells", "birch", "blade", "blank", "bloom", "board",
    "bonds", "brave", "brick", "brine", "brook", "brush", "cable", "cairn", "canal", "cedar",
    "chalk", "chest", "civic", "clamp", "clasp", "cliff", "cloak", "clock", "cloud", "clover",
    "coast", "comet", "coral", "crane", "creek", "crest", "crisp", "cross", "crown", "curve",
    "delta", "depot", "depth", "digit", "ditch", "draft", "drain", "drift", "drill", "drove",
    "dunes", "eagle", "earth", "ember", "epoch", "falls", "fence", "ferry", "field", "fixed",
    "fjord", "flame", "flask", "flint", "float", "flood", "floor", "flute", "forge", "forth",
    "forum", "frost", "gable", "gauge", "gavel", "glade", "gland", "glare", "glass", "gleam",
    "globe", "gloom", "glove", "gorge", "grace", "grade", "grain", "grand", "grant", "graph",
    "grasp", "grass", "gravel", "grove", "guide", "guild", "haven", "hedge", "helix", "hills",
    "hinge", "holds", "holly", "hyena", "inlet", "ivory", "joint", "judge", "keyed", "knoll",
    "lance", "latch", "lathe", "ledge", "light", "linen", "links", "lumen", "marsh", "match",
    "maple", "merit", "meter", "mines", "mirth", "mists", "mocha", "modal", "morse", "mossy",
    "mount", "naval", "nerve", "north", "notch", "oaken", "ocean", "onset", "optic", "orbit",
    "order", "other", "otter", "oxide", "oxide", "pagoda", "parch", "patch", "paths", "pause",
    "peaks", "pearl", "pedal", "perch", "pilot", "pinch", "pixel", "plank", "plaza", "plumb",
    "plume", "polar", "porch", "pound", "press", "prism", "probe", "proof", "prose", "proud",
    "proxy", "pulse", "purge", "quill", "rails", "rains", "rally", "range", "rapid", "reach",
    "reeds", "relay", "ridge", "rivet", "roads", "robin", "rocks", "rouge", "round", "route",
    "rowel", "ruled", "ruins", "ryzen", "sable", "sands", "scale", "scout", "screw", "seals",
    "seam", "servo", "seven", "shade", "shaft", "shale", "sheen", "shelf", "shell", "shift",
    "shore", "sight", "sigma", "signs", "silks", "skiff", "slate", "sleet", "slope", "sluce",
    "smart", "smoke", "solar", "solid", "south", "spark", "spear", "spire", "spoke", "spray",
    "squad", "stack", "staff", "stage", "stair", "stake", "stale", "stall", "stave", "stays",
    "steam", "steel", "steep", "steer", "stern", "stick", "still", "stock", "stone", "store",
    "storm", "stout", "stove", "strap", "straw", "strip", "strut", "style", "sunny", "surge",
    "swamp", "swath", "swept", "swift", "sword", "table", "talon", "tapir", "taunt", "thorn",
    "three", "tidal", "tiger", "tiled", "timed", "titan", "torch", "tower", "towns", "trace",
    "track", "trail", "train", "tramp", "trawl", "tread", "treed", "trend", "tribe", "trick",
    "tried", "trine", "trios", "trout", "trove", "truss", "trust", "tuner", "tuned", "turbo",
    "twice", "twine", "under", "union", "unity", "until", "upper", "urban", "valve", "vapor",
    "vault", "visor", "vista", "vocal", "voles", "voter", "wader", "wands", "watch", "water",
    "watts", "waves", "weald", "wedge", "welds", "wharf", "wheat", "wheel", "whelk", "while",
    "whirl", "white", "wider", "winds", "witch", "woods", "works", "world", "wrath", "wrist",
    "wrote", "xenon", "yacht", "yards", "yokes", "zones",
];

pub fn derive_fingerprint(session_hash: &[u8]) -> String {
    let w = |offset: usize| {
        let idx = u16::from_be_bytes([session_hash[offset], session_hash[offset + 1]]) as usize
            % WORDS.len();
        WORDS[idx]
    };
    format!("{}-{}-{}-{}-{}-{}", w(0), w(2), w(4), w(6), w(8), w(10))
}
