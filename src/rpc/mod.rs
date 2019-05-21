use crate::buckets;
use crate::keyutil;
use crate::kvstore;
use crate::kvstore::KVStore;
use crate::messages::{msgutil, routing};
use futures::lock::{Mutex, MutexGuard};
use futures::prelude::*;
use log::error;
use std::str;
use std::sync::Arc;
use tarpc::context;

tarpc::service! {
    rpc store(message: routing::Message) -> routing::Message;
    rpc find_node(message: routing::Message) -> routing::Message;
    rpc find_value(message: routing::Message) -> routing::Message;
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

impl Service for DHTServer {
    existential type StoreFut: Future<Output = routing::Message>;
    existential type FindNodeFut: Future<Output = routing::Message>;
    existential type FindValueFut: Future<Output = routing::Message>;

    fn store(self, _: context::Context, message: routing::Message) -> Self::StoreFut {
        self.store_async(message)
    }

    fn find_node(self, _: context::Context, message: routing::Message) -> Self::FindNodeFut {
        //let x = self.find_nodes(message.key);
        self.find_node_async(message)
    }

    fn find_value(self, _: context::Context, message: routing::Message) -> Self::FindValueFut {
        self.find_value_async(message)
    }
}

impl DHTServer {
    async fn store_async(self, message: routing::Message) -> routing::Message {
        if !msgutil::validate_request(&message) {
            return msgutil::create_invalid_response();
        }

        update_routing_table(
            &mut await!(self.routing_table.lock()),
            &message.myself.unwrap(),
        );

        let mut kvstore = await!(self.kvstore.lock());
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

        let mut routing_table = await!(self.routing_table.lock());
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
            &mut await!(self.routing_table.lock()),
            &message.clone().myself.unwrap(),
        );

        match await!(self.kvstore.lock()).get(message.key.clone()) {
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
        await!(self.find_node_async(message))
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
