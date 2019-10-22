use crate::kvstore::{self, KVStore};
use crate::routing_table as rt;
use futures::lock::Mutex;
use std::sync::Arc;

pub fn peer(address: &str, id: u128) -> rt::Peer {
    rt::Peer {
        address: address.to_string(),
        id: id,
    }
}

pub fn kv_store() -> Arc<Mutex<kvstore::MemoryStore>> {
    Arc::new(Mutex::new(kvstore::MemoryStore::new()))
}

pub fn routing_table(local: &rt::Peer, k: usize) -> Arc<Mutex<rt::RoutingTable>> {
    Arc::new(Mutex::new(rt::RoutingTable::new(k, local.clone())))
}
