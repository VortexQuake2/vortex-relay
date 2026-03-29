use crate::messages::BusAction;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Sender;

pub type ServerId = u32;

#[derive(Debug, Clone)]
pub struct VortexServer {
    pub id: ServerId,
    pub hostname: String,
    pub address: SocketAddr,
    pub server_channel: Sender<BusAction>,
    pub authorized: bool,
    pub player_count: u32,
}

#[derive(Debug, Clone)]
pub struct BusContext {
    pub authorized_keys: Vec<String>,
    pub bus_tx: Sender<BusAction>,
}

#[derive(Debug, Clone)]
pub struct SharedBusContext {
    pub ctx: Arc<Mutex<BusContext>>,
    // kept so that we don't need to unwrap the ArcMutex every time we just want to access the tx
    bus_tx: Sender<BusAction>,
}

impl SharedBusContext {
    pub fn new(ctx: BusContext) -> Self {
        let bus_tx = ctx.bus_tx.clone();
        SharedBusContext {
            ctx: Arc::new(Mutex::new(ctx)),
            bus_tx,
        }
    }

    pub fn bus_tx(&self) -> &Sender<BusAction> {
        &self.bus_tx
    }

    pub async fn send(&self, bus_action: BusAction) -> Result<(), tokio::sync::mpsc::error::SendError<BusAction>>{
        self.bus_tx.send(bus_action).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashLock {
    // server that is locking the stash
    server_id: ServerId,

    // character that is locking the stash
    lock_owner: String
}

impl StashLock {
    pub fn new(server_id: ServerId, lock_owner: String) -> Self {
        Self { server_id, lock_owner }
    }
}


#[derive(Debug, Clone)]
pub struct GameServerList {
    list: Arc<Mutex<Vec<VortexServer>>>,
    locked_characters: Arc<Mutex<HashMap<String, ServerId>>>,
    locked_stashes: Arc<Mutex<HashMap<String, StashLock>>>,
    bus: SharedBusContext,
}

macro_rules! server_by_id {
    ($self:ident, $id:expr, $name:ident $body:block) =>
        {
            let mut list = $self.list.lock().unwrap();
            if let Some($name) = list.iter_mut().find(|v| v.id == $id)
                $body
        }

}

impl GameServerList {
    pub fn new(bus: &SharedBusContext) -> GameServerList {
        GameServerList {
            list: Arc::new(Mutex::new(Vec::new())),
            locked_characters: Arc::new(Mutex::new(HashMap::new())),
            locked_stashes: Arc::new(Mutex::new(HashMap::new())),
            bus: bus.clone(),
        }
    }

    /// ```rust
    ///     /**
    ///      * Checks if a character is locked by a different server.
    ///      *
    ///      * This function determines whether the specified character (identified by `p0`)
    ///      * is currently locked by another server. A character is considered locked if
    ///      * its entry exists in the `locked_characters` map and the server ID associated
    ///      * with the character is not the same as the given `sender_id`.
    ///      *
    ///      * # Arguments
    ///      *
    ///      * * `p0` - A reference to a `String` representing the identifier of the character
    ///      *          to check for a lock.
    ///      * * `sender_id` - The `ServerId` of the server requesting the lock status.
    ///      *
    ///      * # Returns
    ///      *
    ///      * * `true` if the character is locked by another server.
    ///      * * `false` if the character is not locked or is locked by the given `sender_id`.
    ///      *
    ///      * # Panics
    ///      *
    ///      * This function will panic if the lock on `self.locked_characters` cannot be acquired.
    ///      */
    /// ```
    pub(crate) fn is_character_locked(&self, p0: &String, sender_id: ServerId) -> bool {
        let lock = self.locked_characters.lock().unwrap();
        lock.contains_key(p0) && *lock.get(p0).unwrap() != sender_id
    }

    // returns true if have exclusive access to the stash
    pub fn is_stash_exclusive_access(&self, p0: String, p1: StashLock) -> bool {
        let lock = self.locked_stashes.lock().unwrap();
        let contains_key = lock.contains_key(&p0);

        if !contains_key {
            return false
        }

        let stashlock = lock.get(&p0).unwrap();
        stashlock == &p1
    }

    // returns true if we cannot access the stash
    pub(crate) fn is_stash_locked(&self, p0: String, p1: StashLock) -> bool {
        let lock = self.locked_stashes.lock().unwrap();
        let contains_key = lock.contains_key(&p0);

        if !contains_key {
            return false
        }

        let stashlock = lock.get(&p0).unwrap();
        stashlock != &p1
    }

    pub fn lock_stash(&self, owner_name: String, lock_owner: StashLock) -> bool {
        let mut lock = self.locked_stashes.lock().unwrap();
        let stashlock = lock.get(&owner_name);
        if stashlock.is_some_and(|id| id != &lock_owner) {
            return false;
        }
        lock.insert(owner_name,  lock_owner);
        true
    }

    pub fn unlock_stash(&self, owner_name: &str) {
        let mut lock = self.locked_stashes.lock().unwrap();
        lock.remove(owner_name);
    }

    pub fn lock_character(&self, name: String, server_id: ServerId) -> bool {
        let mut lock = self.locked_characters.lock().unwrap();
        if lock.contains_key(&name) {
            return false;
        }
        lock.insert(name, server_id);
        true
    }

    pub fn unlock_character(&self, name: &str) {
        let mut lock = self.locked_characters.lock().unwrap();
        lock.remove(name);
    }

    pub fn unlock_all_from_server(&self, server_id: ServerId) {
        {
            let mut lock = self.locked_characters.lock().unwrap();
            lock.retain(|_, id| *id != server_id);
        }
        {
            let mut lock = self.locked_stashes.lock().unwrap();
            lock.retain(|_, id| id.server_id != server_id);
        }
    }

    pub fn bus(&self) -> &SharedBusContext {
        &self.bus
    }

    pub async fn send(&self, action: BusAction) -> Result<(), tokio::sync::mpsc::error::SendError<BusAction>> {
        self.bus.send(action).await
    }

    pub fn authorize(&self, id: ServerId, key: String) -> bool {
        let bus = self.bus.ctx.lock().unwrap();
        server_by_id!(self, id, server {
           server.authorized = bus.authorized_keys.contains(&key);
            return server.authorized
        });

        false
    }

    pub fn get_server_copy_by_id(&self, id: ServerId) -> Option<VortexServer> {
        server_by_id!(self, id, server {
            return Some(server.clone());
        });

        None
    }

    pub fn change_player_count(&self, id: ServerId, delta: i32) -> Option<u32> {
        server_by_id!(self, id, server {
            server.player_count = server.player_count.saturating_add_signed(delta);
            return Some(server.player_count)
        });

        None
    }

    pub fn set_player_count(&self, id: ServerId, count: u32) {
        server_by_id!(self, id, server {
            server.player_count = count;
        });
    }

    pub fn set_hostname(&self, id: ServerId, hostname: String) -> bool {
        server_by_id!(self, id, server {
            server.hostname = hostname;
            return true;
        });

        false
    }

    // pub fn get_hostname(&self, id: u32) -> Option<String> {
    //     server_by_id!(self, id, server {
    //         return Some(server.hostname.clone());
    //     });
    //
    //     None
    // }

    pub fn add(&self, server: VortexServer) {
        let mut lock = self.list.lock().unwrap();
        lock.push(server)
    }

    pub fn remove(&self, id: ServerId) {
        let mut lock = self.list.lock().unwrap();
        lock.retain(|x| x.id != id);
    }

    // return a cloned list of the currently open vortex servers
    pub fn get_servers_snapshot(&self) -> Vec<VortexServer> {
        let lock = self.list.lock().unwrap();
        lock.iter().map(|x| x.clone()).collect()
    }

    // pub fn is_authorized(&self, id: u32) -> bool {
    //     let lock = self.list.lock().unwrap();
    //     lock.iter()
    //         .find(|x| x.id == id)
    //         .is_some_and(|x| x.authorized)
    // }

    pub fn get_server_address(&self, id: ServerId) -> String {
        let lock = self.list.lock().unwrap();
        lock.iter()
            .find(|x| x.id == id)
            .map_or("unknown".to_string(), |x| x.address.to_string())
    }
}
