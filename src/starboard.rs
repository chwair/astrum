use std::collections::HashMap;

use poise::serenity_prelude as serenity;
use serde_json::{json, Value};

use crate::api::{self, Upload};
use crate::components as ui;
use crate::{Data, Error};

// how much of the original message is shown before it's cut off
const MAX_CONTENT: usize = 1500;
// how much of a replied-to message fits on the single reply line
const MAX_REPLY: usize = 120;
// embeds carried over from the original message, capped to stay under the component limit
const MAX_EMBEDS: usize = 4;
// discord's per-file limit. anything bigger is linked rather than re-hosted, since re-uploading
// one past the starboard server's limit would fail the whole post
const MAX_UPLOAD: usize = 20 * 1024 * 1024;

// the inline avatar emojis a post needs, for whoever is shown in it
#[derive(Default)]
struct Avatars {
    author: Option<String>,
    reply: Option<String>,
}

// uploads or looks up the avatar of everyone the post names
async fn avatars(ctx: &serenity::Context, data: &Data, message: &serenity::Message) -> Avatars {
    Avatars {
        author: crate::avatar::inline(ctx, data, &message.author).await,
        reply: match &message.referenced_message {
            Some(replied_to) => crate::avatar::inline(ctx, data, &replied_to.author).await,
            None => None,
        },
    }
}

// posts a starred message to the starboard as a single components v2 message, returning its id
pub async fn post(
    ctx: &serenity::Context,
    data: &Data,
    message: &serenity::Message,
    starboard_channel: serenity::ChannelId,
    summary: &str,
) -> Result<u64, Error> {
    let (main, forwarded) = media_plan(message);
    let mut files = Vec::new();
    for plan in [&main, &forwarded] {
        files.append(&mut fetch_media(data, &plan.gallery).await);
        files.append(&mut fetch_media(data, &plan.files).await);
    }

    // everything that made it into the upload can be referenced by the components
    let media: HashMap<String, String> = files
        .iter()
        .map(|file| (file.name.clone(), format!("attachment://{}", file.name)))
        .collect();

    let components = build(message, summary, &media, &avatars(ctx, data, message).await);
    let starboard_msg_id = api::send(
        &data.http_client,
        &data.bot_token,
        starboard_channel,
        &components,
        files,
    )
    .await?;

    Ok(starboard_msg_id)
}

// rewrites an existing starboard post with a fresh star count, keeping its uploaded media
pub async fn update(
    ctx: &serenity::Context,
    data: &Data,
    message: &serenity::Message,
    starboard_channel: serenity::ChannelId,
    starboard_msg_id: u64,
    summary: &str,
) -> Result<(), Error> {
    // a components v2 edit replaces the whole component list, so the media already hosted on
    // the starboard message has to be pointed at again and kept in the attachment list
    let existing = starboard_channel
        .message(&ctx.http, serenity::MessageId::new(starboard_msg_id))
        .await?;
    let media: HashMap<String, String> = existing
        .attachments
        .iter()
        .map(|att| (att.filename.clone(), format!("attachment://{}", att.filename)))
        .collect();
    let keep: Vec<Value> = existing
        .attachments
        .iter()
        .map(|att| json!({ "id": att.id.get(), "filename": att.filename }))
        .collect();

    let components = build(message, summary, &media, &avatars(ctx, data, message).await);
    api::edit(
        &data.http_client,
        &data.bot_token,
        starboard_channel,
        starboard_msg_id,
        &components,
        &keep,
    )
    .await
}

