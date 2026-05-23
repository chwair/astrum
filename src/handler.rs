use poise::serenity_prelude as serenity;

use crate::{Data, Error};

pub async fn handle_event(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    data: &Data,
) -> Result<(), Error> {
    if let serenity::FullEvent::ReactionAdd { add_reaction } = event {
        if let Err(e) = on_reaction_add(ctx, add_reaction, data).await {
            tracing::error!("error handling reaction: {e:?}");
        }
    }
    Ok(())
}

async fn on_reaction_add(
    ctx: &serenity::Context,
    reaction: &serenity::Reaction,
    data: &Data,
) -> Result<(), Error> {
    let guild_id = match reaction.guild_id {
        Some(id) => id,
        None => return Ok(()), // ignore dms
    };

    // load config and bail early if the starboard isn't set up
    let (starboard_channel_id, min_stars, emoji) = {
        let store = data.store.read().await;
        let cfg = store.guild(guild_id.get());
        match cfg.starboard_channel {
            Some(ch) => (serenity::ChannelId::new(ch), cfg.min_stars, cfg.starboard_emoji.clone()),
            None => return Ok(()),
        }
    };

    // only handle the configured emoji
    if !emoji_matches(&reaction.emoji, &emoji) {
        return Ok(());
    }

    // don't re-star messages already in the starboard channel
    if reaction.channel_id == starboard_channel_id {
        return Ok(());
    }

    let message = reaction
        .channel_id
        .message(&ctx.http, reaction.message_id)
        .await?;

    // count how many of the configured emoji are on this message
    let star_count = message
        .reactions
        .iter()
        .find(|r| emoji_matches(&r.reaction_type, &emoji))
        .map(|r| r.count)
        .unwrap_or(0);

    if star_count < min_stars {
        return Ok(());
    }

    // double-check under write lock to prevent duplicate posts
    {
        let mut store = data.store.write().await;
        if store.is_starred(guild_id.get(), message.id.get()) {
            return Ok(());
        }
        store.mark_starred(guild_id.get(), message.id.get());
        store.save().await?;
    }

    crate::starboard::post(ctx, data, &message, starboard_channel_id, star_count, &emoji).await
}

// returns true if the reaction matches the configured emoji string.
// supports unicode emoji and custom emoji matched by "name" or "name:id".
fn emoji_matches(reaction: &serenity::ReactionType, configured: &str) -> bool {
    match reaction {
        serenity::ReactionType::Unicode(s) => s == configured,
        serenity::ReactionType::Custom { name, id, .. } => {
            name.as_deref() == Some(configured)
                || format!("{}:{}", name.as_deref().unwrap_or(""), id) == configured
        }
        _ => false,
    }
}

