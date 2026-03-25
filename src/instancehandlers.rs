use crate::models::{Item, Skills};
use anyhow::Error;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::mpsc::error::SendError;
use crate::instance::{BusAction, GameServerAction};
use crate::messages::{AuthorizeClientMessage, LoadStatus};
use crate::proto::ClientPackage;
use crate::serverlist::{GameServerList, VortexServer};

// handle message sent by the game server
#[derive(Debug, Clone)]
pub struct GameMessageReceivedHandler {
    id: u32,
    peers: GameServerList,
}

impl GameMessageReceivedHandler {
    pub fn server_state(&self) -> VortexServer {
        self.peers
            .get_server_copy_by_id(self.id)
            .unwrap()
    }

    pub fn new(id: u32, peers: GameServerList) -> Self {
        Self {
            id,
            peers,
        }
    }

    pub async fn send(&self, action: BusAction) -> Result<(), SendError<BusAction>> {
        self.peers.send(action).await
    }

    pub async fn dispatch(&self, command: Box<GameServerAction>) -> Result<(), Error> {
        if command.requires_authorization() && !self.server_state().authorized {
            return Ok(());
        }

        let cmd = command.clone();
        match *cmd {
            GameServerAction::Exit => {
                return Ok(());
            }
            GameServerAction::Relay { message } => self.handle_game_relay(message).await?,
            GameServerAction::ClientBegin { name } => self.handle_game_clientbegin(name).await?,
            GameServerAction::ClientDisconnect { name } => {
                self.handle_game_disconnect(name).await?
            }
            GameServerAction::Login { name } => self.handle_game_login(name).await?,
            GameServerAction::Load { name, password, connection_id, .. } => self.handle_game_load(name, password, connection_id).await?,
            GameServerAction::Save { name, connection_id, skills } => self.handle_game_save(name, connection_id, skills).await?,
            GameServerAction::SaveAndClose { name, connection_id, skills } => self.handle_game_save_and_close(name, connection_id, skills).await?,
            GameServerAction::StashPage { name, page, connection_id, .. } => self.handle_game_stash_page(name, page, connection_id).await?,
            GameServerAction::StashTake { name, page, index, connection_id, .. } => self.handle_game_stash_take(name, page, index, connection_id).await?,
            GameServerAction::StashStore { name, page, index, item, connection_id, .. } => self.handle_game_stash_put(name, page, index, item, connection_id).await?,
            GameServerAction::StashOpen { name, connection_id } => self.handle_game_stash_open(name, connection_id).await?,
            GameServerAction::StashOpenResult { .. } => {
                anyhow::bail!("StashOpenResult is a relay to server message");
            }
            GameServerAction::StashClose { name, connection_id } => self.handle_game_stash_close(name, connection_id).await?,
            GameServerAction::StashCloseById { name, id, connection_id, .. } => self.handle_game_stash_close_by_id(name, id, connection_id).await?,
            GameServerAction::SetOwner { name, password, reset, owner, connection_id, .. } => self.handle_game_set_owner(name, password, reset, owner, connection_id).await?,
            GameServerAction::Authorize { result } => self.handle_game_authorize(result).await?,
            GameServerAction::SpawnEntities { mapname } => {
                self.handle_game_spawnentities(mapname).await?
            }
            GameServerAction::LoadResult { .. } => {
                anyhow::bail!("LoadResult is a relay to server message");
            },
            GameServerAction::SetOwnerResult { .. } => {
                anyhow::bail!("SetOwnerResult is a relay to server message");
            },
        }

        Ok(())
    }

    async fn handle_game_spawnentities(&self, mapname: String) -> Result<(), Error> {
        self.peers.set_player_count(self.id, 0);

        self.send(BusAction::Relay {
                sender_id: self.id,
                message: format!("Map changed at {}: {}", self.server_state().hostname, mapname),
            })
            .await?;

        Ok(())
    }

    async fn handle_game_relay(&self, message: String) -> Result<(), Error> {
        // relay the message to all other transmitters
        self.send(BusAction::Relay {
                sender_id: self.id,
                message: message.trim().to_string(),
            })
            .await?;

        Ok(())
    }

    async fn handle_game_clientbegin(&self, name: String) -> Result<(), Error> {
        let count = self.peers.change_player_count(self.id, 1);

        self.send(BusAction::Relay {
                sender_id: self.id,
                message: format!(
                    "{} joined @ {} ({} player(s) online)",
                    name,
                    self.server_state().hostname,
                    count.unwrap()
                ),
            })
            .await?;

        Ok(())
    }

