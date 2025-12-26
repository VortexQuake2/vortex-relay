pub(crate) use crate::messages::{BusAction, GameServerAction};
use crate::proto::{game_socket_send_receive_all, GameServerSender};
use crate::serverlist::{GameServerList};
use log::{error};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender};
use crate::messages::AuthorizeClientMessage;

pub struct GameServer {
    id: u32,
    bus_tx: Sender<BusAction>,
    pub bus_receiver: Option<Receiver<BusAction>>,
    pub game_socket: Option<TcpStream>,
    pub peers: GameServerList,
}

impl GameServer {
    pub fn new(
        id: u32,
        socket: TcpStream,
        receiver: Receiver<BusAction>,
        peers: GameServerList,
    ) -> Self {
        let bus_tx = peers.bus().bus_tx().clone();
        Self {
            id,
            bus_tx,
            game_socket: Some(socket),
            bus_receiver: Some(receiver),
            peers,
        }
    }

    pub(crate) async fn main_loop(mut self) {
        let (game_socket_data_queue_sender, game_socket_data_queue_receiver) =
            tokio::sync::mpsc::channel::<Vec<u8>>(100);
        let alive = Arc::new(AtomicBool::new(true));

        let game_socket = self.game_socket.take().unwrap();
        let id = self.id;
        let alive_socket = alive.clone();
        let game_socket_communication_task = tokio::spawn(async move {
            let mut game_socket_data_queue_receiver = game_socket_data_queue_receiver;
            let mut game_socket = game_socket;
            let peers = self.peers.clone();

            game_socket.set_nodelay(true).unwrap();
            loop {
                if !game_socket_send_receive_all(
                    &mut game_socket_data_queue_receiver,
                    &mut game_socket,
                    &peers,
                    id,
                )
                .await
                .expect("Failed to process game instance socket")
                {
                    break;
                }
            }

            self.bus_tx
                .send(BusAction::ServerOffline { sender_id: id })
                .await
                .unwrap();
            alive_socket.store(false, std::sync::atomic::Ordering::Relaxed);
            game_socket_data_queue_receiver.close();
        });

        let bus_receiver = self.bus_receiver.take().unwrap();
        let bus_communication_task = tokio::spawn(async move {
            let mut bus_receiver = bus_receiver;
            let game_sender = GameServerSender(&game_socket_data_queue_sender);
            let alive_bus_communication = alive;
            loop {
                if !handle_bus_message(&mut bus_receiver, &game_sender).await {
                    break;
                }

                if !alive_bus_communication.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
            }

            alive_bus_communication.store(false, std::sync::atomic::Ordering::Relaxed);
        });

        let (r1, r2) = tokio::join!(game_socket_communication_task, bus_communication_task);
        if let Err(e) = r1 {
            error!("Socket process failed: {:?}", e);
        }

        if let Err(e) = r2 {
            error!("Server communication process failed: {:?}", e);
        }
    }
}

// handle message sent by the game server
pub async fn on_game_server_message_received(
    id: u32,
    game_server_list: &GameServerList,
    command: GameServerAction,
) -> Option<Result<(), anyhow::Error>> {
    let bus = game_server_list.bus();
    let server_state = game_server_list.get_server_copy_by_id(id).unwrap();

    match command {
        GameServerAction::Exit => {
            return Some(Ok(()));
        }
        GameServerAction::Relay { message } => {
            if !server_state.authorized {
                return None;
            }

            // relay the message to all other transmitters
            bus.bus_tx()
                .send(BusAction::Relay {
                    sender_id: id,
                    message: message.trim().to_string(),
                })
                .await
                .unwrap();
        }
        GameServerAction::ClientBegin { name } => {
            if !server_state.authorized {
                return None;
            }

            let count = game_server_list.change_player_count(id, 1);

            bus.bus_tx()
                .send(BusAction::Relay {
                    sender_id: id,
                    message: format!(
                        "{} joined @ {} ({} players online)",
                        name,
                        server_state.hostname,
                        count.unwrap()
                    ),
                })
                .await
                .unwrap()
        }
        GameServerAction::ClientDisconnect { name } => {
            if !server_state.authorized {
                return None;
            }

            let count = game_server_list.change_player_count(id, -1);

            bus.bus_tx()
                .send(BusAction::Relay {
                    sender_id: id,
                    message: format!(
                        "{} disconnected @ {} ({} players online)",
                        name,
                        server_state.hostname,
                        count.unwrap()
                    ),
                })
                .await
                .unwrap()
        }
        GameServerAction::Login { .. } => {}
        GameServerAction::Authorize { result } => {
            if server_state.authorized {
                return None
            }

            match result {
                AuthorizeClientMessage::Request { key, hostname, player_count } => {
                    bus.bus_tx()
                        .send(BusAction::AuthorizeRequest {
                            sender_id: id,
                            key,
                            hostname,
                            player_count
                        })
                        .await
                        .unwrap()
                },
                _ => {}
            }
        }
    }

    None
}

// handle a message received from our bus
async fn handle_bus_message(
    bus_rx: &mut Receiver<BusAction>,
    client_sender: &GameServerSender<'_>,
) -> bool {
    let command = bus_rx.recv().await;
    if command.is_none() {
        return false;
    }

    let command = command.unwrap();
    match command {
        BusAction::Relay { message, .. } => {
            // relay the message to the client
            let message = message.chars().filter(|x| x.is_ascii()).collect::<String>();
            client_sender
                .send_command(GameServerAction::Relay { message })
                .await
        },
        BusAction::AuthorizeResult { ok } => {
            if ok {
                client_sender
                    .send_command(GameServerAction::Authorize {
                        result: AuthorizeClientMessage::Authorized
                    })
                .await
            } else {
                client_sender
                    .send_command(GameServerAction::Authorize {
                        result: AuthorizeClientMessage::Unauthorized,
                    })
                    .await
            }
        }
        _ => {}
    }

    true
}
