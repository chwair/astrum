use poise::serenity_prelude as serenity;

use crate::{Data, Error};

// routes to voice or regular handling
pub async fn post(
    ctx: &serenity::Context,
    data: &Data,
    message: &serenity::Message,
    starboard_channel: serenity::ChannelId,
    star_count: u64,
    emoji: &str,
) -> Result<u64, Error> {
    let is_voice = message
        .flags
        .is_some_and(|f| f.contains(serenity::MessageFlags::IS_VOICE_MESSAGE));

    // text header + gray embed in one message; capture the message id for future edits
    let header = text_header(message, star_count, emoji);
    let mut msg_builder = serenity::CreateMessage::new()
        .content(header)
        .embed(author_embed(message, is_voice));
    // include any embeds from the original message after the author embed
    for src in &message.embeds {
        msg_builder = msg_builder.embed(copy_embed(src));
    }
    let starboard_msg = starboard_channel
        .send_message(&ctx.http, msg_builder)
        .await?;

    // image/video attachments re-uploaded after the embed
    if !is_voice {
        let mut att_msg = serenity::CreateMessage::new();
        let mut has_media = false;
        for att in &message.attachments {
            let is_media = att
                .content_type
                .as_deref()
                .is_some_and(|ct| ct.starts_with("image/") || ct.starts_with("video/"));
            if is_media {
                match data.http_client.get(&att.url).send().await {
                    Ok(resp) => match resp.bytes().await {
                        Ok(bytes) => {
                            att_msg = att_msg.add_file(serenity::CreateAttachment::bytes(
                                bytes.to_vec(),
                                &att.filename,
                            ));
                            has_media = true;
                        }
                        Err(e) => tracing::warn!("failed to read attachment {}: {e}", att.filename),
                    },
                    Err(e) => tracing::warn!("failed to fetch attachment {}: {e}", att.filename),
                }
            }
        }
        if has_media {
            starboard_channel.send_message(&ctx.http, att_msg).await?;
        }
    }

    // tenor gif or voice message (sent as a separate message)
    if is_voice {
        crate::voice::relay(ctx, data, message, starboard_channel).await?;
    } else if let Some(url) = tenor_only_url(&message.content) {
        starboard_channel
            .send_message(&ctx.http, serenity::CreateMessage::new().content(url))
            .await?;
    }

    Ok(starboard_msg.id.get())
}

// builds the first text message: optional reply context followed by the star count line
fn text_header(message: &serenity::Message, star_count: u64, emoji: &str) -> String {
    let mut text = reply_prefix(message);
    text.push_str(&format!("{emoji} {star_count} ({})", msg_link(message)));
    text
}

// returns the -# reply context lines for a message (empty string if not a reply)
pub fn reply_prefix(message: &serenity::Message) -> String {
    let mut text = String::new();
    if let Some(ref_msg) = &message.referenced_message {
        let ref_link = msg_link(ref_msg);
        let ref_name = display_name(&ref_msg.author);
        let ref_content = if !ref_msg.content.is_empty() {
            excerpt(&ref_msg.content, 200).unwrap_or_default()
        } else if !ref_msg.attachments.is_empty() {
            "*[attachment]*".to_string()
        } else {
            "*[no content]*".to_string()
        };
        text.push_str(&format!("-# ↳  Replying to **{ref_name}** ({ref_link})\n"));
        for line in ref_content.lines() {
            text.push_str(&format!("-# > {line}\n"));
        }
    }
    text
}

// gray embed with the author's pfp, name, timestamp, and message content
fn author_embed(message: &serenity::Message, is_voice: bool) -> serenity::CreateEmbed {
    let mut embed = serenity::CreateEmbed::new()
        .author(
            serenity::CreateEmbedAuthor::new(display_name(&message.author))
                .icon_url(message.author.face()),
        )
        .timestamp(message.timestamp)
        .color(serenity::Color::new(0x808080));

    if !is_voice {
        // tenor-only messages are sent as a plain link in part 3, not in the embed
        if !message.content.is_empty() && tenor_only_url(&message.content).is_none() {
            embed = embed.description(&message.content);
        }

    }

    embed
}

// returns the tenor url if the message content is solely a tenor link
fn tenor_only_url(content: &str) -> Option<String> {
    let s = content.trim();
    if s.contains(' ') || s.contains('\n') {
        return None;
    }
    if s.starts_with("https://tenor.com/") || s.starts_with("https://www.tenor.com/") || s.starts_with("https://klipy.com/") || (s.starts_with("https://") && s.ends_with(".gif")) {
        Some(s.to_string())
    } else {
        None
    }
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

// truncate text to at most max chars, returns none if empty
fn excerpt(text: &str, max: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    if text.len() <= max {
        Some(text.to_string())
    } else {
        Some(format!("{}…", &text[..max]))
    }
}
// converts a received embed into a createembed for re-sending (thx claude)
fn copy_embed(e: &serenity::Embed) -> serenity::CreateEmbed {
    let mut ce = serenity::CreateEmbed::new();
    if let Some(title) = &e.title {
        ce = ce.title(title);
    }
    if let Some(desc) = &e.description {
        ce = ce.description(desc);
    }
    if let Some(url) = &e.url {
        ce = ce.url(url);
    }
    if let Some(color) = e.colour {
        ce = ce.color(color);
    }
    if let Some(ts) = e.timestamp {
        ce = ce.timestamp(ts);
    }
    if let Some(img) = &e.image {
        ce = ce.image(&img.url);
    }
    if let Some(thumb) = &e.thumbnail {
        ce = ce.thumbnail(&thumb.url);
    }
    if let Some(author) = &e.author {
        let mut ea = serenity::CreateEmbedAuthor::new(&author.name);
        if let Some(icon) = &author.icon_url {
            ea = ea.icon_url(icon);
        }
        if let Some(url) = &author.url {
            ea = ea.url(url);
        }
        ce = ce.author(ea);
    }
    if let Some(footer) = &e.footer {
        ce = ce.footer(serenity::CreateEmbedFooter::new(&footer.text));
    }
    for field in &e.fields {
        ce = ce.field(&field.name, &field.value, field.inline);
    }
    ce
}