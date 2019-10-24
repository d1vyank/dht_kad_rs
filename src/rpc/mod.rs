use crate::keyutil;
use crate::kvstore;
use crate::kvstore::KVStore;
use crate::messages::{routing, util as msgutil};
use crate::routing_table as rt;
use futures::lock::{Mutex, MutexGuard};
use futures::prelude::*;
use log::{error, trace};
use std::pin::Pin;
use std::str;
use std::sync::Arc;
use tarpc::context;

pub mod dht {
    use crate::messages::routing;

    #[tarpc::service]
    pub trait Service {
        async fn store(message: routing::Message) -> routing::Message;
        async fn find_node(message: routing::Message) -> routing::Message;
        async fn find_value(message: routing::Message) -> routing::Message;
        async fn ping(message: routing::Message) -> routing::Message;
    }
}

#[derive(Clone)]
pub struct DHTServer {
    kvstore: Arc<Mutex<kvstore::MemoryStore>>,
    routing_table: Arc<Mutex<rt::RoutingTable>>,
}

impl DHTServer {
    pub fn new(
        kvstore: Arc<Mutex<kvstore::MemoryStore>>,
        routing_table: Arc<Mutex<rt::RoutingTable>>,
    ) -> Self {
        DHTServer {
            kvstore: kvstore,
            routing_table: routing_table,
        }
    }
}

impl dht::Service for DHTServer {
    type StoreFut = Pin<Box<dyn Future<Output = routing::Message> + Send>>;
    type FindNodeFut = Pin<Box<dyn Future<Output = routing::Message> + Send>>;
    type FindValueFut = Pin<Box<dyn Future<Output = routing::Message> + Send>>;
    type PingFut = Pin<Box<dyn Future<Output = routing::Message> + Send>>;

    fn store(self, _: context::Context, message: routing::Message) -> Self::StoreFut {
        self.store_async(message).boxed()
    }

    fn find_node(self, _: context::Context, message: routing::Message) -> Self::FindNodeFut {
        self.find_node_async(message).boxed()
    }

    fn find_value(self, _: context::Context, message: routing::Message) -> Self::FindValueFut {
        self.find_value_async(message).boxed()
    }

    fn ping(self, _: context::Context, message: routing::Message) -> Self::PingFut {
        self.ping_async(message).boxed()
    }
}

impl DHTServer {
    async fn store_async(self, message: routing::Message) -> routing::Message {
        if !msgutil::validate_request(&message) {
            return msgutil::create_invalid_response();
        }

        update_routing_table(
            &mut self.routing_table.lock().await,
            &message.myself.unwrap(),
        );

        match self.kvstore.lock().await.put(message.key, message.value) {
            Ok(()) => msgutil::create_store_response(true),
            Err(e) => {
                error!("Could not store in kv store {:}", e);
                msgutil::create_store_response(false)
            }
        }
    }

    async fn find_node_async(self, message: routing::Message) -> routing::Message {
        if !msgutil::validate_request(&message) {
            return msgutil::create_invalid_response();
        }

        let mut routing_table = self.routing_table.lock().await;
        update_routing_table(&mut routing_table, &message.myself.unwrap());
        msgutil::create_find_node_response(
            &routing_table.k_nearest_peers(keyutil::key_from_bytes(&message.key)),
        )
    }

    async fn find_value_async(self, message: routing::Message) -> routing::Message {
        if !msgutil::validate_request(&message) {
            return msgutil::create_invalid_response();
        }

        update_routing_table(
            &mut self.routing_table.lock().await,
            &message.clone().myself.unwrap(),
        );

        match self.kvstore.lock().await.get(message.key.clone()) {
            Ok(Some(value)) => {
                return msgutil::create_find_value_response(message.key, value.to_vec());
            }
            Ok(None) => {
                trace!("value not found for find request");
            }
            Err(e) => {
                error!("Could not read from kv store {:}", e);
            }
        }

        // If value not found or error, compute and return 'find node' response
        self.find_node_async(message).await
    }

    async fn ping_async(self, message: routing::Message) -> routing::Message {
        if !msgutil::validate_request(&message) {
            return msgutil::create_invalid_response();
        }

        update_routing_table(
            &mut self.routing_table.lock().await,
            &message.clone().myself.unwrap(),
        );

        msgutil::create_ping_response()
    }
}

