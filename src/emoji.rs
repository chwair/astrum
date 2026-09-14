use poise::serenity_prelude as serenity;

pub const MAX_EMOJIS: usize = 10;

// parses a custom emoji into (animated, name, id).
// accepts the canonical "<a:name:id>" form and the legacy "name:id" form.
fn parse_custom(s: &str) -> Option<(bool, String, u64)> {
    let (animated, rest) = match s.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        Some(inner) => match inner.strip_prefix("a:") {
            Some(rest) => (true, rest),
            None => (false, inner.strip_prefix(':')?),
        },
        None => (false, s),
    };
    let (name, id) = rest.split_once(':')?;
    if name.is_empty() || name.contains(':') {
        return None;
    }
    Some((animated, name.to_string(), id.parse().ok()?))
}

// validates user input and returns the form we store: a unicode emoji or "<a:name:id>"
pub fn normalize(input: &str) -> Option<String> {
    let s = input.trim();
    if let Some((animated, name, id)) = parse_custom(s) {
        let prefix = if animated { "a" } else { "" };
        return Some(format!("<{prefix}:{name}:{id}>"));
    }
    // unicode emoji: a short run of non-ascii chars with no whitespace
    if s.is_empty() || s.is_ascii() || s.chars().count() > 16 || s.chars().any(char::is_whitespace) {
        return None;
    }
    Some(s.to_string())
}

// returns true if the reaction is the configured emoji.
// custom emoji are matched by id, with a fallback to the legacy name-only config form.
pub fn matches(reaction: &serenity::ReactionType, configured: &str) -> bool {
    match reaction {
        serenity::ReactionType::Unicode(s) => s == configured,
        serenity::ReactionType::Custom { id, name, .. } => match parse_custom(configured) {
            Some((_, _, cid)) => id.get() == cid,
            None => name.as_deref() == Some(configured),
        },
        _ => false,
    }
}

// returns the configured emoji this reaction matches, if any
pub fn find<'a>(reaction: &serenity::ReactionType, configured: &'a [String]) -> Option<&'a String> {
    configured.iter().find(|e| matches(reaction, e))
}

// converts a stored emoji into a reaction type usable on buttons.
// legacy name-only entries have no id, so they can't be rendered as an emoji.
pub fn reaction_type(configured: &str) -> Option<serenity::ReactionType> {
    match parse_custom(configured) {
        Some((animated, name, id)) => Some(serenity::ReactionType::Custom {
            animated,
            id: serenity::EmojiId::new(id),
            name: Some(name),
        }),
        None if !configured.is_ascii() => {
            Some(serenity::ReactionType::Unicode(configured.to_string()))
        }
        None => None,
    }
}