    async fn handle_game_disconnect(&self, name: String) -> Result<(), Error> {
        let count = self.peers.change_player_count(self.id, -1);

        self.send(BusAction::Relay {
                sender_id: self.id,
                message: format!(
                    "{} disconnected @ {} ({} player(s) online)",
                    name,
                    self.server_state().hostname,
                    count.unwrap()
                ),
            })
            .await?;

        Ok(())
    }

    async fn handle_game_login(&self, name: String) -> Result<(), Error> {
        if self.peers.lock_character(name.clone(), self.id) {
            self.send(BusAction::Load {
                sender_id: self.id,
                name,
                password: "".to_string(),
                connection_id: 0,
            }).await?;
        } else {
            // maybe notify that character is already logged in?
        }
        Ok(())
    }

    async fn handle_game_load(&self, name: String, password: String, connection_id: u64) -> Result<(), Error> {
        self.send(BusAction::Load {
            sender_id: self.id,
            name,
            password,
            connection_id,
        }).await?;
        Ok(())
    }

    async fn handle_game_save(&self, name: String, connection_id: u64, skills: Box<Skills>) -> Result<(), Error> {
        self.send(BusAction::Save {
            sender_id: self.id,
            name,
            connection_id,
            skills,
        }).await?;
        Ok(())
    }

    async fn handle_game_save_and_close(&self, name: String, connection_id: u64, skills: Box<Skills>) -> Result<(), Error> {
        self.send(BusAction::SaveAndClose {
            sender_id: self.id,
            name,
            connection_id,
            skills,
        }).await?;
        Ok(())
    }

    async fn handle_game_stash_page(&self, name: String, page: i32, connection_id: u64) -> Result<(), Error> {
        self.send(BusAction::StashPage {
            sender_id: self.id,
            name,
            page,
            connection_id,
        }).await?;
        Ok(())
    }

    async fn handle_game_stash_take(&self, name: String, page: i32, index: i32, connection_id: u64) -> Result<(), Error> {
        self.send(BusAction::StashTake {
            sender_id: self.id,
            name,
            page,
            index,
            connection_id,
        }).await?;
        Ok(())
    }

    async fn handle_game_stash_put(&self, name: String, page: i32, index: i32, item: Item, connection_id: u64) -> Result<(), Error> {
        self.send(BusAction::StashStore {
            sender_id: self.id,
            name,
            page,
            index,
            item,
            connection_id,
        }).await?;
        Ok(())
    }

    async fn handle_game_stash_open(&self, name: String, connection_id: u64) -> Result<(), Error> {
        self.send(BusAction::StashOpen {
            sender_id: self.id,
            name,
            connection_id,
        }).await?;
        Ok(())
    }

    async fn handle_game_stash_close(&self, name: String, connection_id: u64) -> Result<(), Error> {
        self.send(BusAction::StashClose {
            sender_id: self.id,
            name,
            connection_id,
        }).await?;
        Ok(())
    }

    async fn handle_game_stash_close_by_id(&self, name: String, id: i32, connection_id: u64) -> Result<(), Error> {
        self.send(BusAction::StashCloseById {
            sender_id: self.id,
            name,
            id,
            connection_id,
        }).await?;
        Ok(())
    }

    async fn handle_game_set_owner(&self, name: String, password: String, reset: bool, owner: String, connection_id: u64) -> Result<(), Error> {
        self.send(BusAction::SetOwner {
            sender_id: self.id,
            name,
            password,
            reset,
            owner,
            connection_id,
        }).await?;
        Ok(())
    }

    async fn handle_game_authorize(&self, result: AuthorizeClientMessage) -> Result<(), Error> {
        if self.server_state().authorized {
            return Ok(());
        }

        match result {
            AuthorizeClientMessage::Request {
                key,
                hostname,
                player_count,
            } => {
                self.peers
                    .send(BusAction::AuthorizeRequest {
                        sender_id: self.id,
                        key,
                        hostname,
                        player_count,
                    })
                    .await?
            }
            _ => {}
        }

        Ok(())
    }
}

// handle a message received from our bus
pub struct GameServerBusCommandHandler<'a> {
    pub sender: &'a Sender<Vec<u8>>,
    pub receiver: &'a mut Receiver<BusAction>,
}