fn update_routing_table(table: &mut MutexGuard<rt::RoutingTable>, peer: &routing::message::Peer) {
    match table.update(msgutil::proto_peer_to_peer(peer)) {
        Ok(()) => return,
        Err(e) => error!(
            "Could not store peer: {:} error: {:}",
            str::from_utf8(&peer.addrs).unwrap().to_string(),
            e
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock;
    use futures::executor;
    use tokio::runtime::Runtime;

    #[test]
    fn store() {
        let me = mock::peer("0.0.0.0:1234", 1234);
        let (kv_store, table) = (mock::kv_store(), mock::routing_table(&me, 10));

        let (key, value) = (123, vec![1, 2, 3, 4]);

        let dht_server = DHTServer::new(kv_store.clone(), table.clone());
        let req = msgutil::create_store_request(&me, key, &value);
        let resp = executor::block_on(dht_server.store_async(req));

        // assert store was successful
        assert!(resp.code.enum_value_or_default() == routing::message::ErrorCode::OK);
        assert_eq!(
            executor::block_on(kv_store.lock())
                .get(keyutil::key_to_bytes(key))
                .unwrap()
                .unwrap(),
            value
        );

        // assert requesting peer was added to routing table
        let known_peers = executor::block_on(table.lock()).k_nearest_peers(0);
        assert_eq!(known_peers.len(), 1);
        assert_eq!(known_peers[0], me);
    }

    #[test]
    fn find_node() {
        let k = 10;
        let me = mock::peer("0.0.0.0:1234", 1234);
        let (kv_store, table) = (mock::kv_store(), mock::routing_table(&me, k));

        let mut table_handle = executor::block_on(table.lock());
        let mut peers_close_to_zero = vec![];
        for i in 1..=20 {
            let peer = mock::peer(&format!("0.0.0.0:{:}", i), i);
            peers_close_to_zero.push(peer.clone());
            let _result = table_handle.update(peer);
        }
        drop(table_handle);

        let runtime = Runtime::new().unwrap();
        let dht_server = DHTServer::new(kv_store.clone(), table.clone());

        let req = msgutil::create_find_node_request(0, &me);
        let resp = runtime.block_on(dht_server.find_node_async(req));
        assert!(resp.code.enum_value_or_default() == routing::message::ErrorCode::OK);
        let response_peers: Vec<rt::Peer> = resp
            .closerPeers
            .iter()
            .map(|p| msgutil::proto_peer_to_peer(&p))
            .collect();
        assert_eq!(response_peers[..], peers_close_to_zero[0..k]);
    }

    #[test]
    fn find_value() {
        let me = mock::peer("0.0.0.0:1234", 1234);
        let closer_peer = mock::peer("0.0.0.0:1235", 1);
        let (kv_store, table) = (mock::kv_store(), mock::routing_table(&me, 10));

        executor::block_on(table.lock())
            .update(closer_peer.clone())
            .unwrap();

        let (key, value) = (0, vec![1, 2, 3, 4]);

        let dht_server = DHTServer::new(kv_store.clone(), table.clone());

        // assert closer peer is returned if value is not found
        let req = msgutil::create_find_value_request(key, &me);
        let resp = executor::block_on(dht_server.clone().find_value_async(req));
        assert!(resp.code.enum_value_or_default() == routing::message::ErrorCode::OK);
        assert!(resp.value.len() == 0);
        assert_eq!(resp.closerPeers.len(), 2);
        assert_eq!(
            msgutil::proto_peer_to_peer(&resp.closerPeers[0]),
            closer_peer
        );

        // assert value is returned if found
        executor::block_on(kv_store.lock())
            .put(keyutil::key_to_bytes(key), value.clone())
            .unwrap();
        let req = msgutil::create_find_value_request(key, &me);
        let resp = executor::block_on(dht_server.find_value_async(req));
        assert!(resp.code.enum_value_or_default() == routing::message::ErrorCode::OK);
        assert_eq!(resp.value, value);
    }

    #[test]
    fn ping() {
        let me = mock::peer("0.0.0.0:1234", 1234);
        let (kv_store, table) = (mock::kv_store(), mock::routing_table(&me, 10));

        let dht_server = DHTServer::new(kv_store.clone(), table.clone());
        let req = msgutil::create_ping_request(&me);
        let resp = executor::block_on(dht_server.ping_async(req));

        // assert ping was successful
        assert_eq!(
            resp.code.enum_value_or_default(),
            routing::message::ErrorCode::OK
        );
    }
}
