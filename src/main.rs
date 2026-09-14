mod api;
mod avatar;
mod commands;
mod components;
mod emoji;
mod handler;
mod starboard;
mod storage;

use std::sync::Arc;

use poise::serenity_prelude as serenity;
use tokio::sync::RwLock;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(Clone)]
pub struct Data {
    pub store: Arc<RwLock<storage::Store>>,
    pub http_client: reqwest::Client,
    pub bot_token: String,
    pub pending_updates: Arc<std::sync::Mutex<std::collections::HashMap<u64, tokio::task::JoinHandle<()>>>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    dotenvy::dotenv().ok();

    println!("starting astrum...");

    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN env var not set");

    let data = Data {
        store: Arc::new(RwLock::new(storage::Store::load("data.json").await)),
        http_client: reqwest::Client::new(),
        bot_token: token.clone(),
        pending_updates: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    };

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![commands::config()],
            event_handler: |ctx, event, _framework, data| {
                Box::pin(handler::handle_event(ctx, event, data))
            },
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                tracing::info!("logged in as {} (⌒▽⌒)☆", ready.user.name);
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                tracing::info!("slash commands registered globally");
                Ok(data)
            })
        })
        .build();

    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::GUILD_MESSAGE_REACTIONS
        | serenity::GatewayIntents::MESSAGE_CONTENT;

    serenity::ClientBuilder::new(&token, intents)
        .framework(framework)
        .await
        .expect("Failed to build client")
        .start()
        .await
        .expect("Client error");
}
