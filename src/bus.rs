use crate::character::CharacterManager;
use crate::discord::DISCORD_CHANNEL;
use crate::instance::BusAction;
use crate::serverlist::GameServerList;
use log::{debug, error, info, trace};
use serenity::all::Channel::Guild;
use serenity::all::{ChannelId, CreateMessage, GuildChannel, Http};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

pub struct DiscordContext {
    pub(crate) http: Option<Arc<Http>>,
    pub(crate) send_channel: Option<u64>,
}

pub struct Bus {
    bus_rx: Receiver<BusAction>,
    clients: GameServerList,
    discord_chan: Option<GuildChannel>,
    discord_http: Option<Arc<Http>>,
    character_manager: CharacterManager,
}

impl Bus {
    pub async fn new(
        discord: DiscordContext,
        bus_rx: Receiver<BusAction>,
        clients: GameServerList,
        character_manager: CharacterManager,
    ) -> Self {
        let (channel, discord_http) = get_discord_chan(discord).await;
        Bus {
            bus_rx,
            discord_chan: channel,
            discord_http,
            clients,
            character_manager,
        }
    }

    pub async fn bus_message_pump(&mut self) {
        if self.discord_chan.is_none() {
            info!("Provided discord channel not found");
        }

        loop {
            let cmd = self.bus_rx.recv().await;

            if cmd.is_none() {
                info!("Bus message pump was none?");

                if self.bus_rx.is_closed() {
                    info!("Bus message pump was closed");
                    return;
                }

                continue;
            }

            let cmd = cmd.unwrap();

            trace!("Received command: {:?}", cmd);
            match cmd {
                BusAction::Relay {
                    sender_id: id,
                    ref message,
                } => self.handle_relay(cmd.clone(), id, message).await,
                BusAction::Load { sender_id, name, password, connection_id } => self.handle_load(sender_id, name, password, connection_id).await,
                BusAction::Save { sender_id, name, connection_id, skills } => self.handle_save(sender_id, name, connection_id, skills).await,
                BusAction::SaveAndClose { sender_id, name, connection_id, skills } => self.handle_save_and_close(sender_id, name, connection_id, skills).await,
                BusAction::StashPage { sender_id, name, page, connection_id } => self.handle_stash_page(sender_id, name, page, connection_id).await,
                BusAction::StashTake { sender_id, name, page, index, connection_id } => self.handle_stash_take(sender_id, name, page, index, connection_id).await,
                BusAction::StashStore { sender_id, name, page, index, item, connection_id } => self.handle_stash_put(sender_id, name, page, index, item, connection_id).await,
                BusAction::StashOpen { sender_id, name, connection_id } => self.handle_stash_open(sender_id, name, connection_id).await,
                BusAction::StashClose { sender_id, name, connection_id } => self.handle_stash_close(sender_id, name, connection_id).await,
                BusAction::StashCloseById { sender_id, name, id, connection_id } => self.handle_stash_close_by_id(sender_id, name, id, connection_id).await,
                BusAction::SetOwner { sender_id, name, password, reset, owner, connection_id } => self.handle_set_owner(sender_id, name, password, reset, owner, connection_id).await,
                BusAction::ServerOffline { sender_id: id } => self.handle_server_offline(id).await,
                BusAction::AuthorizeRequest {
                    sender_id: id,
                    key,
                    hostname,
                    player_count,
                } => {
                    self.handle_auth_request(id, key, hostname, player_count)
                        .await
                }
                BusAction::AuthorizeResult { .. } | BusAction::LoadResult { .. } | BusAction::StashPageResult { .. } | BusAction::StashTakeResult { .. } | BusAction::StashStoreResult { .. } | BusAction::StashOpenResult { .. } => {
                    /* this is not something _we_ must handle */
                }
            };

            debug!("Finished processing command");
        }
    }

    async fn handle_auth_request(
        &mut self,
        id: u32,
        key: String,
        hostname: String,
        player_count: u32,
    ) {
        let server = self.clients.get_server_copy_by_id(id);
        let result = self.clients.authorize(id, key);
        if let Some(server) = server {
            self.clients.set_hostname(id, hostname.clone());
            self.clients.set_player_count(id, player_count);

            server
                .server_channel
                .send(BusAction::AuthorizeResult { ok: result })
                .await
                .ok();

            let message = format!("Server \"{}\" ({}) online", hostname, server.address);

            self.game_server_sendall(
                BusAction::Relay {
                    sender_id: id,
                    message: message.clone(),
                },
                id,
                &message,
            )
            .await;

            self.discord_send(&message).await;
        }
    }

    async fn handle_server_offline(&mut self, id: u32) {
        let server = self.clients.get_server_copy_by_id(id);
        if server.is_none() {
            return;
        }

        let server = server.unwrap();

        info!("Server {} ({}) offline", id, server.address);

        // Unlock all characters from this server
        self.clients.unlock_all_from_server(id);

        self.clients.remove(id);

        if !server.authorized {
            return;
        }

        let message = format!(
            "Server \"{}\" ({}) offline",
            server.hostname, server.address
        );

        self.game_server_sendall(
            BusAction::Relay {
                sender_id: id,
                message: message.clone(),
            },
            id,
            &message,
        )
        .await;

        self.discord_send(&message).await;
    }

