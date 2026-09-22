use poise::serenity_prelude as serenity;

pub const MAX_EMOJIS: usize = 10;

// what one reaction is worth when no weight is configured for it
pub const DEFAULT_WEIGHT: f64 = 1.0;
// a single reaction can't be worth more than this, so a typo can't star everything
pub const MAX_WEIGHT: f64 = 100.0;

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

// validates a weight typed by a user. accepts a decimal ("0.5") or a fraction ("1/3"),
// so an emoji can be made to count for less than a whole star
pub fn parse_weight(input: &str) -> Option<f64> {
    let s = input.trim();
    let weight = match s.split_once('/') {
        Some((num, den)) => {
            let den: f64 = den.trim().parse().ok()?;
            let num: f64 = num.trim().parse().ok()?;
            match den == 0.0 {
                true => return None,
                false => num / den,
            }
        }
        None => s.parse().ok()?,
    };
    (weight.is_finite() && weight > 0.0 && weight <= MAX_WEIGHT).then_some(weight)
}

// renders a weight the short way: whole numbers plain, everything else to two decimals
pub fn format_weight(weight: f64) -> String {
    if (weight - weight.round()).abs() < 1e-9 {
        return format!("{}", weight.round());
    }
    format!("{weight:.2}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_weights_as_decimals_and_fractions() {
        assert_eq!(parse_weight("2"), Some(2.0));
        assert_eq!(parse_weight(" 0.5 "), Some(0.5));
        assert_eq!(parse_weight("1/3"), Some(1.0 / 3.0));
        // nothing that would make a reaction free, negative or absurdly heavy
        assert_eq!(parse_weight("0"), None);
        assert_eq!(parse_weight("-1"), None);
        assert_eq!(parse_weight("1/0"), None);
        assert_eq!(parse_weight("lots"), None);
        assert_eq!(parse_weight("101"), None);
    }

    #[test]
    fn shows_whole_weights_without_a_decimal_point() {
        assert_eq!(format_weight(2.0), "2");
        assert_eq!(format_weight(0.5), "0.5");
        assert_eq!(format_weight(1.0 / 3.0), "0.33");
    }
}
