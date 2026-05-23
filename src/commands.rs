use poise::serenity_prelude as serenity;

use crate::{Context, Error};

/// configure the starboard for this server
#[poise::command(slash_command, guild_only, subcommands("channel", "stars", "emoji", "show"))]
pub async fn config(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// set the channel where starred messages are posted
#[poise::command(slash_command, guild_only, ephemeral, required_permissions = "ADMINISTRATOR")]
pub async fn channel(
    ctx: Context<'_>,
    #[description = "the starboard channel"] channel: serenity::GuildChannel,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only").get();
    {
        let mut store = ctx.data().store.write().await;
        store.guild_mut(guild_id).starboard_channel = Some(channel.id.get());
        store.save().await?;
    }
    ctx.say(format!("starboard channel set to <#{}>.", channel.id))
        .await?;
    Ok(())
}

/// set the minimum number of reactions to reach the starboard
#[poise::command(slash_command, guild_only, ephemeral, required_permissions = "ADMINISTRATOR")]
pub async fn stars(
    ctx: Context<'_>,
    #[description = "minimum reactions (must be at least 1)"] count: u64,
) -> Result<(), Error> {
    if count == 0 {
        ctx.say("minimum reactions must be at least 1").await?;
        return Ok(());
    }
    let guild_id = ctx.guild_id().expect("guild_only").get();
    {
        let mut store = ctx.data().store.write().await;
        store.guild_mut(guild_id).min_stars = count;
        store.save().await?;
    }
    ctx.say(format!("minimum reactions set to {count}")).await?;
    Ok(())
}

/// set the emoji used to star messages
#[poise::command(slash_command, guild_only, ephemeral, required_permissions = "ADMINISTRATOR")]
pub async fn emoji(
    ctx: Context<'_>,
    #[description = "emoji to use as the star reaction"] emoji: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only").get();
    {
        let mut store = ctx.data().store.write().await;
        store.guild_mut(guild_id).starboard_emoji = emoji.clone();
        store.save().await?;
    }
    ctx.say(format!("star emoji set to {emoji}")).await?;
    Ok(())
}

/// show the current starboard configuration
#[poise::command(slash_command, guild_only, ephemeral)]
pub async fn show(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only").get();
    let store = ctx.data().store.read().await;
    let cfg = store.guild(guild_id);

    let channel_str = cfg
        .starboard_channel
        .map(|id| format!("<#{id}>"))
        .unwrap_or_else(|| "*not set*".to_string());

    ctx.say(format!(
        "channel: {channel_str}\nmin reactions: {}\nemoji: {}",
        cfg.min_stars, cfg.starboard_emoji
    ))
    .await?;
    Ok(())
}