    async fn handle_load(&mut self, sender_id: u32, name: String, password: String, connection_id: i32) {
        let skills = self.character_manager.load_character(&name).await.unwrap_or_else(|e| {
            error!("Failed to load character {}: {:?}", name, e);
            None
        });

        let skills = if let Some(mut s) = skills {
            if s.password != password {
                debug!("Character {} load failed: password mismatch", name);
                None
            } else {
                s.connection_id = connection_id;
                Some(s)
            }
        } else {
            None
        };

        if let Some(server) = self.clients.get_server_copy_by_id(sender_id) {
            server.server_channel.send(BusAction::LoadResult {
                name,
                connection_id,
                skills,
            }).await.ok();
        }
    }

    async fn handle_save(&mut self, _sender_id: u32, name: String, connection_id: i32, mut skills: crate::models::Skills) {
        skills.connection_id = connection_id;
        if let Err(e) = self.character_manager.save_character(&skills).await {
            error!("Failed to save character {}: {:?}", name, e);
        }
    }

    async fn handle_save_and_close(&mut self, sender_id: u32, name: String, connection_id: i32, skills: crate::models::Skills) {
        self.handle_save(sender_id, name.clone(), connection_id, skills).await;
        self.clients.unlock_character(&name);
    }

    async fn handle_stash_page(&mut self, sender_id: u32, name: String, page: i32, connection_id: i32) {
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());
        let items = self.character_manager.get_stash_page(&owner, page).await.unwrap_or_else(|e| {
            error!("Failed to load stash page for {} (owner {}): {:?}", name, owner, e);
            vec![None; 64]
        });

        if let Some(server) = self.clients.get_server_copy_by_id(sender_id) {
            server.server_channel.send(BusAction::StashPageResult {
                name,
                page,
                items,
                connection_id,
            }).await.ok();
        }
    }

    async fn handle_stash_take(&mut self, sender_id: u32, name: String, page: i32, index: i32, connection_id: i32) {
        let mut success = false;
        let mut item = None;
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());

        if self.clients.lock_stash(owner.clone(), sender_id) {
            item = self.character_manager.take_stash_item(&owner, page, index).await.unwrap_or_else(|e| {
                error!("Failed to take item from stash for {} (owner {}): {:?}", name, owner, e);
                None
            });
            success = item.is_some();
            self.clients.unlock_stash(&owner);
        }

        if let Some(server) = self.clients.get_server_copy_by_id(sender_id) {
            server.server_channel.send(BusAction::StashTakeResult {
                name,
                page,
                index,
                success,
                item,
                connection_id,
            }).await.ok();
        }
    }

    async fn handle_stash_put(&mut self, sender_id: u32, name: String, page: i32, index: i32, item: crate::models::Item, connection_id: i32) {
        let mut success = false;
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());

        if self.clients.lock_stash(owner.clone(), sender_id) {
            success = self.character_manager.put_stash_item(&owner, page, index, &item).await.unwrap_or_else(|e| {
                error!("Failed to put item into stash for {} (owner {}): {:?}", name, owner, e);
                false
            });
            self.clients.unlock_stash(&owner);
        }

        if let Some(server) = self.clients.get_server_copy_by_id(sender_id) {
            server.server_channel.send(BusAction::StashStoreResult {
                name,
                page,
                index,
                success,
                connection_id,
            }).await.ok();
        }
    }

    async fn handle_stash_open(&mut self, sender_id: u32, name: String, connection_id: i32) {
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());
        info!("Stash opened for {} (owner {})", name, owner);

        // return items immediately
        let items = self.character_manager.get_stash_page(&owner, 0).await.unwrap_or_else(|e| {
            error!("Failed to get initial stash page for {} (owner {}): {:?}", name, owner, e);
            vec![None; 64]
        });

        if let Some(server) = self.clients.get_server_copy_by_id(sender_id) {
            server.server_channel.send(BusAction::StashOpenResult {
                name,
                items,
                connection_id,
            }).await.ok();
        }
    }

    async fn handle_stash_close(&mut self, _sender_id: u32, name: String, _connection_id: i32) {
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());
        info!("Stash closed for {} (owner {})", name, owner);
    }

    async fn handle_stash_close_by_id(&mut self, _sender_id: u32, name: String, id: i32, _connection_id: i32) {
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());
        info!("Stash closed by ID {} for {} (owner {})", id, name, owner);
    }

    async fn handle_set_owner(&mut self, _sender_id: u32, name: String, password: String, reset: bool, owner: String, _connection_id: i32) {
        info!("SetOwner called for character {} by owner {}, reset: {}", name, owner, reset);
        if let Err(e) = self.character_manager.set_owner(&name, &password, reset, &owner).await {
            error!("Failed to set owner for character {}: {:?}", name, e);
        }
    }

    async fn handle_relay(&mut self, cmd: BusAction, id: u32, message: &String) {
        let cl = self.clients.get_server_address(id);

        info!("Msg from server {} ({}): {}", id, cl, message);

        // send to all other vortex servers
        self.game_server_sendall(cmd.clone(), id, message).await;

        // if it was not sent by discord, send this message to it
        if id != DISCORD_CHANNEL {
            self.discord_send(message).await;
        }
    }

    async fn game_server_sendall(&self, cmd: BusAction, sender_id: u32, message: &String) {
        let channels = self.clients.get_servers_snapshot();
        debug!("Broadcasting to all servers: {:?}", channels);

        for cl in channels.iter() {
            if cl.id == sender_id || !cl.authorized {
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