// builds the whole post: one container for the message, one more per carried-over embed.
// media maps an upload name to the url the components should point at
fn build(
    message: &serenity::Message,
    summary: &str,
    media: &HashMap<String, String>,
    avatars: &Avatars,
) -> Vec<Value> {
    let (main, forwarded) = media_plan(message);

    // the reply context sits above the container, the way discord draws a reply
    let mut components = Vec::new();
    if let Some(prefix) = reply_prefix(message, avatars.reply.as_deref()) {
        components.push(ui::text(prefix));
    }

    // author row: avatar, name, then the time in the short form discord puts there
    let mut body = match avatars.author.as_deref() {
        Some(emoji) => format!("{emoji} **{}**", display_name(&message.author)),
        None => format!("**{}**", display_name(&message.author)),
    };
    body.push_str(&format!("  <t:{}:t>", message.timestamp.unix_timestamp()));
    if is_voice(message) {
        body.push_str("\n-# 🎤 voice message");
    } else if let Some(content) = content_text(message) {
        body.push('\n');
        body.push_str(&content);
    }
    let mut items = vec![ui::text(body)];

    // anything shown alongside the attachments: stickers, and a lone media link in the content
    let mut extra = sticker_urls(&message.sticker_items);
    if let Some(url) = lone_media_url(message.content.trim()) {
        extra.insert(0, url);
    }
    items.extend(attachments(&main, media, extra));

    // a forwarded message is quoted below the body, inside the same container
    if let Some(snapshot) = message.message_snapshots.first() {
        items.push(ui::separator());
        let mut quote = String::from("-# ↱ *Forwarded*\n");
        let content = excerpt(&snapshot.content, MAX_CONTENT).unwrap_or_default();
        for line in content.lines() {
            quote.push_str(&format!("> {line}\n"));
        }
        items.push(ui::text(quote));
        items.extend(attachments(
            &forwarded,
            media,
            sticker_urls(&snapshot.sticker_items),
        ));
    }

    components.push(ui::container(items));
    for embed in message.embeds.iter().take(MAX_EMBEDS) {
        if let Some(container) = embed_container(embed) {
            components.push(container);
        }
    }

    // the tally goes under the message, where discord puts reactions
    components.push(ui::text(format!(
        "-# {summary} · [jump]({})",
        msg_link(message)
    )));
    components
}

// renders an embed of the original message as its own container, since a components v2
// message can't carry embeds
fn embed_container(embed: &serenity::Embed) -> Option<Value> {
    // link previews of images and gifs are just the media
    if matches!(embed.kind.as_deref(), Some("gifv") | Some("image")) {
        let url = embed
            .thumbnail
            .as_ref()
            .map(|t| t.url.clone())
            .or_else(|| embed.image.as_ref().map(|i| i.url.clone()))?;
        return Some(ui::container(vec![ui::gallery(&[url])]));
    }

    let mut body = String::new();
    if let Some(author) = &embed.author {
        body.push_str(&format!("-# {}\n", author.name));
    }
    if let Some(title) = &embed.title {
        match &embed.url {
            Some(url) => body.push_str(&format!("### [{title}]({url})\n")),
            None => body.push_str(&format!("### {title}\n")),
        }
    }
    if let Some(description) = &embed.description {
        body.push_str(&format!("{description}\n"));
    }
    for field in &embed.fields {
        body.push_str(&format!("**{}**\n{}\n", field.name, field.value));
    }
    if let Some(footer) = &embed.footer {
        body.push_str(&format!("-# {}\n", footer.text));
    }

    let image = embed.image.as_ref().map(|i| i.url.clone());
    if body.trim().is_empty() && image.is_none() {
        return None;
    }

    let mut items = Vec::new();
    if !body.trim().is_empty() {
        let body = excerpt(&body, MAX_CONTENT).unwrap_or_default();
        items.push(match &embed.thumbnail {
            Some(thumb) => ui::section(vec![ui::text(body)], ui::thumbnail(&thumb.url)),
            None => ui::text(body),
        });
    }
    if let Some(url) = image {
        items.push(ui::gallery(&[url]));
    }

    Some(ui::container(items))
}

// media helpers

// the attachments queued for re-hosting, as (source url, upload name) pairs
type Planned = Vec<(String, String)>;

