use std::collections::HashSet;

use poise::serenity_prelude as serenity;

use crate::emoji;
use crate::{Data, Error};

pub async fn handle_event(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    data: &Data,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::ReactionAdd { add_reaction } => {
            if let Err(e) = on_reaction_add(ctx, add_reaction, data).await {
                tracing::error!("error handling reaction: {e:?}");
            }
        }
        serenity::FullEvent::InteractionCreate { interaction } => match interaction {
            serenity::Interaction::Component(c) if c.data.custom_id.starts_with("sb:") => {
                if let Err(e) = on_component(ctx, c, data).await {
                    tracing::error!("error handling component: {e:?}");
                }
            }
            serenity::Interaction::Modal(m) if m.data.custom_id == MODAL_ID => {
                if let Err(e) = on_modal(ctx, m, data).await {
                    tracing::error!("error handling modal: {e:?}");
                }
            }
            _ => {}
        },
        _ => {}
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
    let (starboard_channel_id, min_stars, emojis) = {
        let store = data.store.read().await;
        let cfg = store.guild(guild_id.get());
        match cfg.starboard_channel {
            Some(ch) => (
                serenity::ChannelId::new(ch),
                cfg.min_stars,
                cfg.starboard_emojis.clone(),
            ),
            None => return Ok(()),
        }
    };

    // only handle the configured emojis
    if emoji::find(&reaction.emoji, &emojis).is_none() {
        return Ok(());
    }

    // don't re-star messages already in the starboard channel
    if reaction.channel_id == starboard_channel_id {
        return Ok(());
    }

    let mut message = reaction
        .channel_id
        .message(&ctx.http, reaction.message_id)
        .await?;

    // messages fetched over the rest api don't carry their guild, and a jump link without one
    // points at nothing
    message.guild_id = Some(guild_id);
    if let Some(replied_to) = message.referenced_message.as_mut() {
        replied_to.guild_id = Some(guild_id);
    }

    let (counts, star_count) = tally(ctx, &message, &emojis).await?;

    if star_count < min_stars {
        return Ok(());
    }

    let summary = crate::starboard::star_summary(&counts, star_count);

    // under write lock: either schedule a count update (already starred) or mark for first post
    {
        let mut store = data.store.write().await;
        if store.is_starred(guild_id.get(), message.id.get()) {
            // already on the starboard, schedule a debounced count edit if we have the msg id
            if let Some(smid) = store.get_starboard_msg(guild_id.get(), message.id.get()) {
                schedule_count_update(
                    ctx.clone(),
                    data.clone(),
                    message.clone(),
                    starboard_channel_id,
                    smid,
                    summary,
                );
            }
            return Ok(());
        }
        store.mark_starred(guild_id.get(), message.id.get());
        store.save().await?;
    }

    // first post: store the returned starboard message id for future count updates
    let starboard_msg_id =
        crate::starboard::post(ctx, data, &message, starboard_channel_id, &summary).await?;
    {
        let mut store = data.store.write().await;
        store.set_starboard_msg(guild_id.get(), message.id.get(), starboard_msg_id);
        store.save().await?;
    }

    Ok(())
}

// counts the reactors of every configured emoji on a message, returning the per-emoji
// counts and the number of distinct people behind them (reacting twice only counts once)
async fn tally(
    ctx: &serenity::Context,
    message: &serenity::Message,
    emojis: &[String],
) -> Result<(Vec<(String, u64)>, u64), Error> {
    let mut people: HashSet<serenity::UserId> = HashSet::new();
    let mut counts = Vec::new();

    for reaction in &message.reactions {
        let configured = match emoji::find(&reaction.reaction_type, emojis) {
            Some(e) => e.clone(),
            None => continue,
        };

        let mut count = 0;
        let mut after: Option<serenity::UserId> = None;
        loop {
            let batch = message
                .reaction_users(&ctx.http, reaction.reaction_type.clone(), Some(100), after)
                .await?;
            after = batch.last().map(|u| u.id);
            let full_page = batch.len() == 100;
            for user in batch {
                if user.bot {
                    continue;
                }
                people.insert(user.id);
                count += 1;
            }
            if !full_page {
                break;
            }
        }

        if count > 0 {
            counts.push((configured, count));
        }
    }

    Ok((counts, people.len() as u64))
}

