use poise::serenity_prelude as serenity;
use serde_json::{json, Value};

use crate::{Context, Error};

// message flags
pub const IS_COMPONENTS_V2: u64 = 1 << 15;
pub const EPHEMERAL: u64 = 1 << 6;

// button styles
pub const PRIMARY: u8 = 1;
pub const DANGER: u8 = 4;

// a gallery holds at most ten items
pub const MAX_GALLERY: usize = 10;

pub fn text(content: impl Into<String>) -> Value {
    json!({ "type": 10, "content": content.into() })
}

pub fn section(components: Vec<Value>, accessory: Value) -> Value {
    json!({ "type": 9, "components": components, "accessory": accessory })
}

pub fn thumbnail(url: impl Into<String>) -> Value {
    json!({ "type": 11, "media": { "url": url.into() } })
}

pub fn gallery(urls: &[String]) -> Value {
    let items: Vec<Value> = urls
        .iter()
        .take(MAX_GALLERY)
        .map(|url| json!({ "media": { "url": url } }))
        .collect();
    json!({ "type": 12, "items": items })
}

// an uploaded file, shown as the attachment card discord draws for it. audio lands here too:
// a real voice message can't carry components, so its card is as close as it gets
pub fn file(url: impl Into<String>) -> Value {
    json!({ "type": 13, "file": { "url": url.into() } })
}

pub fn separator() -> Value {
    json!({ "type": 14, "divider": true, "spacing": 1 })
}

// no accent colour: a bare container reads as part of the channel instead of as an embed
pub fn container(components: Vec<Value>) -> Value {
    json!({ "type": 17, "components": components })
}

pub fn action_row(components: Vec<Value>) -> Value {
    json!({ "type": 1, "components": components })
}

// a clickable button; it shows an emoji when one is given, otherwise the label
pub fn button(
    custom_id: &str,
    style: u8,
    label: Option<&str>,
    emoji: Option<&serenity::ReactionType>,
    disabled: bool,
) -> Value {
    let mut button = json!({
        "type": 2,
        "style": style,
        "custom_id": custom_id,
        "disabled": disabled,
    });
    match emoji {
        Some(emoji) => {
            button["emoji"] = serde_json::to_value(emoji).unwrap_or(Value::Null);
        }
        None => {
            button["label"] = json!(label.unwrap_or("?"));
        }
    }
    button
}

// interaction responses
//
// poise and serenity 0.12 have no components v2 builders, so responses are sent as raw
// payloads through serenity's http client

// replies to a slash command with an ephemeral components v2 message
pub async fn reply(ctx: Context<'_>, components: Vec<Value>) -> Result<(), Error> {
    let ctx = match ctx {
        poise::Context::Application(ctx) => ctx,
        poise::Context::Prefix(_) => return Ok(()), // commands are slash only
    };
    let payload = json!({
        "type": 4,
        "data": { "flags": EPHEMERAL | IS_COMPONENTS_V2, "components": components },
    });
    ctx.serenity_context
        .http
        .create_interaction_response(ctx.interaction.id, &ctx.interaction.token, &payload, vec![])
        .await?;
    ctx.has_sent_initial_response
        .store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

// replaces the message an interaction came from
pub async fn update(
    ctx: &serenity::Context,
    id: serenity::InteractionId,
    token: &str,
    components: Vec<Value>,
) -> Result<(), Error> {
    let payload = json!({
        "type": 7,
        "data": { "flags": IS_COMPONENTS_V2, "components": components },
    });
    ctx.http
        .create_interaction_response(id, token, &payload, vec![])
        .await?;
    Ok(())
}

// answers an interaction with an ephemeral one-liner
pub async fn notice(
    ctx: &serenity::Context,
    id: serenity::InteractionId,
    token: &str,
    content: &str,
) -> Result<(), Error> {
    let payload = json!({
        "type": 4,
        "data": {
            "flags": EPHEMERAL | IS_COMPONENTS_V2,
            "components": [text(content)],
        },
    });
    ctx.http
        .create_interaction_response(id, token, &payload, vec![])
        .await?;
    Ok(())
}
