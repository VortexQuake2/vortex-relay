mod instance;
mod character;

use std::env;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use log::{debug, info, trace};
use tokio::io::{self};
use tokio::sync::mpsc::{Receiver, Sender};
use crate::instance::{Instance, InstanceCommand};
use colog;
use serenity::all::{ChannelId, Context, CreateMessage, GatewayIntents, Message, Ready};
use serenity::{async_trait, Client};
use serenity::all::Channel::Guild;
use serenity::client::EventHandler;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(tag = "t")]
enum ClientCommand {
    Exit,
    Relay { message: String },
}

#[derive(Debug, Clone)]
struct VortexServer {
    id: u32,
    address: SocketAddr,
    server_channel: Sender<InstanceCommand>,
}

type ServerList = Arc<Mutex<Vec<VortexServer>>>;

fn get_server_address(clients: &ServerList, id: u32) -> String {
    let lock = clients.try_lock().unwrap();
    lock.iter()
        .find(|x| x.id == id)
        .map_or("unknown".to_string(), |x| x.address.to_string())
}

struct VrxDiscordHandler {
    transmitter: Sender<InstanceCommand>,
    receive_channels: Vec<u64>,
}

const DISCORD_CHANNEL: u32 = 0x7FFFFFF;

#[async_trait]
impl EventHandler for VrxDiscordHandler {
    async fn message(&self, _ctx: Context, new_message: Message) {
        if new_message.author.bot {
            return;
        }
        
        if !self.receive_channels.contains(&new_message.channel_id.get()) {
            return;
        }
        
        let author = new_message.author.name.clone();
        let content = new_message.content.clone();
        let chan = new_message.channel_id.name(&_ctx).await.unwrap_or("unknown".to_string());
        self.transmitter.send(InstanceCommand::Relay { 
            id: DISCORD_CHANNEL, 
            message: format!("{author}#{chan}: {content}") 
        }).await.ok();
    }

    async fn ready(&self, _ctx: Context, data_about_bot: Ready) {
        info!("Discord: Connected as {}", data_about_bot.user.name);
    }
}

struct DiscordContext {
    http: Option<Arc<serenity::http::Http>>,
    send_channel: Option<u64>
}

#[tokio::main]
async fn main() -> io::Result<()> {
    dotenvy::dotenv().ok();
    
    colog::init();
    let clients_list = Arc::new(Mutex::new(Vec::<VortexServer>::new()));
    let (maintx, mainrx) = tokio::sync::mpsc::channel(32);
    
    // let res = rmp_serde::to_vec(&ClientCommand::Relay { message: "Polzard: Polzard: say a".to_string() }).unwrap();
    // hexdump::hexdump(&res);

    let token = env::var("DISCORD_TOKEN").ok();
    let mut task_discord: Option<JoinHandle<()>> = None;
    let mut discord_http = None;
    let send_channel = env::var("DISCORD_SEND_CHANNEL")
        .unwrap_or("0".to_string())
        .parse()
        .ok();
    
    if token.is_some() {
        let token = token.unwrap();

        let receive_channels_str = env::var("DISCORD_RECEIVE_CHANNELS")
            .unwrap_or("".to_string());
        let receive_channels: Vec<u64> = receive_channels_str
            .split(",")
            .map(|x| x.parse().unwrap()).collect();
        let client = Client::builder(&token, 
                                     GatewayIntents::GUILD_MESSAGES | 
                                         GatewayIntents::MESSAGE_CONTENT)
            .event_handler(VrxDiscordHandler {
                transmitter: maintx.clone(),
                receive_channels
            })
            .await
            .expect("Err creating client");

        discord_http = Some(client.http.clone());

        task_discord = Some(tokio::spawn(async move {
            let mut client = client;
            if let Err(why) = client.start().await {
                println!("Client error: {:?}", why);
            }
        }));
    }
    
    // server intercommunication
    info!("Starting server message pump");
    let clients = Arc::clone(&clients_list);
    let task_isc = tokio::spawn(async move {
        server_message_pump(mainrx, clients, DiscordContext {
            http: discord_http,
            send_channel
        }).await;
    });

    let task_listener = tokio::spawn(async move {
        relay_listener(clients_list, maintx).await;
    });
    
    if let Some(task_discord) = task_discord {
        let (r1, r2, r3) = tokio::join!(task_isc, task_listener, task_discord);
        r1.unwrap();
        r2.unwrap();
        r3.unwrap();
    } else {
        let (r1, r2) = tokio::join!(task_isc, task_listener);
        r1.unwrap();
        r2.unwrap();
    }
   
    
    Ok(())
}