// what a message's attachments turn into
#[derive(Default)]
struct Plan {
    // images and videos, shown in a media gallery
    gallery: Planned,
    // everything else, voice notes included, shown as file components
    files: Planned,
    // attachments too big to re-host, linked instead of copied
    links: Vec<(String, String)>,
}

// how the attachments of a message and of anything it forwards get shown. upload names are
// derived the same way on every build, so a later edit can point at the files already uploaded
fn media_plan(message: &serenity::Message) -> (Plan, Plan) {
    let mut index = 0;
    let mut plan = |attachments: &[serenity::Attachment]| {
        let mut plan = Plan::default();
        for att in attachments {
            let name = format!("{index}-{}", att.filename);
            index += 1;

            // re-hosting something past the upload limit would fail the whole post
            if att.size as usize > MAX_UPLOAD {
                plan.links.push((att.filename.clone(), att.url.clone()));
                continue;
            }
            let showable = att
                .content_type
                .as_deref()
                .is_some_and(|ct| ct.starts_with("image/") || ct.starts_with("video/"));
            match showable {
                true => plan.gallery.push((att.url.clone(), name)),
                false => plan.files.push((att.url.clone(), name)),
            }
        }
        plan
    };

    let main = plan(&message.attachments);
    let forwarded = match message.message_snapshots.first() {
        Some(snapshot) => plan(&snapshot.attachments),
        None => Plan::default(),
    };
    (main, forwarded)
}

// renders the attachments of one plan: a gallery, then a card per file, then any links
fn attachments(plan: &Plan, media: &HashMap<String, String>, extra: Vec<String>) -> Vec<Value> {
    let mut items = Vec::new();

    let mut urls = resolve(&plan.gallery, media);
    urls.extend(extra);
    if !urls.is_empty() {
        items.push(ui::gallery(&urls));
    }
    for url in resolve(&plan.files, media) {
        items.push(ui::file(&url));
    }
    if !plan.links.is_empty() {
        let links: Vec<String> = plan
            .links
            .iter()
            .map(|(name, url)| format!("[{name}]({url})"))
            .collect();
        items.push(ui::text(format!("-# 📎 {}", links.join(" · "))));
    }

    items
}

// downloads the planned media, skipping anything that fails to come down
async fn fetch_media(data: &Data, planned: &Planned) -> Vec<Upload> {
    let mut files = Vec::new();
    for (url, name) in planned {
        match data.http_client.get(url).send().await {
            Ok(resp) => match resp.bytes().await {
                Ok(bytes) => files.push(Upload {
                    name: name.clone(),
                    bytes: bytes.to_vec(),
                }),
                Err(e) => tracing::warn!("failed to read attachment {name}: {e}"),
            },
            Err(e) => tracing::warn!("failed to fetch attachment {name}: {e}"),
        }
    }
    files
}

// looks up the url of every planned upload that actually made it onto the message
fn resolve(planned: &Planned, media: &HashMap<String, String>) -> Vec<String> {
    planned
        .iter()
        .filter_map(|(_, name)| media.get(name).cloned())
        .collect()
}

// sticker images, which components can point at directly on discord's cdn
fn sticker_urls(stickers: &[serenity::StickerItem]) -> Vec<String> {
    stickers
        .iter()
        .filter_map(|sticker| match sticker.format_type {
            // gif stickers live on media.discordapp.net, not the cdn host serenity builds
            serenity::StickerFormatType::Gif => Some(format!(
                "https://media.discordapp.net/stickers/{}.gif",
                sticker.id
            )),
            // lottie stickers aren't an image, so there's nothing to show
            serenity::StickerFormatType::Lottie => None,
            _ => sticker.image_url(),
        })
        .collect()
}

// text helpers

// renders each reacted emoji with its count, plus the unique reactor total when there's more than one
pub fn star_summary(counts: &[(String, u64)], total: u64) -> String {
    let mut parts: Vec<String> = counts
        .iter()
        .map(|(emoji, count)| format!("{emoji} {count}"))
        .collect();
    if counts.len() > 1 {
        parts.push(format!("· **{total}**"));
    }
    parts.join(" ")
}

