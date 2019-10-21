use super::RPCError;
use crate::keyutil;
use crate::kvstore;
use crate::kvstore::KVStore;
use crate::messages::{routing, util as msgutil};
use crate::routing_table;
use crate::rpc;

use futures::lock::Mutex;
use futures::{channel::mpsc, prelude::*};
use log::{error, info, trace};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use tarpc::{client, context};

// Issues RPCs to other DHT nodes
pub struct DHTClient {
    alpha: u16,
    myself: routing_table::Peer,
    kvstore: Arc<Mutex<kvstore::MemoryStore>>,
    routing_table: Arc<Mutex<routing_table::RoutingTable>>,
}

impl DHTClient {
    /// Returns a DHT client with the given parameters
    pub fn new(
        alpha: u16,
        myself: routing_table::Peer,
        kvstore: Arc<Mutex<kvstore::MemoryStore>>,
        routing_table: Arc<Mutex<routing_table::RoutingTable>>,
    ) -> Self {
        DHTClient {
            alpha: alpha,
            myself: myself,
            kvstore: kvstore,
            routing_table: routing_table,
        }
    }

    /// Puts the given key value pair in the DHT by issuing a store RPC to the 'k' nearest peers
    /// to the given key
    /// Returns an 'RPCError' if all the stores issued fail
    pub async fn store(&self, key: u128, value: Vec<u8>) -> Result<(), RPCError> {
        let request = msgutil::create_store_request(&self.myself, key, &value);
        let mut v = Vec::new();
        let mut failures = 0;
        let mut last_error = RPCError {
            error_code: routing::message::ErrorCode::OK,
            message: "".to_string(),
        };

        let peers = self.nearest_peers(key).await;
        let num_reqs = peers.len();

        for peer in peers {
            v.push(store_value(peer.address.parse().unwrap(), request.clone()))
        }

        for r in future::join_all(v).await {
            let validation_result = msgutil::validate_store_result(r);
            if validation_result.is_err() {
                failures = failures + 1;
                last_error = validation_result.err().unwrap();
            }
        }

        if failures == num_reqs {
            error!("All store RPCs failed. Last error: {:?}", last_error);
            return Err(last_error);
        }

        Ok(())
    }

    /// Issues a 'find node' RPC to the given address
    pub async fn find_node(&self, addr: SocketAddr, target: u128) -> io::Result<routing::Message> {
        let conn = tarpc_bincode_transport::connect(&addr).await?;
        let mut client = rpc::dht::ServiceClient::new(client::Config::default(), conn).spawn()?;
        client
            .find_node(
                context::current(),
                msgutil::create_find_node_request(target, &self.myself),
            )
            .await
    }

    /// Iteratively queries 'alpha' nodes concurrently until value is found or we run out of candidates
    pub async fn find_value(&self, key: u128) -> Result<Vec<u8>, RPCError> {
        // look for value in local kv store
        match self.find_value_local(key).await {
            Some(v) => return Ok(v),
            None => (),
        }

        let mut candidates = self.nearest_peers(key).await;
        let mut queried = Vec::new();
        let mut in_progress: u16 = 0;
        let (tx, mut rx) = mpsc::unbounded();

        loop {
            if candidates.len() == 0 {
                break;
            }
            // Sort in reverse order (i.e nearest peer to target is at the end of the vector)
            candidates.sort_by(|a, b| (a.id ^ key).cmp(&(b.id ^ key)).reverse());

            // Spawn upto 'alpha' concurrent queries to available candidate peers
            while in_progress < self.alpha && candidates.len() > 0 {
                let peer = candidates.pop().unwrap();
                let request = msgutil::create_find_value_request(key, &self.myself);
                spawn_query(request, peer.clone(), tx.clone());
                in_progress = in_progress + 1;
                queried.push(peer.clone());
            }

            // await and handle responses from spawned queries
            match rx.next().await {
                Some(result) => {
                    in_progress = in_progress - 1;
                    if result.is_err() {
                        info!("failed to query peer {:?}", result.err().unwrap());
                        break;
                    }

                    let response = result.ok().unwrap();

                    if response.code.enum_value_or_default() != routing::message::ErrorCode::OK {
                        info!("Find value RPC returned error code: {:?}", response.code);
                    }
                    if !response.value.is_empty() {
                        // TODO: cancel in-progress queries?
                        return Ok(response.value);
                    }

                    // add new candidates
                    // TODO: ping and add closer peers to routing table.
                    let mut new_candidates = response
                        .closerPeers
                        .iter()
                        .map(|p| msgutil::proto_peer_to_peer(p))
                        .filter(|p| !queried.contains(&p) && !candidates.contains(&p))
                        .collect();
                    candidates.append(&mut new_candidates);
                }
                None => break,
            }
        }

        error!("Value not found, ran out of peers to query");
        Err(RPCError {
            error_code: routing::message::ErrorCode::NOT_FOUND,
            message: "value not found".to_string(),
        })
    }

