pub(crate) use crate::messages::{BusAction, GameServerAction};
use crate::proto::{ ProtocolHandler};
use crate::serverlist::{GameServerList};
use log::error;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender};
use crate::instancehandlers::GameServerBusCommandHandler;

pub struct GameServer {
    id: u32,
    bus_tx: Sender<BusAction>,
    pub bus_receiver: Receiver<BusAction>,
    pub game_socket_data_queue_sender: Sender<Vec<u8>>,
    proto: ProtocolHandler,
}

impl GameServer {
    pub fn new(
        id: u32,
        socket: TcpStream,
        receiver: Receiver<BusAction>,
        peers: GameServerList,
    ) -> Self {
        let bus_tx = peers.bus().bus_tx().clone();
        let (
            game_socket_data_queue_sender,
            game_socket_data_queue_receiver) =
            tokio::sync::mpsc::channel::<Vec<u8>>(100);

        socket.set_nodelay(true).unwrap();

        Self {
            id,
            bus_tx,
            bus_receiver: receiver,
            game_socket_data_queue_sender,
            proto: ProtocolHandler::new(
                game_socket_data_queue_receiver,
                socket,
                peers,
                id,
            )
        }
    }

    pub(crate) async fn main_loop(mut self) {
        let alive = Arc::new(AtomicBool::new(true));
        let id = self.id;

        let alive_socket = alive.clone();
        let game_socket_communication_task = tokio::spawn(async move {
            loop {
                if !self.proto.communicate()
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
        });

        let bus_communication_task = tokio::spawn(async move {
            let mut handler =
                GameServerBusCommandHandler::new(&self.game_socket_data_queue_sender, &mut self.bus_receiver);
            let alive_bus_communication = alive;
            loop {
                if let Err(e) = handler.dispatch().await {
                    error!("Failed to process bus messages: {:?}", e);
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

