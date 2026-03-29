use crate::character::CharacterManager;
use crate::discord::DISCORD_CHANNEL;
use crate::instance::BusAction;
use crate::serverlist::{GameServerList, ServerId, StashLock};
use log::{debug, error, info, trace};
use serenity::all::Channel::Guild;
use serenity::all::{ChannelId, CreateMessage, GuildChannel, Http};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;
use crate::messages::{LoadStatus, PlayerConnectionId, SetOwnerStatus, StashStatus};
use anyhow::Result;
use crate::models::{Item, Skills};

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
            let res = match cmd {
                BusAction::Relay {
                    sender_id: id,
                    ref message,
                } => self.handle_relay(cmd.clone(), id, message).await,
                BusAction::Load { sender_id, name, password, connection_id } => self.handle_load(sender_id, name, password, connection_id).await,
                BusAction::Save { sender_id, name, connection_id, skills } => self.handle_save(sender_id, name, connection_id, skills).await,
                BusAction::SaveAndClose { sender_id, name, connection_id, skills } => self.handle_save_and_close(sender_id, name, connection_id, skills).await,
                BusAction::StashPage { sender_id, name, page, connection_id } => self.handle_stash_page(sender_id, name, page, connection_id).await,
                BusAction::StashTake { sender_id, name, page, index, connection_id } => self.handle_stash_take(sender_id, name, page, index, connection_id).await,
                BusAction::StashStore { sender_id, name,  item, connection_id } => self.handle_stash_put(sender_id, name, item, connection_id).await,
                BusAction::StashOpen { sender_id, name, connection_id } => self.handle_stash_open(sender_id, name, connection_id).await,
                BusAction::StashClose { sender_id, name, connection_id } => self.handle_stash_close(sender_id, name, connection_id).await,
                BusAction::StashCloseById { sender_id, name, connection_id } => self.handle_stash_close_by_id(sender_id, name, connection_id).await,
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
                BusAction::AuthorizeResult { .. } |
                BusAction::LoadResult { .. } |
                BusAction::StashPageResult { .. } |
                BusAction::StashTakeResult { .. } |
                BusAction::StashStoreResult { .. } |
                BusAction::SetOwnerResult { .. } |
                BusAction::StashOpenResult { .. } => {
                    /* this is not something _we_ must handle */
                    Ok(())
                }
            };

            if let Err(e) = res {
                error!("Error handling bus message: {:?}", e);
            }

            debug!("Finished processing command");
        }
    }

    async fn handle_auth_request(
        &mut self,
        id: u32,
        key: String,
        hostname: String,
        player_count: u32,
    ) -> Result<()> {
        let server = self.clients.get_server_copy_by_id(id);
        let result = self.clients.authorize(id, key);
        if let Some(server) = server {
            self.clients.set_hostname(id, hostname.clone());
            self.clients.set_player_count(id, player_count);

            server
                .server_channel
                .send(BusAction::AuthorizeResult { ok: result })
                .await?;

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

        Ok(())
    }

    async fn handle_server_offline(&mut self, id: u32) -> Result<()> {
        let server = self.clients.get_server_copy_by_id(id);
        if server.is_none() {
            return Ok(());
        }

        let server = server.unwrap();

        info!("Server {} ({}) offline", id, server.address);

        // Unlock all characters from this server
        self.clients.unlock_all_from_server(id);

        self.clients.remove(id);

        if !server.authorized {
            return Ok(());
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

        Ok(())
    }


    async fn handle_load(&mut self, sender_id: ServerId, name: String, password: String, connection_id: PlayerConnectionId) -> Result<()> {
        let server = self.clients.get_server_copy_by_id(sender_id);

        if self.clients.is_character_locked(&name, sender_id) {
            if let Some(server) = server {
                server.server_channel.send(BusAction::LoadResult {
                    status: LoadStatus::CharacterLocked,
                    connection_id,
                    skills: None,
                }).await?;
            }

            return Ok(());
        }

        let skills = self.character_manager.load_character(&name).await;

        if let Err(e) = skills {
            self.clients.unlock_character(&name);
            error!("Failed to load character {}: {:?}", name, e);

            if let Some(server) = server {
                server.server_channel.send(BusAction::LoadResult {
                        status: LoadStatus::InternalError,
                        connection_id,
                        skills: None,
                    }).await?;
            }

            return Err(e);
        }

        let mut status = None;
        let skills = skills.ok().flatten();

        let skills = if let Some(mut s) = skills {
            if s.password != password {
                status = Some(LoadStatus::WrongPassword);
                None
            } else {
                s.connection_id = connection_id;
                Some(s)
            }
        } else {
            status = Some(LoadStatus::CharacterNotFound);
            None
        };

        if let Some(server) = server {
            if skills.is_some() {
                self.clients.lock_character(name, sender_id);
            }

            server.server_channel.send(BusAction::LoadResult {
                status: status.unwrap_or(LoadStatus::Ok),
                connection_id,
                skills,
            }).await?;
        }

        Ok(())
    }

    async fn handle_save(&mut self, _sender_id: ServerId, name: String, connection_id: PlayerConnectionId, mut skills: Box<Skills>) -> Result<()> {
        skills.connection_id = connection_id;
        if let Err(e) = self.character_manager.save_character(&skills).await {
            error!("Failed to save character {}: {:?}", name, e);
            return Err(e);
        }
        Ok(())
    }

    async fn handle_save_and_close(&mut self, sender_id: ServerId, name: String, connection_id: PlayerConnectionId, skills: Box<Skills>) -> Result<()> {
        self.handle_save(sender_id, name.clone(), connection_id, skills).await?;
        self.clients.unlock_character(&name);
        Ok(())
    }

    async fn handle_stash_page(&mut self, sender_id: ServerId, name: String, page: i32, connection_id: PlayerConnectionId) -> Result<()> {
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());
        let items = self.character_manager.get_stash_page(&owner, page).await;

        if let Err(e) = items {
            error!("Failed to load stash page for {} (owner {}): {:?}", name, owner, e);
            return Err(e);
        }

        let items = items.unwrap_or_else(|_| vec![None; 64]);

        if let Some(server) = self.clients.get_server_copy_by_id(sender_id) {
            server.server_channel.send(BusAction::StashPageResult {
                page,
                items,
                connection_id,
            }).await?;
        }

        Ok(())
    }

    async fn handle_stash_take(&mut self, sender_id: ServerId, name: String, page: i32, index: i32, connection_id: PlayerConnectionId) -> Result<()> {
        let mut success = false;
        let mut item = None;
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());

        if self.clients.is_stash_exclusive_access(owner.clone(), StashLock::new(sender_id, name.clone())) {
            let res = self.character_manager.take_stash_item(&owner, page, index).await;
            match res {
                Ok(i) => {
                    item = i;
                    success = item.is_some();
                }
                Err(e) => {
                    error!("Failed to take item from stash for {} (owner {}): {:?}", name, owner, e);
                    self.clients.unlock_stash(&owner);
                    return Err(e);
                }
            }
        }

        if let Some(server) = self.clients.get_server_copy_by_id(sender_id) {
            server.server_channel.send(BusAction::StashTakeResult {
                page,
                index,
                success,
                item,
                connection_id,
            }).await?;
        }

        Ok(())
    }

    async fn handle_stash_put(&mut self, sender_id: ServerId, name: String, item: Item, connection_id: PlayerConnectionId) -> Result<()> {
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());

        if !self.clients.is_stash_exclusive_access(owner.clone(), StashLock::new(sender_id, name.clone())) {
            let pos = self.character_manager.find_free_stash_slot(&owner).await?;
            let res = self.character_manager.put_stash_item(&owner, pos, &item).await;
            return match res {
                Ok(s) => {
                    if let Some(server) = self.clients.get_server_copy_by_id(sender_id) {
                        server.server_channel.send(BusAction::StashStoreResult {
                            page: pos.page,
                            index: pos.slot,
                            success: true,
                            connection_id,
                        }).await?
                    }
                    Ok(())
                },
                Err(e) => {
                    error!("Failed to put item into stash for {} (owner {}): {:?}", name, owner, e);
                    if let Some(server) = self.clients.get_server_copy_by_id(sender_id) {
                        server.server_channel.send(BusAction::StashStoreResult {
                            page: pos.page,
                            index: pos.slot,
                            success: false,
                            connection_id,
                        }).await?;
                    }
                    Err(e)
                }
            }
        }

        anyhow::bail!("Stash is not owned by this server");
    }

    async fn handle_stash_open(&mut self, sender_id: ServerId, name: String, connection_id: PlayerConnectionId) -> Result<()> {
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());
        info!("Stash opened for {} (owner {})", &name, owner);

        if !self.clients.lock_stash(owner.clone(), StashLock::new(sender_id, name.clone())) {
            if let Some(server) = self.clients.get_server_copy_by_id(sender_id) {
                server.server_channel.send(BusAction::StashOpenResult {
                    items: None,
                    status: StashStatus::StashLocked,
                    connection_id,
                }).await?;
            }

            return Ok(());
        }

        // return items immediately
        let items = self.character_manager.get_stash_page(&owner, 0).await;
        if let Err(e) = items {
            error!("Failed to get initial stash page for {} (owner {}): {:?}", name, owner, e);
            return Err(e);
        }

        let items = items.unwrap_or_else(|_| vec![None; 64]);

        if let Some(server) = self.clients.get_server_copy_by_id(sender_id) {
            server.server_channel.send(BusAction::StashOpenResult {
                items: Some(items),
                status: StashStatus::Ok,
                connection_id,
            }).await?;
        }

        Ok(())
    }

    async fn handle_stash_close(&mut self, _sender_id: ServerId, name: String, _connection_id: PlayerConnectionId) -> Result<()> {
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());
        self.clients.unlock_stash(&owner);
        info!("Stash closed for {} (owner {})", name, owner);
        Ok(())
    }

    async fn handle_stash_close_by_id(&mut self, _sender_id: ServerId, name: String, _connection_id: PlayerConnectionId) -> Result<()> {
        let owner = self.character_manager.get_owner(&name).await.unwrap_or(name.clone());
        info!("Stash closed by ID for {} (owner {})",  name, owner);
        Ok(())
    }

    async fn handle_set_owner(&mut self, _sender_id: ServerId, name: String, password: String, reset: bool, owner: String, _connection_id: PlayerConnectionId) -> Result<()> {
        info!("SetOwner called for character {} setting owner {}, reset: {}", name, owner, reset);
        let result = self.character_manager.set_owner(&name, &password, reset, &owner).await;

        if let Err(e) = result {
            error!("Failed to set owner for character {}: {:?}", name, e);
            return Err(e);
        }

        if let Ok(status) = result {
            if let Some(server) = self.clients.get_server_copy_by_id(_sender_id) {
                let new_owner = if status == SetOwnerStatus::Ok { Some(owner) } else { None };
                server.server_channel.send(
                    BusAction::SetOwnerResult {
                        status,
                        connection_id: _connection_id,
                        new_owner
                    }
                ).await?;
            }
        }

        Ok(())
    }

    async fn handle_relay(&mut self, cmd: BusAction, id: u32, message: &String) -> Result<()> {
        let cl = self.clients.get_server_address(id);

        info!("Msg from server {} ({}): {}", id, cl, message);

        // send to all other vortex servers
        self.game_server_sendall(cmd.clone(), id, message).await;

        // if it was not sent by discord, send this message to it
        if id != DISCORD_CHANNEL {
            self.discord_send(message).await;
        }

        Ok(())
    }

    async fn game_server_sendall(&self, cmd: BusAction, sender_id: ServerId, message: &String) {
        let channels = self.clients.get_servers_snapshot();
        debug!("Broadcasting to all servers: {:?}", channels);

        for cl in channels.iter() {
            if cl.id == sender_id || !cl.authorized {
                continue;
            }

            let new_cmd = cmd.clone();
            trace!("Broadcasting to server {}: {:?}", cl.id, message);
            if let Err(e) = cl.server_channel.send(new_cmd).await {
                error!("Failed to broadcast message to server {}: {:?}", cl.id, e);
            }
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
