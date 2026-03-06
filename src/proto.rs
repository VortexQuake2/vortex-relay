use crate::instancehandlers::GameMessageReceivedHandler;
use crate::messages::GameServerAction;
use crate::serverlist::GameServerList;
use anyhow::Context;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use log::{error, info, trace};
use rhexdump::prelude::*;
use std::error::Error;
use std::fmt::Display;
use std::io;
use std::io::ErrorKind::WouldBlock;
use std::io::Write;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc::Receiver;

const MAGIC: u32 = 0xC0A7BEEF;

// pub struct ServerMessage {
//     magic: u32,
//     size: u32,
//     server_command: BusAction
// }
//
// impl ServerMessage {
//     fn is_valid(&self) -> bool {
//         self.magic == MAGIC
//     }
//
//     fn from(bytes: &[u8]) -> Result<Self, anyhow::Error> {
//         let mut cursor = io::Cursor::new(bytes);
//         let magic = ReadBytesExt::read_u32::<LittleEndian>(&mut cursor)?;
//         let size = ReadBytesExt::read_u32::<LittleEndian>(&mut cursor)?;
//
//         let server_command;
//         if size as usize == bytes.len() - 8 {
//             server_command = rmp_serde::from_slice( &bytes[8..8 + (size as usize)] )?;
//         } else {
//             return Err(anyhow::anyhow!("Invalid size"));
//         }
//
//         Ok(Self {
//             magic,
//             size,
//             server_command
//         })
//     }
// }

const MAX_PACKET_SIZE: usize = 16 * 1024 + 8;
const MAX_MESSAGE_SIZE: usize = 16 * 1024;

pub struct ClientPackage {
    magic: u32,
    size: u32,
    client_command: GameServerAction,
}

#[derive(Debug, PartialEq)]
enum MessageParseError {
    TooSmall,
    InvalidMagic,
    // TooBig
}

impl Display for MessageParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageParseError::TooSmall => write!(f, "Invalid message size"),
            MessageParseError::InvalidMagic => write!(f, "Invalid magic number"),
            // MessageParseError::TooBig => write!(f, "Message too big")
        }
    }
}

impl Error for MessageParseError {}

impl ClientPackage {
    fn is_valid(&self) -> bool {
        self.magic == MAGIC && self.size < MAX_MESSAGE_SIZE as u32
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, anyhow::Error> {
        let mut cursor = io::Cursor::new(bytes);
        let magic = ReadBytesExt::read_u32::<LittleEndian>(&mut cursor)?;

        if magic != MAGIC {
            return Err(MessageParseError::InvalidMagic.into());
        }

        let size = ReadBytesExt::read_u32::<LittleEndian>(&mut cursor)?;

        let client_command: GameServerAction;
        if size as usize <= bytes.len() - 8 {
            let slice = &bytes[8..8 + size as usize];
            client_command = rmp_serde::from_slice(slice)?;
        } else {
            return Err(MessageParseError::TooSmall.into());
        }

        Ok(Self {
            magic,
            size,
            client_command,
        })
    }
}

impl From<GameServerAction> for ClientPackage {
    fn from(value: GameServerAction) -> Self {
        let vec = rmp_serde::to_vec(&value).unwrap();
        Self {
            magic: MAGIC,
            size: vec.len() as u32,
            client_command: value,
        }
    }
}

impl Into<Vec<u8>> for ClientPackage {
    fn into(self) -> Vec<u8> {
        let mut cursor = io::Cursor::new(vec![]);
        WriteBytesExt::write_u32::<LittleEndian>(&mut cursor, self.magic).unwrap();
        WriteBytesExt::write_u32::<LittleEndian>(&mut cursor, self.size).unwrap();
        let vec = rmp_serde::to_vec(&self.client_command).unwrap();
        Write::write_all(&mut cursor, &vec).unwrap();
        cursor.into_inner()
    }
}

pub struct ProtocolHandler {
    // bytes of stuff to send to the game server
    game_data_queue: Receiver<Vec<u8>>,

    // the socket to send to the game server
    game_socket: TcpStream,

    // the id of the game server that we're communicating with
    game_server_id: u32,

    // the handler that will handle the incoming messages from the game server
    action_handler: GameMessageReceivedHandler,

    recv_buffer: Vec<u8>,
    pending_buffer: Vec<u8>
}

// on OK case, if false, drop connection, if true, keep going
impl ProtocolHandler {
    pub fn new(
        game_data_queue: Receiver<Vec<u8>>,
        game_socket: TcpStream,
        game_server_list: GameServerList,
        game_server_id: u32,
    ) -> Self {
        Self {
            game_data_queue,
            game_socket,
            game_server_id,
            action_handler: GameMessageReceivedHandler::new(game_server_id, game_server_list),
            recv_buffer: vec![0; MAX_PACKET_SIZE],
            pending_buffer: vec![0; MAX_PACKET_SIZE],
        }
    }

    async fn bytes_received(&mut self, n: usize) -> Result<bool, anyhow::Error> {
        // hexdump::hexdump(&buf[..n]);
        trace!("{}: Received {} bytes", self.game_server_id, n);

        if n < 8 {
            error!(
                "invalid message size; expected at least 8 bytes, actual = {}",
                n
            );
            return Ok(false);
        }

        // get the command from a messagepack frame describing the command
        let mut start = 0;
        let full_buffer = [&self.pending_buffer[..], &self.recv_buffer[..n]].concat();
        self.pending_buffer.clear();
        while start < full_buffer.len() {
            let data = &full_buffer[start..];
            let message = ClientPackage::from_bytes(data).with_context(|| rhexdumps!(data));
            if message.is_err() {
                if let Some(e) = message.err() {
                    if let Some(e2) = e.downcast_ref::<MessageParseError>() {
                        if *e2 == MessageParseError::TooSmall {
                            self.pending_buffer.extend_from_slice(data);
                            return Ok(true);
                        }
                    }

                    error!("failed to parse message; err = {:?}", e);
                }

                return Ok(false);
            }

            let message = message?;

            if !message.is_valid() {
                error!("invalid message; magic = {}", message.magic);
                return Ok(false);
            }

            self.action_handler
                .dispatch(message.client_command.clone())
                .await?;

            start += 8 + message.size as usize;
        }

        Ok(true)
    }

    pub async fn communicate(&mut self) -> Result<bool, anyhow::Error> {
        tokio::select! {
            n = self.game_socket.read(&mut self.recv_buffer) => {
                return match n {
                    Ok(0) => {
                        info!("Connection from {:?} closed", self.game_socket.peer_addr());
                        Ok(false)
                    }
                    Ok(n) => self.bytes_received(n).await,
                    Err(e) => if e.kind() != WouldBlock {
                            Err(e.into())
                        } else {
                        Ok(true)
                    }
                }
            },
            msg = self.game_data_queue.recv() => {
                if let Some(msg) = msg {
                    self.game_socket.write_all(&msg).await?;
                    trace!("{}: Sent {} bytes", self.game_server_id, msg.len());
                } else {
                    anyhow::bail!("Game data protocol channel closed")
                }
            }
        }

        Ok(true)
    }
}
