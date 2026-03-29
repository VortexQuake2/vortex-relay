use crate::models::{Item, Skills};
use crate::serverlist::ServerId;
// almost all these types are going to be msgpack serialized into arrays and arrays of arrays, so
// ordering matters. changing order is a breaking change.

pub type PlayerConnectionId = u64;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(tag = "a")]
pub enum AuthorizeClientMessage {
    Request { key: String, hostname: String, player_count: u32 },
    Authorized,
    Unauthorized,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub enum LoadStatus {
    Ok,
    WrongPassword,
    CharacterNotFound,
    InternalError,
    CharacterLocked
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum SetOwnerStatus {
    Ok,
    WrongPassword,
    CharacterNotFound,
    InternalError,
    OwnerAlreadySet
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum StashStatus {
    Ok,
    StashLocked,
}

// messages we get and send to a q2 server
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(tag = "t")]
pub enum GameServerAction {
    // bidirectional
    // ========================================
    // player sends a message or play a message in our server
    Relay { message: String },

    // try and authorize our instance
    Authorize { result: AuthorizeClientMessage },


    // server to relay
    // ========================================
    // server is closing
    Exit,
    // client finishes connecting and has entered the game
    ClientBegin { name: String },
    // client disconnects from the game
    ClientDisconnect { name: String },
    // map loaded
    SpawnEntities { mapname: String },
    // notification that client logs into their player character
    Login { name: String },
    // load a character
    Load { name: String, connection_id: PlayerConnectionId, password: String,  },
    // save a character
    Save { name: String, connection_id: PlayerConnectionId, skills: Box<Skills> },
    // save and close character (unlocks it)
    SaveAndClose { name: String, connection_id: PlayerConnectionId, skills: Box<Skills> },
    // stash take needs to be properly implemented on the C side - if the inventory is full it has to put it back, too
    // these movements need to be logged in case of network or software failure with a date and time
    StashTake { name: String, connection_id: PlayerConnectionId, page: i32, index: i32,  },
    // This is "StashStore" in C.
    StashStore { name: String, connection_id: PlayerConnectionId, item: Item },
    StashOpen { name: String, connection_id: PlayerConnectionId },
    StashPage { name: String, connection_id: PlayerConnectionId, page: i32 },
    StashClose { name: String, connection_id: PlayerConnectionId },
    StashCloseById { name: String, connection_id: PlayerConnectionId },
    SetOwner { name: String, connection_id: PlayerConnectionId, password: String, reset: bool, owner: String,  },

    // relay to server
    // ========================================
    LoadResult { connection_id: PlayerConnectionId, status: LoadStatus, skills: Option<Box<Skills>> },
    StashPageResult { connection_id: PlayerConnectionId, page: i32, items: Vec<Option<Item>>,  },
    StashOpenResult {
        connection_id: PlayerConnectionId,
        status: StashStatus,
        items: Option<Vec<Option<Item>>>
    },

    SetOwnerResult { connection_id: PlayerConnectionId, status: SetOwnerStatus, new_owner: Option<String> },
    StashTakeResult { connection_id: PlayerConnectionId, item: Option<Item> },
    StashStoreResult { connection_id: PlayerConnectionId, item: Option<Item> }

    // TODO: character logs! they're currently purely on the C side (with raw files) but we want something
    // we can audit across servers. (Ideally, they identify the server itself.)

    // TODO: We have a database now, so we can try and authorize servers using server keys stored in the database on top of environment variables.
    // This will help us identify where things happened.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_gsa_size() {
        // test memory of a non serialized GSA
        let size = size_of::<GameServerAction>();
        assert!(size <= 1000, "GameServerAction is too large: {}", size);
    }
}

impl From<GameServerAction> for Vec<u8> {
    fn from(value: GameServerAction) -> Vec<u8> {
        rmp_serde::to_vec(&value).unwrap()
    }
}

impl GameServerAction {
    pub fn requires_authorization(&self) -> bool {
        match self {
            GameServerAction::Authorize { .. } => false,
            _ => true,
        }
    }
}

// internal IPC message
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub enum BusAction {
    // bidirectional
    Relay { sender_id: ServerId, message: String },

    // game server to relay
    // ======================================================
    Load { sender_id: ServerId, name: String, password: String, connection_id: PlayerConnectionId },
    Save { sender_id: ServerId, name: String, connection_id: PlayerConnectionId, skills: Box<Skills> },
    SaveAndClose { sender_id: ServerId, name: String, connection_id: PlayerConnectionId, skills: Box<Skills> },
    StashPage { sender_id: ServerId, name: String, page: i32, connection_id: PlayerConnectionId },
    StashTake { sender_id: ServerId, name: String, connection_id: PlayerConnectionId, page: i32, index: i32,  },
    StashStore { sender_id: ServerId, name: String, item: Item, connection_id: PlayerConnectionId },
    StashOpen { sender_id: ServerId, name: String, connection_id: PlayerConnectionId },
    StashClose { sender_id: ServerId, name: String, connection_id: PlayerConnectionId },
    StashCloseById { sender_id: ServerId, name: String, connection_id: PlayerConnectionId },
    SetOwner { sender_id: ServerId, name: String, password: String, reset: bool, owner: String, connection_id: PlayerConnectionId },

    // allow server to connect to relay
    AuthorizeRequest {
        sender_id: ServerId,
        key: String,
        hostname: String,
        player_count: u32,
    },

    // the q2 server instance has gone offline
    ServerOffline { sender_id: ServerId },

    // relay to game server
    // ======================================================
    LoadResult { status: LoadStatus, connection_id: PlayerConnectionId, skills: Option<Box<Skills>> },
    StashPageResult {  page: i32, items: Vec<Option<Item>>, connection_id: PlayerConnectionId },
    StashTakeResult {  page: i32, index: i32, success: bool, item: Option<Item>, connection_id: PlayerConnectionId },
    StashStoreResult {  page: i32, index: i32, success: bool, connection_id: PlayerConnectionId },
    StashOpenResult { connection_id: PlayerConnectionId, status: StashStatus, items: Option<Vec<Option<Item>>> },
    AuthorizeResult { ok: bool },
    SetOwnerResult { status: SetOwnerStatus, connection_id: PlayerConnectionId, new_owner: Option<String> },
}


#[cfg(test)]
mod gsa_tests {
    use super::*;
    use rhexdump::prelude::*;

    // hi yes these are not real tests but damn if they ain't convenient
    // https://jsontotable.org/messagepack-to-json
    #[test]
    fn test_authorize_request_message() {
        let message = GameServerAction::Authorize { result: AuthorizeClientMessage::Request {
            key: "12345678".to_string(),
            hostname: "test".to_string(),
            player_count: 1,
        }};
        let data: Vec<u8> = message.into();
        rhexdump!(data);
        //[
        //   "Authorize",
        //   [
        //     "Request",
        //     "12345678",
        //     "test",
        //     1
        //   ]
        // ]
    }

    #[test]
    fn test_authorize_response_message() {
        let message = GameServerAction::Authorize {
            result: AuthorizeClientMessage::Authorized
        };
        let data: Vec<u8> = message.into();
        rhexdump!(data);
        // [
        //   "Authorize",
        //   [
        //     "Authorized"
        //   ]
        // ]
    }
}