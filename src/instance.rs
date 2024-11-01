use std::error::Error;
use std::fmt::Display;
use std::io;
use std::io::ErrorKind::WouldBlock;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use byteorder::LittleEndian;
use log::{error, info, trace};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender};
use crate::ClientCommand;
use byteorder::{WriteBytesExt, ReadBytesExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub enum InstanceCommand {
    Relay { id: u32, message: String },
    // Load { id: u32, char_name: String },
    // Save { id: u32, char_name: String, char_data: Character },
    // SaveClose { id: u32, char_name: String, char_data: Character },
    // SaveRunes { id: u32, char_name: String, runes: Vec<Rune> },
    ServerOffline { id: u32 },
}

pub struct Instance {
    id: u32,
    transmitter: Sender<InstanceCommand>,
    pub receiver: Option<Receiver<InstanceCommand>>,
    pub socket: Option<TcpStream>
}

const MAGIC: u32 = 0xC0A7BEEF;

pub struct ServerMessage {
    magic: u32,
    size: u32,
    server_command: InstanceCommand
}

impl ServerMessage {
    fn is_valid(&self) -> bool {
        self.magic == MAGIC
    }

    fn from(bytes: &[u8]) -> Result<Self, anyhow::Error> {
        let mut cursor = io::Cursor::new(bytes);
        let magic = ReadBytesExt::read_u32::<LittleEndian>(&mut cursor)?;
        let size = ReadBytesExt::read_u32::<LittleEndian>(&mut cursor)?;
        
        let server_command;
        if size as usize == bytes.len() - 8 {
            server_command = rmp_serde::from_slice( &bytes[8..8 + (size as usize)] )?;
        } else {  
            return Err(anyhow::anyhow!("Invalid size"));
        }
        
        Ok(Self {
            magic,
            size,
            server_command
        })
    }
}

const MAX_PACKET_SIZE: usize = 16 * 1024 + 8;
const MAX_MESSAGE_SIZE: usize = 16 * 1024;

pub struct ClientMessage {
    magic: u32,
    size: u32,
    client_command: ClientCommand
}

#[derive(Debug, PartialEq)]
enum MessageParseError {
    TooSmall,
    InvalidMagic,
    TooBig
}

impl Display for MessageParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageParseError::TooSmall => write!(f, "Invalid message size"),
            MessageParseError::InvalidMagic => write!(f, "Invalid magic number"),
            MessageParseError::TooBig => write!(f, "Message too big")
        }
    }
}

impl Error for MessageParseError {}

impl ClientMessage {
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

impl From<ClientCommand> for ClientMessage {
    fn from(value: ClientCommand) -> Self {
        let vec = rmp_serde::to_vec(&value).unwrap();
        Self {
            magic: MAGIC,
            size: vec.len() as u32,
            client_command: value
        }
    }
}

impl Into<Vec<u8>> for ClientMessage {
    fn into(self) -> Vec<u8> {
        let mut cursor = io::Cursor::new(vec![]);
        WriteBytesExt::write_u32::<LittleEndian>(&mut cursor, self.magic).unwrap();
        WriteBytesExt::write_u32::<LittleEndian>(&mut cursor, self.size).unwrap();
        let vec = rmp_serde::to_vec(&self.client_command).unwrap();
        Write::write_all(&mut cursor, &vec).unwrap();
        cursor.into_inner()
    }
}

impl Instance {
    pub fn new(id: u32, transmitter: Sender<InstanceCommand>, socket: TcpStream, receiver: Receiver<InstanceCommand>) -> Self {
        Self {
            id,
            transmitter,
            socket: Some(socket),
            receiver: Some(receiver)
        }
    }

