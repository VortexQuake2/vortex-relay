use crate::messages::BusAction;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone)]
pub struct VortexServer {
    pub id: u32,
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
}

#[derive(Debug, Clone)]
pub struct GameServerList {
    list: Arc<Mutex<Vec<VortexServer>>>,
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
            bus: bus.clone(),
        }
    }

    pub fn bus(&self) -> &SharedBusContext {
        &self.bus
    }

    pub(crate) fn authorize(&self, id: u32, key: String) -> bool {
        let bus = self.bus.ctx.lock().unwrap();
        server_by_id!(self, id, server {
           server.authorized = bus.authorized_keys.contains(&key);
            return server.authorized
        });

        false
    }

    pub fn get_server_copy_by_id(&self, id: u32) -> Option<VortexServer> {
        server_by_id!(self, id, server {
            return Some(server.clone());
        });

        None
    }

    pub fn change_player_count(&self, id: u32, delta: i32) {
        server_by_id!(self, id, server {
            server.player_count = server.player_count.checked_add_signed(delta).unwrap();
        });
    }

    pub fn set_player_count(&self, id: u32, count: u32) {
        server_by_id!(self, id, server {
            server.player_count = count;
        });
    }

    pub(crate) fn set_hostname(&self, id: u32, hostname: String) -> bool {
        server_by_id!(self, id, server {
            server.hostname = hostname;
            return true;
        });

        false
    }

    pub fn get_hostname(&self, id: u32) -> Option<String> {
        server_by_id!(self, id, server {
            return Some(server.hostname.clone());
        });

        None
    }

    pub fn add(&mut self, server: VortexServer) {
        let mut lock = self.list.lock().unwrap();
        lock.push(server)
    }

    pub fn remove(&mut self, id: u32) {
        let mut lock = self.list.lock().unwrap();
        lock.retain(|x| x.id != id);
    }

    // return a cloned list of the currently open vortex servers
    pub fn get_servers_snapshot(&self) -> Vec<VortexServer> {
        let lock = self.list.lock().unwrap();
        lock.iter().map(|x| x.clone()).collect()
    }

    pub fn is_authorized(&self, id: u32) -> bool {
        let lock = self.list.lock().unwrap();
        lock.iter()
            .find(|x| x.id == id)
            .is_some_and(|x| x.authorized)
    }

    pub fn get_server_address(&self, id: u32) -> String {
        let lock = self.list.lock().unwrap();
        lock.iter()
            .find(|x| x.id == id)
            .map_or("unknown".to_string(), |x| x.address.to_string())
    }
}