async fn relay_listener(clients_list: Arc<Mutex<Vec<VortexServer>>>, maintx: Sender<InstanceCommand>) {
    let port = env::var("PORT").unwrap_or("9999".to_string());
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();
    // let addr_whitelist = env::var("ADDR_WHITELIST").unwrap_or("".to_string());
    // let addr_whitelist: Vec<IpAddr> = addr_whitelist
    //     .split(",")
    //     .map(|x| x.parse().unwrap()).collect();

    let mut id = 0;

    info!("Server started on port {}", port);
    loop {
        let (socket, address) = listener.accept().await.unwrap();
        let (server_channel, rx) = tokio::sync::mpsc::channel(32);
        
        // if !addr_whitelist.is_empty() && !addr_whitelist.contains(&address.ip()) {
        //     info!("Connection from {:?} not in whitelist", address);
        //     continue;
        // }

        let mut lock = clients_list.try_lock().unwrap();
        lock.push(VortexServer {
            id,
            address,
            server_channel
        });

        info!("New connection from {:?} ID: {}", socket.peer_addr(), id);

        // Create a new instance
        let mut instance = Instance::new(id, maintx.clone(), socket, rx);
        tokio::spawn(async move {
            instance.main_loop().await;
        });

        id += 1;
    }
}

async fn server_message_pump(mut mainrx: Receiver<InstanceCommand>, clients: ServerList, discord: DiscordContext) {
    let clients = clients;
    let discord = discord;
    let mut channel = None;
    let mut discord_http = None;

    if let Some(http_client) = discord.http.as_ref() {
        if let Some(send_channel) = discord.send_channel {
            let ch = http_client.get_channel(ChannelId::from(send_channel)).await;
            if let Ok(Guild(g)) = ch {
                channel = Some(g);
            }
        }
        
        discord_http = Some(http_client.clone());
    }
    
    if channel.is_none() {
        info!("Provided discord channel not found");
    }
    
    loop {
        let cmd = mainrx.recv().await.unwrap();
        
        trace!("Received command: {:?}", cmd);
        match cmd {
            InstanceCommand::Relay { id, ref message } => {
                let cl = get_server_address(&clients, id);
                
                info!("Msg from server {} ({}): {}", id, cl, message);
                // broadcast(&clients, InstanceCommand::Relay { id, message }, id).await;

                let channels: Vec<VortexServer>;

                {
                    let lock = clients.try_lock().unwrap();
                    channels = lock.iter().map(|x| x.clone()).collect();
                }

                debug!("Broadcasting to all servers: {:?}", channels);
                for cl in channels.iter() {
                    if cl.id == id {
                        continue;
                    }

                    let newcmd = cmd.clone();
                    trace!("Broadcasting to server {}: {:?}", cl.id, message);
                    cl.server_channel.send(newcmd).await.ok();
                }

                if id != DISCORD_CHANNEL {
                    if let Some(channel) = channel.as_ref() {
                        let state = channel.send_message(
                            &discord_http.as_ref().unwrap(),
                            CreateMessage::new()
                                .content(message)
                        ).await;

                        if let Err(e) = state {
                            info!("Failed to send message to discord: {:?}", e);
                        }
                    }
                }
            }
            InstanceCommand::ServerOffline { id } => {
                let cl = get_server_address(&clients, id);
                info!("Server {} ({}) offline", id, cl);

                {
                    let mut lock = clients.try_lock().unwrap();
                    lock.retain(|x| x.id != id);
                }
            }
        }
        debug!("Finished processing command");
    }
}

