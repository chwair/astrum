use poise::serenity_prelude as serenity;
use serde_json::Value;

use crate::components as ui;
use crate::{Context, Error};

/// configure the starboard for this server
#[poise::command(slash_command, guild_only, subcommands("channel", "stars", "emoji", "weight", "show"))]
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
    ui::reply(ctx, panel(format!("starboard channel set to <#{}>", channel.id))).await
}

/// set the minimum number of reactions to reach the starboard
#[poise::command(slash_command, guild_only, ephemeral, required_permissions = "ADMINISTRATOR")]
pub async fn stars(
    ctx: Context<'_>,
    #[description = "minimum reactions (must be at least 1)"] count: u64,
) -> Result<(), Error> {
    if count == 0 {
        return ui::reply(ctx, panel("minimum reactions must be at least 1")).await;
    }
    let guild_id = ctx.guild_id().expect("guild_only").get();
    {
        let mut store = ctx.data().store.write().await;
        store.guild_mut(guild_id).min_stars = count;
        store.save().await?;
    }
    ui::reply(ctx, panel(format!("minimum reactions set to {count}"))).await
}

/// manage the emojis that count toward the starboard
#[poise::command(slash_command, guild_only, ephemeral, required_permissions = "ADMINISTRATOR")]
pub async fn emoji(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only").get();
    let cfg = {
        let store = ctx.data().store.read().await;
        store.guild(guild_id)
    };

    ui::reply(ctx, emoji_panel(&cfg)).await
}

/// set what one reaction of an emoji counts for
#[poise::command(slash_command, guild_only, ephemeral, required_permissions = "ADMINISTRATOR")]
pub async fn weight(
    ctx: Context<'_>,
    #[description = "the configured emoji"] emoji: String,
    #[description = "what one reaction counts for, e.g. 2, 0.5 or 1/3"] weight: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only").get();
    let normalized = match crate::emoji::normalize(&emoji) {
        Some(e) => e,
        None => return ui::reply(ctx, panel("that doesn't look like an emoji")).await,
    };
    let weight = match crate::emoji::parse_weight(&weight) {
        Some(w) => w,
        None => {
            return ui::reply(
                ctx,
                panel(format!(
                    "weight has to be a number above 0 and at most {}, like 2, 0.5 or 1/3",
                    crate::emoji::format_weight(crate::emoji::MAX_WEIGHT)
                )),
            )
            .await
        }
    };

    {
        let mut store = ctx.data().store.write().await;
        let cfg = store.guild_mut(guild_id);
        if !cfg.starboard_emojis.contains(&normalized) {
            return ui::reply(
                ctx,
                panel(format!("{normalized} isn't a starboard emoji. add it with /config emoji")),
            )
            .await;
        }
        cfg.set_weight(&normalized, weight);
        store.save().await?;
    }

    ui::reply(
        ctx,
        panel(format!(
            "{normalized} now counts for {} per reaction",
            crate::emoji::format_weight(weight)
        )),
    )
    .await
}

/// show the current starboard configuration
#[poise::command(slash_command, guild_only, ephemeral)]
pub async fn show(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().expect("guild_only").get();
    let cfg = {
        let store = ctx.data().store.read().await;
        store.guild(guild_id)
    };

    let channel = cfg
        .starboard_channel
        .map(|id| format!("<#{id}>"))
        .unwrap_or_else(|| "*not set*".to_string());

    ui::reply(
        ctx,
        vec![ui::container(vec![
            ui::text("**starboard configuration**"),
            ui::separator(),
            ui::text(format!(
                "channel: {channel}\nmin reactions: {}\nemojis: {}",
                cfg.min_stars,
                weighted_list(&cfg)
            )),
        ])],
    )
    .await
}

// the configured emojis, each with its weight when it isn't worth a plain reaction
fn weighted_list(cfg: &crate::storage::GuildConfig) -> String {
    cfg.starboard_emojis
        .iter()
        .map(|e| match cfg.weight(e) == crate::emoji::DEFAULT_WEIGHT {
            true => e.clone(),
            false => format!("{e} ×{}", crate::emoji::format_weight(cfg.weight(e))),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// a plain one-line config response
fn panel(content: impl Into<String>) -> Vec<Value> {
    vec![ui::container(vec![ui::text(content)])]
}

// builds the emoji config panel: a remove button per emoji plus an add button
pub fn emoji_panel(cfg: &crate::storage::GuildConfig) -> Vec<Value> {
    let emojis = &cfg.starboard_emojis;
    let header = format!(
        "**starboard emojis** ({}/{})\n{}\n-# click an emoji to remove it. a message reaches the \
         starboard once these reactions add up to the minimum, each counting for its weight. \
         set one with /config weight.",
        emojis.len(),
        crate::emoji::MAX_EMOJIS,
        weighted_list(cfg),
    );

    let mut items = vec![ui::text(header), ui::separator()];

    // the last emoji can't be removed, otherwise nothing could star a message
    let locked = emojis.len() <= 1;
    for chunk in emojis.chunks(5) {
        let buttons = chunk
            .iter()
            .map(|e| {
                let id = format!("{}{e}", crate::handler::REMOVE_PREFIX);
                match crate::emoji::reaction_type(e) {
                    Some(rt) => ui::button(&id, ui::DANGER, None, Some(&rt), locked),
                    None => ui::button(&id, ui::DANGER, Some(e), None, locked),
                }
            })
            .collect();
        items.push(ui::action_row(buttons));
    }

    items.push(ui::action_row(vec![ui::button(
        crate::handler::ADD_ID,
        ui::PRIMARY,
        Some("add emoji"),
        None,
        emojis.len() >= crate::emoji::MAX_EMOJIS,
    )]));

    vec![ui::container(items)]
}
