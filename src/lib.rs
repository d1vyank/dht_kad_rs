#![allow(dead_code)]

use crate::kvstore::KVStore;
use crate::messages::routing;
use crate::messages::util as msgutil;
use crate::routing_table as rt;
use crate::rpc::dht::Service;

use futures::future::join_all;
use futures::lock::Mutex;
use futures::prelude::*;
use log::{error, info, warn};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{error, fmt, io};
use tarpc::server::{self, Handler};
use tokio::timer::Interval;

mod client;
pub mod keyutil;
mod kvstore;
mod messages;
mod mock;
pub mod routing_table;
mod rpc;

/// DHT configuration
#[derive(Clone)]
pub struct DHTConfig {
    /// 'k' is a system-wide redundancy parameter
    pub k: usize,
    /// the number of peers to query concurrently
    pub alpha: u16,
    /// a list of peers to connect to at start up, address should be in IPv4/IPv6 format
    pub bootstrap_peers: Vec<routing_table::Peer>,
    /// identifier for this peer
    pub id: u128,
    /// socket address to listen on
    pub address: SocketAddr,
}

pub struct DHTService {
    conf: DHTConfig,
    local: routing_table::Peer,
    kvstore: Arc<Mutex<kvstore::MemoryStore>>,
    routing_table: Arc<Mutex<routing_table::RoutingTable>>,
    dht_client: client::DHTClient,
}

/// An implementation of the Kademlia DHT protocol
impl DHTService {
    /// Returns a new DHT service with the supplied configuration
    pub fn new(c: DHTConfig) -> Self {
        let local_peer = routing_table::Peer {
            address: c.address.to_string(),
            id: c.id,
        };

        let kvstore = Arc::new(Mutex::new(kvstore::MemoryStore::new()));
        let routing_table = Arc::new(Mutex::new(routing_table::RoutingTable::new(
            c.k,
            local_peer.clone(),
        )));
        let dht_client = client::DHTClient::new(
            c.alpha,
            local_peer.clone(),
            kvstore.clone(),
            routing_table.clone(),
        );

        DHTService {
            conf: c.clone(),
            local: local_peer.clone(),
            kvstore: kvstore,
            routing_table: routing_table,
            dht_client: dht_client,
        }
    }

    /// initializes the DHT by starting the server and bootstrapping the routing table.
    pub async fn init(&self) -> io::Result<()> {
        let rt = self.routing_table.clone();
        let kvstore = self.kvstore.clone();
        let address = self.conf.address.clone();

        tokio::spawn(async move {
            let transport = tarpc_bincode_transport::listen(&address)
                .unwrap()
                .filter_map(|r| future::ready(r.ok()));

            server::new(server::Config::default())
                .incoming(transport)
                .respond_with(rpc::DHTServer::new(kvstore, rt).serve())
                .await;
        });

        self.bootstrap().await
    }

    /// get the value corresponding to the given key from the DHT
    pub async fn get(&self, key: Vec<u8>) -> Result<Vec<u8>, RPCError> {
        let hashed_key = keyutil::calculate_hash(key);
        self.dht_client.find_value(hashed_key).await
    }

    pub async fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), RPCError> {
        // TODO: keep local map of 'my' keys for republishing -- consider ttl cache crate
        let hashed_key = keyutil::calculate_hash(key);
        self.dht_client.store(hashed_key, value).await
    }

    async fn republish() {
        //TODO: republish
    }

    async fn refresh(&mut self) {
        // TODO: refresh
    }

    async fn bootstrap(&self) -> io::Result<()> {
        let bootstrap_peers = self.conf.bootstrap_peers.clone();
        // TODO: implement Kademlia refresh during bootstrap
        if self.conf.bootstrap_peers.len() == 0 {
            warn!("No bootstrap peers provided. Blocking initialization until incoming connection");
            self.wait_for_connection().await;
        }

        let mut introductions = vec![];
        let mut candidate_peers = vec![];
        let mut alive_peers = vec![];
        for peer in bootstrap_peers.clone() {
            let address = peer.address.parse().unwrap();
            introductions.push(self.dht_client.find_node(address, self.local.id));
        }

        // TODO optimize... join all waits for all futures to complete
        let results = join_all(introductions).await;

        for (i, result) in results.iter().enumerate() {
            match result {
                Ok(resp) => {
                    for p in resp.closerPeers.iter() {
                        candidate_peers.push(msgutil::proto_peer_to_peer(&p));
                    }
                    alive_peers.push(bootstrap_peers[i].clone());
                }
                Err(err) => {
                    warn!(
                        "Failed to reach bootstrap peer {:}: {:}",
                        bootstrap_peers[i].address, err
                    );
                    continue;
                }
            }
        }
        // remove any duplicate candidates and ones we've already visited
        // TODO: struct.Vec#method.drain_filter may be useful here, once stabilized
        let mut candidate_peers: Vec<rt::Peer> = candidate_peers
            .iter()
            .filter(|p| !bootstrap_peers.contains(p))
            .cloned()
            .collect();
        candidate_peers.sort_by_key(|p| p.id);
        candidate_peers.dedup();

        // Ping candidates
        let mut introductions = vec![];
        for peer in candidate_peers.iter() {
            let address = peer.address.parse().unwrap();
            introductions.push(self.dht_client.ping(address));
        }
        let results = join_all(introductions).await;

        for (i, result) in results.iter().enumerate() {
            match result {
                Ok(_resp) => {
                    alive_peers.push(candidate_peers[i].clone());
                }
                Err(err) => {
                    warn!(
                        "Failed to ping candidate peer {:}: {:}",
                        candidate_peers[i].address, err
                    );
                    continue;
                }
            }
        }

        let mut rt = self.routing_table.lock().await;

        for peer in alive_peers.iter() {
            match rt.update(peer.clone()) {
                Ok(()) => info!("Added peer {:} to routing table", peer.address),
                Err(e) => error!("Could not add peer to routing table: {:}", e),
            }
        }
        Ok(())
    }

    async fn wait_for_connection(&self) {
        loop {
            Interval::new(Instant::now(), Duration::from_millis(1000))
                .next()
                .await;
            let rt = self.routing_table.lock().await;
            if rt.nearest_peers(self.conf.id, 2).len() >= 1 {
                info!("Incoming connection detected, proceeding.");
                break;
            }
            drop(rt);
        }
    }
}

/// Indicates an error encountered by the client while making requests to other peers
#[derive(Debug, Clone)]
pub struct RPCError {
    pub error_code: routing::message::ErrorCode,
    pub message: String,
}

impl fmt::Display for RPCError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "RPC Error Code: {:?} Description: {:?}",
            self.error_code, self.message
        )
    }
}

impl error::Error for RPCError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}
