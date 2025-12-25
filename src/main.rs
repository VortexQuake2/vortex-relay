mod character;
mod discord;
mod instance;
mod messages;
mod proto;
mod serverlist;

use crate::discord::{make_discord_bot, DISCORD_CHANNEL};
use crate::instance::GameServer;
use crate::messages::BusAction;
use crate::serverlist::{BusContext, GameServerList, SharedBusContext, VortexServer};
use colog;
use log::{debug, info, trace};
use serenity::all::Channel::Guild;
use serenity::all::{ChannelId, CreateMessage};
use std::env;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::io::{self};
use tokio::net::TcpListener;
use tokio::sync::mpsc::Receiver;

pub struct DiscordContext {
    http: Option<Arc<serenity::http::Http>>,
    send_channel: Option<u64>,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    env::set_var("RUST_LOG", "none,vrxbridge=info");
    dotenvy::dotenv().ok();
    colog::init();

    // simple list of keys in server environment
    let authorized_keys = env::var("SERVER_KEYS")
        .map(|f| f.split(",").map(|s| s.trim().to_string()).collect())
        .unwrap_or(Vec::new());

    let (bus_tx, bus_rx) = tokio::sync::mpsc::channel(32);
    let bus = SharedBusContext::new(BusContext {
        authorized_keys,
        bus_tx: bus_tx.clone(),
    });

    let game_server_list = GameServerList::new(&bus);

    let send_channel = env::var("DISCORD_SEND_CHANNEL")
        .unwrap_or("0".to_string())
        .parse()
        .ok();

    let discord_bot = make_discord_bot(&bus_tx).await;
    let mut task_discord = None;
    let mut discord_http = None;

    if let Some((task, http)) = discord_bot {
        task_discord = Some(task);
        discord_http = Some(http);
    }

    // server intercommunication
    info!("Starting server message pump");

    // "inter-server communication"
    let clients = game_server_list.clone();
    let task_bus = tokio::spawn(async move {
        bus_message_pump(
            bus_rx,
            clients,
            DiscordContext {
                http: discord_http,
                send_channel,
            },
        )
        .await;
    });

    let clients = game_server_list.clone();
    let task_listener = tokio::spawn(async move {
        relay_listener(clients).await;
    });

    if let Some(task_discord) = task_discord {
        let (r1, r2, r3) = tokio::join!(task_bus, task_listener, task_discord);
        r1?;
        r2?;
        r3?;
    } else {
        let (r1, r2) = tokio::join!(task_bus, task_listener);
        r1?;
        r2?;
    }

    Ok(())
}

async fn relay_listener(mut clients_list: GameServerList) {
    let port = env::var("PORT").unwrap_or("9999".to_string());
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    let addr_whitelist = env::var("ADDR_WHITELIST").unwrap_or("".to_string());
    let addr_whitelist: Vec<IpAddr> = addr_whitelist
        .split(",")
        .filter_map(|x| x.parse().ok())
        .collect();

    let mut id = 0;

    info!("Server started on port {}", port);
    info!("Whitelisted addresses: {:?}", addr_whitelist);
    loop {
        let (socket, address) = listener.accept().await.unwrap();
        let (server_channel, rx) = tokio::sync::mpsc::channel(32);
        let mut authorized = false;

        if !addr_whitelist.is_empty() && !addr_whitelist.contains(&address.ip()) {
            info!("Connection from {:?} not in whitelist", address);
        } else {
            authorized = true;
        }

        clients_list.add(VortexServer {
            id,
            address,
            server_channel,
            authorized,
            hostname: "".to_string(),
            player_count: 0,
        });

        info!("New connection from {:?} ID: {}", socket.peer_addr(), id);

        // Create a new instance
        let instance = GameServer::new(id, socket, rx, clients_list.clone());

        tokio::spawn(async move {
            instance.main_loop().await;
        });

        id += 1;
    }
}

async fn bus_message_pump(
    mut bus_rx: Receiver<BusAction>,
    clients: GameServerList,
    discord: DiscordContext,
) {
    let mut clients = clients;
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
        let cmd = bus_rx.recv().await.unwrap();

        trace!("Received command: {:?}", cmd);
        match cmd {
            BusAction::Relay {
                sender_id: id,
                ref message,
            } => {
                let cl = clients.get_server_address(id);

                info!("Msg from server {} ({}): {}", id, cl, message);

                let channels = clients.get_servers_snapshot();

                debug!("Broadcasting to all servers: {:?}", channels);

                // send to all other vortex servers
                for cl in channels.iter() {
                    if cl.id == id {
                        continue;
                    }

                    let new_cmd = cmd.clone();
                    trace!("Broadcasting to server {}: {:?}", cl.id, message);
                    cl.server_channel.send(new_cmd).await.ok();
                }

                // if it was not sent by discord, send this message to it
                if id != DISCORD_CHANNEL {
                    if let Some(channel) = channel.as_ref() {
                        let state = channel
                            .send_message(
                                &discord_http.as_ref().unwrap(),
                                CreateMessage::new().content(message),
                            )
                            .await;

                        if let Err(e) = state {
                            info!("Failed to send message to discord: {:?}", e);
                        }
                    }
                }
            }
            BusAction::ServerOffline { sender_id: id } => {
                let cl = clients.get_server_address(id);
                info!("Server {} ({}) offline", id, cl);

                clients.remove(id)
            }
            BusAction::AuthorizeRequest { sender_id: id, key, hostname, player_count } => {
                let server = clients.get_server_copy_by_id(id);
                let result = clients.authorize(id, key);
                if let Some(server) = server {
                    clients.set_hostname(id, hostname);
                    clients.set_player_count(id, player_count);

                    server.server_channel
                        .send(BusAction::AuthorizeResult { ok: result })
                        .await
                        .ok();
                }
            },
            BusAction::AuthorizeResult { .. } => { /* this is not something _we_ must handle */ }
        };

        debug!("Finished processing command");
    }
}
