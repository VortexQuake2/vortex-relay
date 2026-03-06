use anyhow::Error;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::mpsc::error::SendError;
use crate::instance::{BusAction, GameServerAction};
use crate::messages::AuthorizeClientMessage;
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

    pub async fn dispatch(&self, command: GameServerAction) -> Result<(), Error> {
        if command.requires_authorization() && !self.server_state().authorized {
            return Ok(());
        }

        match command.clone() {
            GameServerAction::Exit => {
                return Ok(());
            }
            GameServerAction::Relay { message } => self.handle_game_relay(message).await?,
            GameServerAction::ClientBegin { name } => self.handle_game_clientbegin(name).await?,
            GameServerAction::ClientDisconnect { name } => {
                self.handle_game_disconnect(name).await?
            }
            GameServerAction::Login { .. } => {}
            GameServerAction::Authorize { result } => self.handle_game_authorize(result).await?,
            GameServerAction::SpawnEntities { mapname } => {
                self.handle_game_spawnentities(mapname).await?
            }
        }

        anyhow::bail!("Unknown command received from game server: {:?}", command);
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

    async fn send_command(&self, command: GameServerAction) -> Result<(), SendError<Vec<u8>>> {
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
            _ => {}
        }

        Ok(())
    }

    async fn handle_bus_authorize(&mut self, ok: bool) -> Result<(), SendError<Vec<u8>>>{
        if ok {
            self.send_command(GameServerAction::Authorize {
                result: AuthorizeClientMessage::Authorized,
            })
                .await
        } else {
            self.send_command(GameServerAction::Authorize {
                result: AuthorizeClientMessage::Unauthorized,
            })
                .await
        }
    }

    async fn handle_bus_relay(&mut self, message: String) -> Result<(), SendError<Vec<u8>>> {
        // relay the message to the client
        let message = message.chars().filter(|x| x.is_ascii()).collect::<String>();
        self.send_command(GameServerAction::Relay { message }).await
    }
}
