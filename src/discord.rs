use std::env;
use std::sync::Arc;
use log::info;
use serenity::all::{Context, EventHandler, GatewayIntents, Http, Message, Ready};
use serenity::{async_trait, Client};
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use crate::instance::BusAction;

pub struct VrxDiscordHandler {
    // our means of sending commands past our thread
    pub transmitter: Sender<BusAction>,

    // list of discord servers we accept messages from
    pub receive_channels: Vec<u64>,
}

pub const DISCORD_CHANNEL: u32 = 0x7FFFFFF;

#[async_trait]
impl EventHandler for VrxDiscordHandler {
    async fn message(&self, _ctx: Context, new_message: Message) {
        if new_message.author.bot {
            return;
        }

        if !self.receive_channels.contains(&new_message.channel_id.get()) {
            return;
        }

        let author = new_message.author.name.clone();
        let content = new_message.content.clone();
        let chan = new_message.channel_id.name(&_ctx).await.unwrap_or("unknown".to_string());
        self.transmitter.send(BusAction::Relay {
            sender_id: DISCORD_CHANNEL,
            message: format!("{author}#{chan}: {content}")
        }).await.ok();
    }

    async fn ready(&self, _ctx: Context, data_about_bot: Ready) {
        info!("Discord: Connected as {}", data_about_bot.user.name);
    }
}


pub async fn make_discord_bot(bus_tx: &Sender<BusAction>) -> Option<(JoinHandle<()>, Arc<Http>)> {
    let token = env::var("DISCORD_TOKEN").ok();

    if let Some(token) = token {
        let receive_channels_str = env::var("DISCORD_RECEIVE_CHANNELS")
            .unwrap_or("".to_string());
        let receive_channels: Vec<u64> = receive_channels_str
            .split(',')
            .filter_map(|x| {
                let trimmed = x.trim();
                if trimmed.is_empty() {
                    return None;
                }
                match trimmed.parse::<u64>() {
                    Ok(id) => Some(id),
                    Err(e) => {
                        log::warn!("Invalid Discord channel ID '{}': {}", trimmed, e);
                        None
                    }
                }
            })
            .collect();
        let client = Client::builder(&token,
                                     GatewayIntents::GUILD_MESSAGES |
                                         GatewayIntents::MESSAGE_CONTENT)
            .event_handler(VrxDiscordHandler {
                transmitter: bus_tx.clone(),
                receive_channels
            })
            .await
            .expect("Err creating client");

        let discord_http = client.http.clone();

        let task_discord = tokio::spawn(async move {
            let mut client = client;
            if let Err(why) = client.start().await {
                println!("Client error: {:?}", why);
            }
        });

        return Some((task_discord, discord_http))
    }

    None
}