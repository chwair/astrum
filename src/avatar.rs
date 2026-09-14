use base64::{engine::general_purpose::STANDARD, Engine as _};
use poise::serenity_prelude as serenity;
use serde::Deserialize;
use serde_json::json;

use crate::Data;

const API: &str = "https://discord.com/api/v10";
// an app can own 2000 emojis, so uploads stop short of the wall
const MAX_AVATARS: usize = 1900;
// discord rejects emoji images larger than this
const MAX_BYTES: usize = 256 * 1024;
// avatars are fetched and uploaded at emoji resolution
const SIZE: u32 = 128;

#[derive(Deserialize)]
struct CreatedEmoji {
    id: String,
}

// a user's avatar, cropped to a circle and uploaded as an inline emoji so it can sit to the
// left of their name the way discord draws its own message headers. components v2 has no author
// row, and app emojis are the only images that render inline in text; they belong to the app,
// so they work in every server. returns none when there's nothing to show or the upload failed
pub async fn inline(ctx: &serenity::Context, data: &Data, user: &serenity::User) -> Option<String> {
    let hash = user.avatar?.to_string();
    let app_id = ctx.http.application_id()?.get();

    // cached, as long as they haven't changed their avatar since
    {
        let mut store = data.store.write().await;
        if let Some(emoji) = store.avatar(user.id.get(), &hash) {
            return Some(emoji);
        }
    }

    // the static avatar is used even for animated ones: a gif only has on/off transparency,
    // so a circle cut out of one comes back with a staircase for an edge
    let url = format!(
        "https://cdn.discordapp.com/avatars/{}/{hash}.png?size={SIZE}",
        user.id.get()
    );
    let bytes = match data.http_client.get(url).send().await {
        Ok(resp) => resp.bytes().await.ok()?,
        Err(e) => {
            tracing::warn!("failed to fetch avatar for {}: {e}", user.id);
            return None;
        }
    };
    let image = circle(&bytes)?;
    if image.len() > MAX_BYTES {
        return None;
    }

    // free the slot this replaces, plus the least used one if the app is full
    let stale = {
        let mut store = data.store.write().await;
        let stale = store.take_stale_avatars(user.id.get(), MAX_AVATARS);
        store.save().await.ok();
        stale
    };
    for emoji_id in stale {
        delete(data, app_id, emoji_id).await;
    }

    let image = format!("data:image/png;base64,{}", STANDARD.encode(&image));
    let resp = data
        .http_client
        .post(format!("{API}/applications/{app_id}/emojis"))
        .header("Authorization", format!("Bot {}", data.bot_token))
        .json(&json!({ "name": format!("av{}", user.id.get()), "image": image }))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        tracing::warn!(
            "failed to upload avatar emoji: {} {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
        return None;
    }

    let created: CreatedEmoji = resp.json().await.ok()?;
    let emoji_id: u64 = created.id.parse().ok()?;
    let mut store = data.store.write().await;
    let emoji = store.set_avatar(user.id.get(), hash, emoji_id);
    store.save().await.ok();
    Some(emoji)
}

// masks the square avatar into a circle, softening the last pixel of the edge so it doesn't
// come out jagged at emoji size
fn circle(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut avatar = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = avatar.dimensions();
    let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
    let radius = cx.min(cy);

    for (x, y, pixel) in avatar.enumerate_pixels_mut() {
        let dx = x as f32 + 0.5 - cx;
        let dy = y as f32 + 0.5 - cy;
        let edge = (radius - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
        pixel.0[3] = (pixel.0[3] as f32 * edge) as u8;
    }

    let mut png = Vec::new();
    avatar
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

// drops an emoji the app no longer needs
async fn delete(data: &Data, app_id: u64, emoji_id: u64) {
    let resp = data
        .http_client
        .delete(format!("{API}/applications/{app_id}/emojis/{emoji_id}"))
        .header("Authorization", format!("Bot {}", data.bot_token))
        .send()
        .await;
    if let Err(e) = resp {
        tracing::warn!("failed to delete avatar emoji {emoji_id}: {e}");
    }
}