// the quoted reply context shown above the post, if the message is a reply
fn reply_prefix(message: &serenity::Message, avatar: Option<&str>) -> Option<String> {
    let ref_msg = message.referenced_message.as_ref()?;
    let ref_content = if !ref_msg.content.is_empty() {
        excerpt(&ref_msg.content, MAX_REPLY).unwrap_or_default()
    } else if !ref_msg.attachments.is_empty() {
        "*[attachment]*".to_string()
    } else {
        "*[no content]*".to_string()
    };

    // one small line, the way discord renders a reply above a message
    let icon = match avatar {
        Some(emoji) => format!("{emoji} "),
        None => String::new(),
    };
    Some(format!(
        "-# ↳  {icon}[**{}**]({})  {}",
        display_name(&ref_msg.author),
        msg_link(ref_msg),
        ref_content.replace('\n', " ")
    ))
}

// the message body, dropped when it's only the link an embed already renders
fn content_text(message: &serenity::Message) -> Option<String> {
    let content = message.content.trim();
    if content.is_empty() || lone_media_url(content).is_some() {
        return None;
    }
    if message.embeds.iter().any(|e| e.url.as_deref() == Some(content)) {
        return None;
    }
    excerpt(content, MAX_CONTENT)
}

// a message whose content is a lone direct media link shows as media instead of as text
fn lone_media_url(content: &str) -> Option<String> {
    if !content.starts_with("https://") || content.split_whitespace().nth(1).is_some() {
        return None;
    }
    let path = content.split(['?', '#']).next().unwrap_or(content).to_lowercase();
    let extensions = [".gif", ".png", ".jpg", ".jpeg", ".webp", ".mp4", ".webm"];
    extensions
        .iter()
        .any(|ext| path.ends_with(ext))
        .then(|| content.to_string())
}

// builds a discord message link
pub fn msg_link(message: &serenity::Message) -> String {
    format!(
        "https://discord.com/channels/{}/{}/{}",
        message.guild_id.map_or(0, |g| g.get()),
        message.channel_id.get(),
        message.id.get(),
    )
}

// returns the display name (global name if set, otherwise username)
pub fn display_name(user: &serenity::User) -> String {
    user.global_name
        .as_deref()
        .unwrap_or(&user.name)
        .to_string()
}

// whether the message is a voice note
fn is_voice(message: &serenity::Message) -> bool {
    message
        .flags
        .is_some_and(|f| f.contains(serenity::MessageFlags::IS_VOICE_MESSAGE))
}

