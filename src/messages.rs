#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(tag = "a")]
pub enum AuthorizeClientMessage {
    Request { key: String, hostname: String, player_count: u32 },
    Authorized,
    Unauthorized,
}

// messages we get and send to a q2 server
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(tag = "t")]
pub enum GameServerAction {
    // server is closing
    Exit,

    // player sends a message or play a message in our server
    Relay { message: String },

    // client finishes connecting and has entered the game
    ClientBegin { name: String },

    // client disconnects from the game
    ClientDisconnect { name: String },

    // notification that client logs into their player character
    Login { name: String },

    // try and authorize our instance
    Authorize { result: AuthorizeClientMessage },
}

// internal IPC message
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub enum BusAction {
    Relay { sender_id: u32, message: String },
    // Load { id: u32, char_name: String },
    // Save { id: u32, char_name: String, char_data: Character },
    // SaveClose { id: u32, char_name: String, char_data: Character },
    // SaveRunes { id: u32, char_name: String, runes: Vec<Rune> },

    // allow server to connect to relay
    AuthorizeRequest {
        sender_id: u32,
        key: String,
        hostname: String,
        player_count: u32,
    },
    AuthorizeResult { ok: bool },

    // the q2 server instance has gone offline
    ServerOffline { sender_id: u32 },
}