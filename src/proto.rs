use tokio::io::AsyncWriteExt;
use std::error::Error;
use std::io::ErrorKind::WouldBlock;
use std::fmt::Display;
use std::io;
use std::io::Write;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use log::{error, info, trace};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender};
use crate::instance::on_game_server_message_received;
use crate::messages::{GameServerAction, };
use crate::serverlist::{GameServerList};

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
    client_command: GameServerAction
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


        let client_command;
        if size as usize <= bytes.len() - 8 {
            client_command = rmp_serde::from_slice( &bytes[8..8+(size as usize)] )?;
        } else {
            return Err(MessageParseError::TooSmall.into());
        }

        Ok(Self {
            magic,
            size,
            client_command
        })
    }
}

impl From<GameServerAction> for ClientPackage {
    fn from(value: GameServerAction) -> Self {
        let vec = rmp_serde::to_vec(&value).unwrap();
        Self {
            magic: MAGIC,
            size: vec.len() as u32,
            client_command: value
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


pub async fn game_socket_send_receive_all(
    game_data_queue: &mut Receiver<Vec<u8>>,
    game_socket: &mut TcpStream,
    game_server_list: &GameServerList,
    game_server_id: u32,
)
    -> Result<bool, anyhow::Error>
 {
    let mut recv_buffer = vec![0; MAX_PACKET_SIZE];
    let mut pending_buffer = vec![];

    tokio::select! {
        n = game_socket.read(&mut recv_buffer) => {
            match n {
                Ok(0) => {
                    info!("Connection from {:?} closed", game_socket.peer_addr());
                    return Ok(false);
                }
                Ok(n) => {
                    // hexdump::hexdump(&buf[..n]);
                    trace!("{}: Received {} bytes", game_server_id, n);

                    if n < 8 {
                        error!("invalid message size; expected at least 8 bytes, actual = {}", n);
                        return Ok(false);
                    }

                    // get the command from a messagepack frame describing the command
                    let mut start = 0;
                    let full_buffer = [&pending_buffer[..], &recv_buffer[..n]].concat();
                    pending_buffer.clear();
                    while start < full_buffer.len() {
                        let data = &full_buffer[start..];
                        let message = ClientPackage::from_bytes(data);
                        if message.is_err() {
                            if let Some(e) = message.err() {
                                if let Some(e2) = e.downcast_ref::<MessageParseError>() {
                                    if *e2 == MessageParseError::TooSmall {
                                        pending_buffer.extend_from_slice(data);
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

                        on_game_server_message_received(
                            game_server_id,
                            game_server_list,
                            message.client_command.clone()
                        ).await;

                        start += 8 + message.size as usize;
                    }
                }
                Err(e) => {
                    if e.kind() != WouldBlock {
                        error!("failed to read from socket; err = {:?}", e);
                        return Ok(false);
                    }
                }
            }
        },
        msg = game_data_queue.recv() => {
            if let Some(msg) = msg {
                game_socket.write_all(&msg).await?;
                trace!("{}: Sent {} bytes", game_server_id, msg.len());
            } else {
                return Ok(false);
            }
        }
    }

    Ok(true)
}

pub struct GameServerSender<'a>(pub &'a Sender<Vec<u8>>);

impl<'a> GameServerSender<'a> {
    pub async fn send_command(&self, command: GameServerAction) {
       self.0.send(
           ClientPackage::from(command).into()
       ).await.unwrap()
    }
}