impl GameServerBusCommandHandler<'_> {
    pub(crate) fn new<'a>(p0: &'a Sender<Vec<u8>>, p1: &'a mut Receiver<BusAction>) -> GameServerBusCommandHandler<'a> {
        GameServerBusCommandHandler {
            sender: p0,
            receiver: p1,
        }
    }

    async fn send_command(&self, command: Box<GameServerAction>) -> Result<(), SendError<Vec<u8>>> {
        self.sender.send(ClientPackage::from(command).into()).await
    }

    pub async fn dispatch(&mut self) -> Result<(), anyhow::Error> {
        let command = self.receiver.recv().await;
        if command.is_none() {
            anyhow::bail!("Command channel closed; no more commands to process")
        }

        let command = command.unwrap();
        match command {
            BusAction::Relay { message, .. } => self.handle_bus_relay(message).await?,
            BusAction::AuthorizeResult { ok } => self.handle_bus_authorize(ok).await?,
            BusAction::LoadResult { status, connection_id, skills } => self.handle_bus_load(status, connection_id, skills).await?,
            BusAction::StashPageResult { name, page, items, connection_id } => self.handle_bus_stash_page(name, page, items, connection_id).await?,
            BusAction::StashTakeResult { name, page, index, success, item, connection_id } => self.handle_bus_stash_take(name, page, index, success, item, connection_id).await?,
            BusAction::StashStoreResult { name, page, index, success, connection_id } => self.handle_bus_stash_put(name, page, index, success, connection_id).await?,
            BusAction::StashOpenResult { name, items, connection_id } => self.handle_bus_stash_open_result(name, items, connection_id).await?,
            _ => {}
        }

        Ok(())
    }

    async fn handle_bus_authorize(&mut self, ok: bool) -> Result<(), SendError<Vec<u8>>>{
        if ok {
            self.send_command(Box::from(GameServerAction::Authorize {
                result: AuthorizeClientMessage::Authorized,
            }))
                .await
        } else {
            self.send_command(Box::from(GameServerAction::Authorize {
                result: AuthorizeClientMessage::Unauthorized,
            }))
                .await
        }
    }

    async fn handle_bus_relay(&mut self, message: String) -> Result<(), SendError<Vec<u8>>> {
        // relay the message to the client
        let message = message.chars().filter(|x| x.is_ascii()).collect::<String>();
        self.send_command(Box::from(GameServerAction::Relay { message })).await
    }

    async fn handle_bus_load(&mut self, status: LoadStatus, connection_id: u64, skills: Option<Box<Skills>>) -> Result<(), SendError<Vec<u8>>> {
        self.send_command(Box::from(GameServerAction::LoadResult {
            status,
            connection_id,
            skills,
        })).await
    }

    async fn handle_bus_stash_page(&mut self, name: String, page: i32, items: Vec<Option<Item>>, connection_id: u64) -> Result<(), SendError<Vec<u8>>> {
        self.send_command(Box::from(GameServerAction::StashPage { name, page, items, connection_id })).await
    }

    async fn handle_bus_stash_take(&mut self, name: String, page: i32, index: i32, success: bool, item: Option<Item>, connection_id: u64) -> Result<(), SendError<Vec<u8>>> {
        self.send_command(Box::from(GameServerAction::StashTake { name, page, index, success, item, connection_id })).await
    }

    async fn handle_bus_stash_put(&mut self, name: String, page: i32, index: i32, success: bool, connection_id: u64) -> Result<(), SendError<Vec<u8>>> {
        self.send_command(Box::from(GameServerAction::StashStore {
            name,
            page,
            index,
            success,
            connection_id,
            item: Item {
                item_type: 0,
                item_level: 0,
                quantity: 0,
                untradeable: 0,
                id: "".to_string(),
                name: "".to_string(),
                num_mods: 0,
                set_code: 0,
                class_num: 0,
                modifiers: [crate::models::IModifier { modifier_type: 0, index: 0, value: 0, set: 0 }; 6],
                is_unique: 0
            }
        })).await
    }

    async fn handle_bus_stash_open_result(&mut self, name: String, items: Vec<Option<Item>>, connection_id: u64) -> Result<(), SendError<Vec<u8>>> {
        self.send_command(Box::from(GameServerAction::StashOpenResult { name, connection_id, items })).await
    }
}
