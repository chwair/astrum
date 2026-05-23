use base64::{engine::general_purpose::STANDARD, Engine as _};
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};

use crate::{Data, Error};

// discord upload api types

#[derive(Serialize)]
struct RequestUploadBody {
    files: Vec<RequestUploadFile>,
}

#[derive(Serialize)]
struct RequestUploadFile {
    filename: String,
    file_size: usize,
    id: String,
}

#[derive(Deserialize)]
struct UploadResponse {
    attachments: Vec<UploadedFile>,
}

#[derive(Deserialize)]
struct UploadedFile {
    upload_url: String,
    upload_filename: String,
}

#[derive(Serialize)]
struct SendVoiceBody {
    flags: u64,
    attachments: Vec<VoiceAttachment>,
}

#[derive(Serialize)]
struct VoiceAttachment {
    id: String,
    filename: String,
    uploaded_filename: String,
    duration_secs: f64,
    waveform: String,
}

// re-uploads and re-sends the voice message to the starboard channel
pub async fn relay(
    _ctx: &serenity::Context,
    data: &Data,
    message: &serenity::Message,
    starboard_channel: serenity::ChannelId,
) -> Result<(), Error> {
    let audio_att = match message
        .attachments
        .iter()
        .find(|a| a.content_type.as_deref().is_some_and(|ct| ct.starts_with("audio/")))
    {
        Some(a) => a,
        None => return Ok(()), // no audio attachment found
    };

    // download the original audio
    let audio_bytes = data
        .http_client
        .get(&audio_att.url)
        .send()
        .await?
        .bytes()
        .await?;

    let file_size = audio_bytes.len();
    let filename = "voice-message.ogg".to_string();

    // request an upload slot from discord
    let upload_body = RequestUploadBody {
        files: vec![RequestUploadFile {
            filename: filename.clone(),
            file_size,
            id: "1".to_string(),
        }],
    };

    let upload_resp: UploadResponse = data
        .http_client
        .post(format!(
            "https://discord.com/api/v10/channels/{}/attachments",
            starboard_channel.get()
        ))
        .header("Authorization", format!("Bot {}", data.bot_token))
        .json(&upload_body)
        .send()
        .await?
        .json()
        .await?;

    let slot = upload_resp
        .attachments
        .into_iter()
        .next()
        .ok_or("Discord returned no upload URL")?;

    // upload audio to the pre-signed url
    data.http_client
        .put(&slot.upload_url)
        .header("Content-Type", "audio/ogg")
        .body(audio_bytes)
        .send()
        .await?;

    // send as voice message (flag 8192 = IS_VOICE_MESSAGE)
    let duration_secs = audio_att.duration_secs.unwrap_or(1.0);
    let waveform_bytes = audio_att.waveform.clone().unwrap_or_else(default_waveform);
    let waveform = STANDARD.encode(&waveform_bytes);

    let send_body = SendVoiceBody {
        flags: 8192,
        attachments: vec![VoiceAttachment {
            id: "0".to_string(),
            filename,
            uploaded_filename: slot.upload_filename,
            duration_secs,
            waveform,
        }],
    };

    data.http_client
        .post(format!(
            "https://discord.com/api/v10/channels/{}/messages",
            starboard_channel.get()
        ))
        .header("Authorization", format!("Bot {}", data.bot_token))
        .json(&send_body)
        .send()
        .await?;

    Ok(())
}

// helpers

// flat default waveform for when the original is unavailable
fn default_waveform() -> Vec<u8> {
    vec![64u8; 64]
}
