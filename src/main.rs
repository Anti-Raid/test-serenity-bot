use log::info;
use serenity::all::{Framework, GuildId, HttpBuilder};
use serenity::async_trait;
use serenity::prelude::*;
use std::io::Write;
use std::sync::Arc;

struct Handler;

#[async_trait]
impl Framework for Handler {
    async fn init(&mut self, _client: &serenity::all::Client) {}

    async fn dispatch(&self, ctx: &Context, event: &serenity::all::FullEvent) {
        if let serenity::all::FullEvent::Ready { .. } = event {
            println!(
                "{} is ready on {} servers",
                ctx.cache.current_user().name,
                ctx.cache.guilds().len()
            );

            if ctx.shard_id.get() == 0 {
                let ctx = ctx.clone();
                tokio::task::spawn(async move {
                    loop {
                        println!("Server count: {}", ctx.cache.guilds().len());
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                });
            }
        } else if let serenity::all::FullEvent::Message { new_message, .. } = event {
            println!(
                "Message from {}: {}",
                new_message.author.name, new_message.content
            );
        }
    }
}

#[tokio::main]
async fn main() {
    let config_file = std::fs::File::open("config.json").expect("Failed to open config file");

    #[derive(serde::Deserialize)]
    struct Config {
        pub token: String,
        pub proxy_url: String,
        pub guild_ids: Vec<GuildId>,
    }

    let config: Config = serde_json::from_reader(config_file).expect("Failed to parse config file");

    let mut env_builder = env_logger::builder();

    env_builder.format(move |buf, record| {
        writeln!(
            buf,
            "({}) {} - {}",
            record.target(),
            record.level(),
            record.args()
        )
    });

    env_builder.init();

    info!("Proxy URL: {}", config.proxy_url);

    let token: serenity::all::Token = config.token.parse().expect("Invalid token");
    let http = Arc::new(
        HttpBuilder::new(token.clone())
            .proxy(config.proxy_url.clone())
            .ratelimiter_disabled(true)
            .build(),
    );

    info!("HttpBuilder done");

    let mut intents = serenity::all::GatewayIntents::all();

    // Remove the really spammy intents
    intents.remove(serenity::all::GatewayIntents::GUILD_PRESENCES); // Don't even have the privileged gateway intent for this
    intents.remove(serenity::all::GatewayIntents::GUILD_MESSAGE_TYPING); // Don't care about typing
    intents.remove(serenity::all::GatewayIntents::DIRECT_MESSAGE_TYPING); // Don't care about typing
    intents.remove(serenity::all::GatewayIntents::DIRECT_MESSAGES); // Don't care about DMs

    let mut client = serenity::all::ClientBuilder::new_with_http(token.clone(), http, intents)
        .framework(Handler)
        .await
        .expect("Error creating client");

    if !config.guild_ids.is_empty() {
        // Fetch /api/gateway/bot?guild_ids=... to get the shard count
        let req = reqwest::Client::new()
            .get(format!(
                "{}/api/gateway/bot?guild_ids={}",
                config.proxy_url,
                config
                    .guild_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ))
            .send()
            .await
            .expect("Failed to fetch gateway info")
            .json::<serenity::all::BotGateway>()
            .await
            .expect("Failed to parse gateway info");

        println!("WS URL: {}", req.url);
        client.ws_url = Arc::from(req.url.to_string());
        client.shard_manager.ws_url = Arc::from(req.url.to_string());

        client
            .start_shards(req.shards.into())
            .await
            .expect("Failed to start shards");
    } else {
        // Finally, start a shard, and start listening to events.
        //
        // Shards will automatically attempt to reconnect, and will perform exponential backoff until
        // it reconnects.
        if let Err(why) = client.start_autosharded().await {
            println!("Client error: {why:?}");
        }
    }
}
