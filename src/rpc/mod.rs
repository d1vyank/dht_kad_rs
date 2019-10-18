use crate::buckets;
use crate::keyutil;
use crate::kvstore;
use crate::kvstore::KVStore;
use crate::messages::{msgutil, routing};
use futures::lock::{Mutex, MutexGuard};
use futures::prelude::*;
use log::error;
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
    }
}

#[derive(Clone)]
pub struct DHTServer {
    kvstore: Arc<Mutex<kvstore::MemoryStore>>,
    routing_table: Arc<Mutex<buckets::RoutingTable>>,
}

impl DHTServer {
    pub fn new(
        kvstore: Arc<Mutex<kvstore::MemoryStore>>,
        routing_table: Arc<Mutex<buckets::RoutingTable>>,
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

    fn store(self, _: context::Context, message: routing::Message) -> Self::StoreFut {
        self.store_async(message).boxed()
    }

    fn find_node(self, _: context::Context, message: routing::Message) -> Self::FindNodeFut {
        self.find_node_async(message).boxed()
    }

    fn find_value(self, _: context::Context, message: routing::Message) -> Self::FindValueFut {
        self.find_value_async(message).boxed()
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

        let mut kvstore = self.kvstore.lock().await;
        match kvstore.put(message.key, message.value) {
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
            routing_table.k_nearest_peers(keyutil::key_from_bytes(&message.key)),
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
            Ok(value) => {
                if value.is_some() {
                    return msgutil::create_find_value_response(
                        message.key,
                        value.unwrap().to_vec(),
                    );
                }
            }
            Err(e) => {
                error!("Could not read from kv store {:}", e);
            }
        }

        // If value not found or error, compute and return 'find node' response
        self.find_node_async(message).await
    }
}

fn update_routing_table(rt: &mut MutexGuard<buckets::RoutingTable>, peer: &routing::message::Peer) {
    match rt.update(msgutil::msg_peer_to_peer(peer)) {
        Ok(()) => return,
        Err(e) => error!(
            "Could not store peer: {:} error: {:}",
            str::from_utf8(&peer.addrs).unwrap().to_string(),
            e
        ),
    }
}