// schedules a debounced star count edit, cancelling any previous pending edit for this message
fn schedule_count_update(
    ctx: serenity::Context,
    data: Data,
    message: serenity::Message,
    starboard_channel: serenity::ChannelId,
    starboard_msg_id: u64,
    summary: String,
) {
    let pending = data.pending_updates.clone();
    let mut map = pending.lock().unwrap();
    if let Some(handle) = map.remove(&message.id.get()) {
        handle.abort();
    }
    let original_msg_id = message.id.get();
    let handle = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
        if let Err(e) = crate::starboard::update(
            &ctx,
            &data,
            &message,
            starboard_channel,
            starboard_msg_id,
            &summary,
        )
        .await
        {
            tracing::warn!("failed to update star count: {e}");
        }
    });
    map.insert(original_msg_id, handle);
}

pub const ADD_ID: &str = "sb:add";
pub const REMOVE_PREFIX: &str = "sb:rm:";
const MODAL_ID: &str = "sb:add:modal";
const INPUT_ID: &str = "sb:add:input";

// handles the add/remove buttons on the emoji config panel
async fn on_component(
    ctx: &serenity::Context,
    interaction: &serenity::ComponentInteraction,
    data: &Data,
) -> Result<(), Error> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Ok(()),
    };
    if !is_admin(interaction.member.as_ref()) {
        return deny(ctx, &interaction.token, interaction.id).await;
    }

    if interaction.data.custom_id == ADD_ID {
        let input = serenity::CreateInputText::new(serenity::InputTextStyle::Short, "emoji", INPUT_ID)
            .placeholder("paste an emoji, custom ones included")
            .max_length(100)
            .required(true);
        let modal = serenity::CreateModal::new(MODAL_ID, "add a starboard emoji")
            .components(vec![serenity::CreateActionRow::InputText(input)]);
        interaction
            .create_response(&ctx.http, serenity::CreateInteractionResponse::Modal(modal))
            .await?;
        return Ok(());
    }

    let target = match interaction.data.custom_id.strip_prefix(REMOVE_PREFIX) {
        Some(t) => t.to_string(),
        None => return Ok(()),
    };

    let emojis = {
        let mut store = data.store.write().await;
        let cfg = store.guild_mut(guild_id.get());
        // at least one emoji has to stay configured
        if cfg.starboard_emojis.len() > 1 {
            cfg.starboard_emojis.retain(|e| *e != target);
        }
        let emojis = cfg.starboard_emojis.clone();
        store.save().await?;
        emojis
    };

    let panel = crate::commands::emoji_panel(&emojis);
    crate::components::update(ctx, interaction.id, &interaction.token, panel).await
}

// handles the emoji submitted from the add modal
async fn on_modal(
    ctx: &serenity::Context,
    interaction: &serenity::ModalInteraction,
    data: &Data,
) -> Result<(), Error> {
    let guild_id = match interaction.guild_id {
        Some(id) => id,
        None => return Ok(()),
    };
    if !is_admin(interaction.member.as_ref()) {
        return deny(ctx, &interaction.token, interaction.id).await;
    }

    let submitted = interaction
        .data
        .components
        .iter()
        .flat_map(|row| &row.components)
        .find_map(|c| match c {
            serenity::ActionRowComponent::InputText(input) if input.custom_id == INPUT_ID => {
                input.value.clone()
            }
            _ => None,
        })
        .unwrap_or_default();

    let normalized = match emoji::normalize(&submitted) {
        Some(e) => e,
        None => return ephemeral(ctx, &interaction.token, interaction.id, "that doesn't look like an emoji").await,
    };

    let emojis = {
        let mut store = data.store.write().await;
        let cfg = store.guild_mut(guild_id.get());
        if cfg.starboard_emojis.contains(&normalized) {
            return ephemeral(ctx, &interaction.token, interaction.id, &format!("{normalized} is already set")).await;
        }
        if cfg.starboard_emojis.len() >= emoji::MAX_EMOJIS {
            return ephemeral(
                ctx,
                &interaction.token,
                interaction.id,
                &format!("you can only have {} emojis", emoji::MAX_EMOJIS),
            )
            .await;
        }
        cfg.starboard_emojis.push(normalized);
        let emojis = cfg.starboard_emojis.clone();
        store.save().await?;
        emojis
    };

    let panel = crate::commands::emoji_panel(&emojis);
    crate::components::update(ctx, interaction.id, &interaction.token, panel).await
}

fn is_admin(member: Option<&serenity::Member>) -> bool {
    member.is_some_and(|m| m.permissions.is_some_and(|p| p.administrator()))
}

async fn deny(
    ctx: &serenity::Context,
    token: &str,
    id: serenity::InteractionId,
) -> Result<(), Error> {
    ephemeral(ctx, token, id, "you need administrator to change this").await
}

async fn ephemeral(
    ctx: &serenity::Context,
    token: &str,
    id: serenity::InteractionId,
    content: &str,
) -> Result<(), Error> {
    crate::components::notice(ctx, id, token, content).await
}