    async fn nearest_peers(&self, key: u128) -> Vec<routing_table::Peer> {
        self.routing_table.lock().await.k_nearest_peers(key)
    }

    async fn find_value_local(&self, key: u128) -> Option<Vec<u8>> {
        match self.kvstore.lock().await.get(keyutil::key_to_bytes(key)) {
            Ok(v) => return v.clone(),
            Err(e) => {
                error!("could not read from kvstore {:?}", e);
                return None;
            }
        }
    }
}

// spawns an asynchronous lookup whose result is sent to the given channel
fn spawn_query(
    request: routing::Message,
    peer: routing_table::Peer,
    mut tx: mpsc::UnboundedSender<io::Result<routing::Message>>,
) {
    let mut tx_error = tx.clone();
    tokio::spawn(async move {
        match find_value(peer.address.parse().unwrap(), request.clone()).await {
            Ok(response) => {
                match tx.unbounded_send(Ok(response)) {
                    Err(_e) => (), // Sends fail when reciever is out of scope i.e. the caller has returned. Okay to ignore.
                    Ok(()) => tx.disconnect(),
                }
            }
            Err(err) => {
                match tx_error.unbounded_send(Err(err)) {
                    Err(_e) => (), // Sends fail when reciever is out of scope i.e. the caller has returned. Okay to ignore.
                    Ok(()) => tx_error.disconnect(),
                }
            }
        }
    });
}

async fn store_value(addr: SocketAddr, msg: routing::Message) -> io::Result<routing::Message> {
    trace!("Issuing store_value RPC to peer {:}", addr.to_string());

    let conn = tarpc_bincode_transport::connect(&addr).await?;
    let mut client = rpc::dht::ServiceClient::new(client::Config::default(), conn).spawn()?;
    client.store(context::current(), msg).await
}

