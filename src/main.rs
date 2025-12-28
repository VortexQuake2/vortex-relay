mod character;
mod discord;
mod instance;
mod messages;
mod proto;
mod serverlist;
mod bus;

use crate::discord::{make_discord_bot};
use crate::instance::GameServer;
use crate::serverlist::{BusContext, GameServerList, SharedBusContext, VortexServer};
use colog;
use log::{info};
use std::env;
use std::net::IpAddr;
use tokio::io::{self};
use tokio::net::TcpListener;
use crate::bus::{Bus, DiscordContext};

//              GameServerAction             BusAction
//
//                      ┌──────────────────────┐   ┌──────────────────────────┐
//                      │    GameServerList   ◄┼─┐ │           Bus            │
//                      │┌────────────────────┐│ │ │  ┌────────────────────┐  │
//                      ││ GameServerInstance ││ └─┼──►  Listener Process  │  │
// ┌─────────────────┐  ││ ┌────────────────┐ ││   │  └────────────────────┘  │
// │ External Server ◄──┼┼─► Socket Process ├─┼┼─┐ │  ┌────────────────────┐  │
// └─────────────────┘  ││ └───────▲────────┘ ││ └─┼──►                    │  │
//                      ││         │          ││   │  │    Message Pump    │  │
//                      ││  ┌──────┴──────┐   ││ ┌─┼──┤                    │  │
//                      ││  │ Bus Process ◄───┼┼─┘ │  └────────────────────┘  │
//                      ││  └─────────────┘   ││   └──────────────────────────┘
//                      │└────────────────────┘│
//                      └──────────────────────┘

#[tokio::main]
async fn main() -> io::Result<()> {
    dotenvy::dotenv().ok();

    let rust_log = env::var("RUST_LOG").unwrap_or("".to_string());

    if rust_log.is_empty() {
        println!("RUST_LOG is unset, setting to sane defaults");
        env::set_var("RUST_LOG", "off,vrxbridge=info");
    }

    colog::init();

    info!("package name: {}", env!("CARGO_PKG_NAME"));
    info!("version: {}", env!("CARGO_PKG_VERSION"));
    info!("RUST_LOG: {}", rust_log);


    // simple list of keys in server environment
    let authorized_keys = env::var("SERVER_KEYS")
        .map(|f| f.split(",").map(|s| s.trim().to_string()).collect())
        .unwrap_or(Vec::new());

    let (bus_tx, bus_rx) = tokio::sync::mpsc::channel(32);
    let bus_ctx = SharedBusContext::new(BusContext {
        authorized_keys,
        bus_tx: bus_tx.clone(),
    });

    let game_server_list = GameServerList::new(&bus_ctx);

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
    let mut bus = Bus::new(DiscordContext {
        http: discord_http,
        send_channel
    }, bus_rx, clients).await;
    let task_bus = tokio::spawn(async move {
        bus.bus_message_pump().await;
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
