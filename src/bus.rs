use std::sync::Arc;
use log::{debug, info, trace};
use serenity::all::{ChannelId, CreateMessage, GuildChannel, Http};
use serenity::all::Channel::Guild;
use tokio::sync::mpsc::Receiver;
use crate::discord::DISCORD_CHANNEL;
use crate::instance::BusAction;
use crate::serverlist::{GameServerList};

pub struct DiscordContext {
    pub(crate) http: Option<Arc<Http>>,
    pub(crate) send_channel: Option<u64>,
}

pub struct Bus {
    bus_rx: Receiver<BusAction>,
    clients: GameServerList,
    discord_chan: Option<GuildChannel>,
    discord_http: Option<Arc<Http>>,
}

impl Bus {
    pub async fn new(discord: DiscordContext, bus_rx: Receiver<BusAction>, clients: GameServerList) -> Self {
        let (channel, discord_http) = get_discord_chan(discord).await;
        Bus {
            bus_rx,
            discord_chan: channel,
            discord_http,
            clients,
        }   
    }

    pub async fn bus_message_pump(&mut self) {
        if self.discord_chan.is_none() {
            info!("Provided discord channel not found");
        }

        loop {
            let cmd = self.bus_rx.recv().await.unwrap();

            trace!("Received command: {:?}", cmd);
            match cmd {
                BusAction::Relay {
                    sender_id: id,
                    ref message,
                } => {
                    let cl = self.clients.get_server_address(id);

                    info!("Msg from server {} ({}): {}", id, cl, message);

                    // send to all other vortex servers
                    self.game_server_sendall(cmd.clone(), id, message).await;

                    // if it was not sent by discord, send this message to it
                    if id != DISCORD_CHANNEL {
                        self.discord_send(message).await;
                    }
                }
                BusAction::ServerOffline { sender_id: id } => {
                    let cl = self.clients.get_server_address(id);
                    info!("Server {} ({}) offline", id, cl);

                    self.clients.remove(id)
                }
                BusAction::AuthorizeRequest { sender_id: id, key, hostname, player_count } => {
                    let server = self.clients.get_server_copy_by_id(id);
                    let result = self.clients.authorize(id, key);
                    if let Some(server) = server {
                        self.clients.set_hostname(id, hostname);
                        self.clients.set_player_count(id, player_count);

                        server.server_channel
                            .send(BusAction::AuthorizeResult { ok: result })
                            .await
                            .ok();
                    }
                },
                BusAction::AuthorizeResult { .. } => { /* this is not something _we_ must handle */ }
            };

            debug!("Finished processing command");
        }
    }

    async fn game_server_sendall(&self, cmd: BusAction, sender_id: u32, message: &String) {
        let channels = self.clients.get_servers_snapshot();
        debug!("Broadcasting to all servers: {:?}", channels);

        for cl in channels.iter() {
            if cl.id == sender_id {
                continue;
            }

            let new_cmd = cmd.clone();
            trace!("Broadcasting to server {}: {:?}", cl.id, message);
            cl.server_channel.send(new_cmd).await.ok();
        }
    }

    async fn discord_send(&self, message: &String) {
        if let Some(channel) = self.discord_chan.as_ref() {
            let state = channel
                .send_message(
                    // unwrap safety: we always get this if we get a valid channel
                    // see get_discord_chan
                    &self.discord_http.as_ref().unwrap(),
                    CreateMessage::new().content(message),
                )
                .await;

            if let Err(e) = state {
                info!("Failed to send message to discord: {:?}", e);
            }
        }
    }
}





async fn get_discord_chan(discord: DiscordContext) -> (Option<GuildChannel>, Option<Arc<Http>>) {
    let mut channel = None;
    if let Some(http_client) = discord.http.as_ref() {
        if let Some(send_channel) = discord.send_channel {
            let ch = http_client.get_channel(ChannelId::from(send_channel)).await;
            if let Ok(Guild(g)) = ch {
                channel = Some(g);
            }
        }
    }

    (channel, discord.http.clone())
}