async fn find_value(addr: SocketAddr, msg: routing::Message) -> io::Result<routing::Message> {
    trace!("Issuing find_value RPC to peer {:}", addr.to_string());

    let conn = tarpc_bincode_transport::connect(&addr).await?;
    let mut client = rpc::dht::ServiceClient::new(client::Config::default(), conn).spawn()?;
    client.find_value(context::current(), msg).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor;
    use futures::future::Ready;
    use std::sync::Mutex as sync_mutex;
    use tarpc::server::{self, Handler};
    use tokio::runtime::Runtime;

    use crate::mock::*;
    use crate::rpc::dht::Service;

    #[test]
    fn test_store() {
        // scale
        let n = 15;

        // mock data and structures
        let key = 123;
        let value = vec![1, 2, 3];
        let me = mock_local_peer("0.0.0.0:1234", 1234);
        let routing_table = mock_routing_table(&me, n);
        let dht_client = DHTClient::new(5, me, mock_kv_store(), routing_table.clone());

        let client_runtime = Runtime::new().unwrap();
        let server_runtime = Runtime::new().unwrap();

        // run n servers and add them to our routing table
        let servers = run_n_servers(n, &server_runtime);
        let mut rt = executor::block_on(routing_table.lock());
        for server in servers.iter() {
            // ignore bucket capacity reached error
            let _r = rt.update(routing_table::Peer {
                id: keyutil::create_id(),
                address: server.address.to_string(),
            });
        }
        drop(rt);

        client_runtime.block_on(async {
            dht_client.store(key, value.clone()).await.unwrap();
        });

        // Assert each server received correct request
        for server in servers.iter() {
            let req = server.last_request.lock().unwrap();
            assert_eq!(req.key, keyutil::key_to_bytes(key), "");
            assert_eq!(req.value, value);
        }
    }

    #[test]
    fn test_retrieve() {
        // scale
        let n = 15;

        // mock data and structures
        let me = mock_local_peer("0.0.0.0:1234", 1234);
        let routing_table = mock_routing_table(&me, 10);
        let dht_client = DHTClient::new(5, me, mock_kv_store(), routing_table.clone());
        let key = 123;

        let client_runtime = Runtime::new().unwrap();
        let server_runtime = Runtime::new().unwrap();

        // create n servers and add them to our routing table
        let mut servers = run_n_servers(n, &server_runtime);
        let mut rt = executor::block_on(routing_table.lock());
        for server in servers.iter_mut() {
            let _e = rt.update(routing_table::Peer {
                id: keyutil::create_id(),
                address: server.address.to_string(),
            });
        }
        drop(rt);

        client_runtime.block_on(async {
            dht_client.find_value(key).await.unwrap();
        });

        // Assert any server received correct request
        let mut count = 0;
        for server in servers.iter() {
            let req = server.last_request.lock().unwrap();
            if req.key == keyutil::key_to_bytes(key) {
                count += 1;
            }
        }
        assert!(count >= 1);
    }

    fn run_n_servers(n: usize, rt: &Runtime) -> Vec<MockDHTServer> {
        let mut servers = vec![];

        for _i in 0..n {
            let address = "127.0.0.1:0".to_string();
            let transport = tarpc_bincode_transport::listen(&address.parse().unwrap()).unwrap();
            let mock_server = MockDHTServer::new(transport.local_addr().to_string());
            servers.push(mock_server.clone());

            trace!("running a mock server at {:?}", mock_server.address);
            let server = server::new(server::Config::default())
                .incoming(transport.filter_map(|r| future::ready(r.ok())))
                .respond_with(mock_server.serve());

            rt.spawn(server);
        }

        servers
    }

    #[derive(Clone)]
    struct MockDHTServer {
        pub last_request: Arc<sync_mutex<routing::Message>>,
        pub address: String,
        pub retrieve_result: Vec<u8>,
    }

    impl MockDHTServer {
        pub fn new(addr: String) -> Self {
            MockDHTServer {
                last_request: Arc::new(sync_mutex::new(routing::Message::new())),
                address: addr,
                retrieve_result: vec![1, 2, 3],
            }
        }
    }

    impl rpc::dht::Service for MockDHTServer {
        type StoreFut = Ready<routing::Message>;
        type FindNodeFut = Ready<routing::Message>;
        type FindValueFut = Ready<routing::Message>;

        fn store(self, _: context::Context, message: routing::Message) -> Self::StoreFut {
            let mut last_req = self.last_request.lock().unwrap();
            *last_req = message;
            future::ready(msgutil::create_store_response(true))
        }

        fn find_node(self, _: context::Context, _message: routing::Message) -> Self::FindNodeFut {
            future::ready(routing::Message::new())
        }

        fn find_value(self, _: context::Context, message: routing::Message) -> Self::FindValueFut {
            let mut last_req = self.last_request.lock().unwrap();
            *last_req = message.clone();
            future::ready(msgutil::create_find_value_response(
                message.key.clone(),
                self.retrieve_result.clone(),
            ))
        }
    }
}