// truncate text to at most max chars, returns none if empty
fn excerpt(text: &str, max: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    match text.char_indices().nth(max) {
        Some((end, _)) => Some(format!("{}…", &text[..end])),
        None => Some(text.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // a starred message with an image, as the gateway sends it
    fn fixture() -> serenity::Message {
        serde_json::from_value(json!({
            "id": "2",
            "channel_id": "3",
            "guild_id": "4",
            "author": { "id": "5", "username": "nova", "global_name": "Nova", "discriminator": "0" },
            "content": "look at this",
            "timestamp": "2024-01-01T00:00:00.000000+00:00",
            "edited_timestamp": null,
            "tts": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [{
                "id": "6",
                "filename": "cat.png",
                "content_type": "image/png",
                "size": 1,
                "url": "https://cdn.discordapp.com/attachments/3/6/cat.png",
                "proxy_url": "https://media.discordapp.net/attachments/3/6/cat.png"
            }],
            "embeds": [],
            "pinned": false,
            "type": 0
        }))
        .expect("fixture should deserialize")
    }

    #[test]
    fn builds_one_uncoloured_container_with_the_uploaded_image() {
        let message = fixture();
        let (main, _) = media_plan(&message);
        assert_eq!(main.gallery, vec![(
            "https://cdn.discordapp.com/attachments/3/6/cat.png".to_string(),
            "0-cat.png".to_string(),
        )]);

        let media = HashMap::from([("0-cat.png".to_string(), "attachment://0-cat.png".to_string())]);
        let components = build(
            &message,
            "⭐ 3",
            &media,
            &Avatars { author: Some("<:av5:7>".into()), reply: None },
        );

        // the message is one uncoloured container, with the tally underneath it
        assert_eq!(components.len(), 2);
        let container = &components[0];
        assert_eq!(container["type"], 17);
        assert!(container["accent_color"].is_null(), "containers stay uncoloured");

        let items = container["components"].as_array().expect("container children");
        assert!(items.len() <= 10, "a container holds at most ten components");

        // author row: circular avatar emoji, name, then the time, all on one line
        assert_eq!(
            items[0]["content"],
            "<:av5:7> **Nova**  <t:1704067200:t>\nlook at this"
        );
        assert_eq!(items[1]["items"][0]["media"]["url"], "attachment://0-cat.png");

        assert_eq!(
            components[1]["content"],
            "-# ⭐ 3 · [jump](https://discord.com/channels/4/3/2)"
        );
    }

    #[test]
    fn shows_a_voice_note_as_a_file_card_inside_the_container() {
        let mut message = fixture();
        message.content = String::new();
        message.flags = Some(serenity::MessageFlags::IS_VOICE_MESSAGE);
        message.attachments[0].filename = "voice-message.ogg".to_string();
        message.attachments[0].content_type = Some("audio/ogg".to_string());

        let (main, _) = media_plan(&message);
        assert!(main.gallery.is_empty());
        assert_eq!(main.files[0].1, "0-voice-message.ogg");

        let media = HashMap::from([(
            "0-voice-message.ogg".to_string(),
            "attachment://0-voice-message.ogg".to_string(),
        )]);
        let items = build(&message, "⭐ 3", &media, &Avatars::default())[0]["components"].clone();

        assert_eq!(items[1]["type"], 13);
        assert_eq!(items[1]["file"]["url"], "attachment://0-voice-message.ogg");
    }

    #[test]
    fn links_attachments_too_big_to_re_host() {
        let mut message = fixture();
        message.attachments[0].size = (MAX_UPLOAD + 1) as u32;

        let (main, _) = media_plan(&message);
        assert!(main.gallery.is_empty() && main.files.is_empty());

        let items = build(&message, "⭐ 3", &HashMap::new(), &Avatars::default())[0]["components"]
            .clone();
        assert_eq!(
            items[1]["content"],
            "-# 📎 [cat.png](https://cdn.discordapp.com/attachments/3/6/cat.png)"
        );
    }

    #[test]
    fn puts_the_reply_context_on_one_line_above_the_container() {
        let mut message = fixture();
        message.referenced_message = Some(Box::new(fixture()));

        let avatars = Avatars {
            author: None,
            reply: Some("<:av5:7>".into()),
        };
        let components = build(&message, "⭐ 3", &HashMap::new(), &avatars);
        assert_eq!(
            components[0]["content"],
            "-# ↳  <:av5:7> [**Nova**](https://discord.com/channels/4/3/2)  look at this"
        );
        assert_eq!(components[1]["type"], 17);
    }

    #[test]
    fn drops_content_that_an_embed_already_renders() {
        let mut message = fixture();
        message.content = "https://tenor.com/view/cat-1".to_string();
        message.embeds = vec![serde_json::from_value(json!({
            "type": "gifv",
            "url": "https://tenor.com/view/cat-1",
            "thumbnail": { "url": "https://media.tenor.com/cat.gif" }
        }))
        .expect("embed should deserialize")];

        assert!(content_text(&message).is_none());

        let components = build(&message, "⭐ 3", &HashMap::new(), &Avatars::default());
        // the message container, the gif the embed stood for, then the tally
        assert_eq!(components.len(), 3);
        assert_eq!(
            components[1]["components"][0]["items"][0]["media"]["url"],
            "https://media.tenor.com/cat.gif"
        );
    }
}
