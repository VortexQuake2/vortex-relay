use crate::models::{Item, Skills};

// almost all these types are going to be msgpack serialized into arrays and arrays of arrays, so
// ordering matters. changing order is a breaking change.

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

    // map loaded
    SpawnEntities { mapname: String },

    // notification that client logs into their player character
    Login { name: String },

    // try and authorize our instance
    #[serde(rename = "Authorize")]
    Authorize { result: AuthorizeClientMessage },


    // TODO: GDS load id on the C side should be cleared on a successful load

    // TODO: check the names of these commands in C
    // load a character
    #[serde(rename = "CharacterLoad")]
    Load { name: String, password: String, connection_id: i32, skills: Option<Skills> },

    // save a character
    #[serde(rename = "CharacterSave")]
    Save { name: String, connection_id: i32, skills: Skills },

    // save and close character (unlock)
    #[serde(rename = "CharacterSaveAndClose")]
    SaveAndClose { name: String, connection_id: i32, skills: Skills },

    // stash commands
    StashPage { name: String, page: i32, items: Vec<Option<Item>>, connection_id: i32 },

    // stash take needs to be properly implemented on the C side - if the inventory is full it has to put it back, too
    // these movements need to be logged in case of network or software failure with a date and time
    StashTake { name: String, page: i32, index: i32, success: bool, item: Option<Item>, connection_id: i32 },
    
    // This is "StashStore" in C.
    #[serde(rename = "StashStore")]
    StashStore { name: String, page: i32, index: i32, item: Item, success: bool, connection_id: i32 },

    #[serde(rename = "StashOpen")]
    StashOpen { name: String, connection_id: i32 },
    #[serde(rename = "StashOpenResult")]
    StashOpenResult { name: String, connection_id: i32, items: Vec<Option<Item>> },
    StashClose { name: String, connection_id: i32 },
    StashCloseById { name: String, id: i32, connection_id: i32 },

    // the master password field that allows us to set an owner is called "email", but it really isn't!
    SetOwner { name: String, password: String, reset: bool, owner: String, connection_id: i32 },


    // TODO: character logs! they're currently purely on the C side (with raw files) but we want something
    // we can audit across servers. (Ideally, they identify the server itself.)

    // TODO: We have a database now, so we can try and authorize servers using server keys stored in the database on top of environment variables.
    // This will help us identify where things happened.
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
    Relay { sender_id: u32, message: String },
    Load { sender_id: u32, name: String, password: String, connection_id: i32 },
    Save { sender_id: u32, name: String, connection_id: i32, skills: Skills },
    SaveAndClose { sender_id: u32, name: String, connection_id: i32, skills: Skills },
    StashPage { sender_id: u32, name: String, page: i32, connection_id: i32 },
    StashTake { sender_id: u32, name: String, page: i32, index: i32, connection_id: i32 },
    StashStore { sender_id: u32, name: String, page: i32, index: i32, item: Item, connection_id: i32 },
    StashOpen { sender_id: u32, name: String, connection_id: i32 },
    StashClose { sender_id: u32, name: String, connection_id: i32 },
    StashCloseById { sender_id: u32, name: String, id: i32, connection_id: i32 },
    SetOwner { sender_id: u32, name: String, password: String, reset: bool, owner: String, connection_id: i32 },

    LoadResult { name: String, connection_id: i32, skills: Option<Skills> },
    StashPageResult { name: String, page: i32, items: Vec<Option<Item>>, connection_id: i32 },
    StashTakeResult { name: String, page: i32, index: i32, success: bool, item: Option<Item>, connection_id: i32 },
    StashStoreResult { name: String, page: i32, index: i32, success: bool, connection_id: i32 },
    StashOpenResult { name: String, connection_id: i32, items: Vec<Option<Item>> },

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


#[cfg(test)]
mod tests {
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