    pub(crate) async fn main_loop(&mut self) {
        let (buffer_sender, buffer_receiver) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
        let alive = Arc::new(AtomicBool::new(true));

        let socket = self.socket.take().unwrap();
        let broadcaster = self.transmitter.clone();
        let id = self.id;
        let alive_clone = alive.clone();
        let socket_process = tokio::spawn(async move {
            let mut buffer_receiver = buffer_receiver;
            let mut socket = socket;
            let broadcaster = broadcaster;

            socket.set_nodelay(true).unwrap();
            loop {
                if !process_socket(&mut buffer_receiver, &mut socket, &broadcaster, id)
                    .await
                    .expect("Failed to process socket") {
                    break;
                }
            }

            notify_death(&broadcaster, id).await;
            alive_clone.store(false, std::sync::atomic::Ordering::Relaxed);
            buffer_receiver.close();
        });

        let broadcast_receiver = self.receiver.take().unwrap();
        let isc_process = tokio::spawn(async move {
            let mut broadcast_receiver = broadcast_receiver;
            let buffer_sender = buffer_sender;
            let alive = alive;
            loop {
                if !process_server_communication(&mut broadcast_receiver, &buffer_sender).await {
                    break;
                }

                if !alive.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
            }

            alive.store(false, std::sync::atomic::Ordering::Relaxed);
        });

        let (r1, r2) = tokio::join!(socket_process, isc_process);
        if let Err(e) = r1 {
            error!("Socket process failed: {:?}", e);
        }

        if let Err(e) = r2 {
            error!("Server communication process failed: {:?}", e);
        }
    }
}

async fn notify_death(transmitter: &Sender<InstanceCommand>, id: u32) {
    transmitter.send(InstanceCommand::ServerOffline {
        id
    }).await.unwrap();
}

async fn on_receive(id: u32, broadcaster: &Sender<InstanceCommand>, command: ClientCommand) -> Option<Result<(), anyhow::Error>> {
    match command {
        ClientCommand::Exit => {
            return Some(Ok(()));
        }
        ClientCommand::Relay { message } => {
            // relay the message to all other transmitters
            broadcaster.send(
                InstanceCommand::Relay {
                    id,
                    message: message.trim().to_string(),
                }).await.unwrap();

            // if let Some(webhook) = &self.webhook {
            //     relay_discord(&webhook.clone(), &message).await
            // }
        }
    }
    None
}

async fn process_socket(
    buffer_queue: &mut Receiver<Vec<u8>>,
    socket: &mut TcpStream,
    broadcaster: &Sender<InstanceCommand>, id: u32)
    -> Result<bool, anyhow::Error> {
    let mut recv_buffer = vec![0; MAX_PACKET_SIZE];
    let mut pending_buffer = vec![];

    tokio::select! {
        n = socket.read(&mut recv_buffer) => {
            match n {
                Ok(0) => {
                    info!("Connection from {:?} closed", socket.peer_addr());
                    return Ok(false);
                }
                Ok(n) => {
                    // hexdump::hexdump(&buf[..n]);
                    trace!("{}: Received {} bytes", id, n);
        
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
                        let message = ClientMessage::from_bytes(data);
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
        
                        let message = message.unwrap();
        
                        if !message.is_valid() {
                            error!("invalid message; magic = {}", message.magic);
                            return Ok(false);
                        }
        
                        on_receive(id, broadcaster, message.client_command.clone()).await;
        
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
        msg = buffer_queue.recv() => {
            if let Some(msg) = msg {
                socket.write_all(&msg).await.unwrap();
                trace!("{}: Sent {} bytes", id, msg.len());
            } else {
                return Ok(false);
            }
        }
    }

    Ok(true)
}

async fn process_server_communication(receiver: &mut Receiver<InstanceCommand>, buffer_sender: &Sender<Vec<u8>>) -> bool {
    let command = receiver.recv().await;
    if command.is_none() {
        return false;
    }

    let command = command.unwrap();
    match command {
        InstanceCommand::Relay { message, .. } => {
            // relay the message to the client
            let message = message.chars().filter(|x| x.is_ascii()).collect::<String>();
            buffer_sender.send(ClientMessage::from(ClientCommand::Relay { message }).into()).await.unwrap();
        }
        _ => {}
    }

    true
}