use poise::serenity_prelude as serenity;
use serde_json::{json, Value};

use crate::components::IS_COMPONENTS_V2;
use crate::Error;

const API: &str = "https://discord.com/api/v10";

// a file uploaded alongside a components v2 message
pub struct Upload {
    pub name: String,
    pub bytes: Vec<u8>,
}

// posts a components v2 message, returning its id.
// serenity 0.12 can't build these, so the payload goes out over the raw api
pub async fn send(
    client: &reqwest::Client,
    token: &str,
    channel: serenity::ChannelId,
    components: &[Value],
    files: Vec<Upload>,
) -> Result<u64, Error> {
    let url = format!("{API}/channels/{}/messages", channel.get());
    let request = client.post(url).header("Authorization", format!("Bot {token}"));

    let body: Value = check(carry(request, components, files).send().await?)
        .await?
        .json()
        .await?;
    body["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| Error::from("discord returned a message without an id"))
}

// rewrites an existing message, replacing its attachments with the given uploads. an edit
// declares the whole attachment list, so anything left out of files is dropped from the message
pub async fn edit(
    client: &reqwest::Client,
    token: &str,
    channel: serenity::ChannelId,
    message: u64,
    components: &[Value],
    files: Vec<Upload>,
) -> Result<(), Error> {
    let url = format!("{API}/channels/{}/messages/{message}", channel.get());
    let request = client.patch(url).header("Authorization", format!("Bot {token}"));

    check(carry(request, components, files).send().await?).await?;
    Ok(())
}

// attaches the components and uploads to a request, as multipart when there are files to send
fn carry(
    request: reqwest::RequestBuilder,
    components: &[Value],
    files: Vec<Upload>,
) -> reqwest::RequestBuilder {
    let attachments: Vec<Value> = files
        .iter()
        .enumerate()
        .map(|(i, file)| json!({ "id": i, "filename": file.name }))
        .collect();
    let payload = json!({
        "flags": IS_COMPONENTS_V2,
        "components": components,
        "attachments": attachments,
    });

    if files.is_empty() {
        return request.json(&payload);
    }
    let mut form = reqwest::multipart::Form::new().text("payload_json", payload.to_string());
    for (i, file) in files.into_iter().enumerate() {
        let part = reqwest::multipart::Part::bytes(file.bytes).file_name(file.name);
        form = form.part(format!("files[{i}]"), part);
    }
    request.multipart(form)
}

// turns a failed request into an error carrying discord's explanation
async fn check(resp: reqwest::Response) -> Result<reqwest::Response, Error> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    Err(Error::from(format!("discord returned {status}: {body}")))
}
