use log::info;
use std::error;
use std::fmt;

use crate::keyutil;

#[derive(Debug, PartialEq, Clone)]
pub struct Peer {
    pub id: u128,
    pub address: String,
}

struct Bucket {
    index: u32,
    peers: Vec<Peer>,
}

pub struct RoutingTable {
    k: usize,
    local: Peer,
    buckets: Vec<Bucket>,
}

impl Bucket {
    fn new(index: u32) -> Self {
        Bucket {
            index: index,
            peers: Vec::new(),
        }
    }

    fn move_to_front_if_exists(&mut self, p: Peer) -> bool {
        let opt = self.peers.remove_item(&p);
        match opt {
            Some(peer) => {
                self.peers.push(peer);
                true
            }
            None => false,
        }
    }

    /// split current bucket by placing peers that don't belong in it in the next bucket that is
    /// returned
    fn split(&mut self, target: u128) -> Bucket {
        let i = self.index;
        let mut out = Bucket::new(i + 1);

        self.peers.retain(|peer| {
            let peer_cpl = keyutil::common_prefix_length(peer.id, target);
            if peer_cpl > i {
                out.peers.push(peer.clone());
                false
            } else {
                true
            }
        });

        out
    }
}

impl RoutingTable {
    pub fn new(k: usize, local: Peer) -> Self {
        RoutingTable {
            k: k,
            local: local,
            buckets: vec![Bucket::new(0)],
        }
    }

    /// attempts to add the given peer to the routing table.
    /// returns NoCapacityError if the appropriate bucket has reached the k limit
    pub fn update(&mut self, p: Peer) -> Result<(), NoCapacityError> {
        let cpl = keyutil::common_prefix_length(self.local.id, p.id) as usize;
        let mut bucket_index = cpl;

        // If ideal bucket doesn't exist choose last bucket
        if bucket_index > self.buckets.len() - 1 {
            bucket_index = self.buckets.len() - 1
        }

        if self.buckets[bucket_index].move_to_front_if_exists(p.clone()) {
            return Ok(());
        }

        // If bucket has space, insert
        if self.buckets[bucket_index].peers.len() < self.k {
            self.add_peer_to_bucket(bucket_index, p);
            return Ok(());
        }

        // Else if the last bucket is full, unfold and re-check position on new structure
        if bucket_index == self.buckets.len() - 1 {
            self.unfold_buckets();
            bucket_index = cpl;
            if bucket_index > self.buckets.len() - 1 {
                bucket_index = self.buckets.len() - 1
            }
            if self.buckets[bucket_index].peers.len() >= self.k {
                return Err(NoCapacityError);
            }

            self.add_peer_to_bucket(bucket_index, p);

            return Ok(());
        }

        Err(NoCapacityError)
    }

    pub fn k_nearest_peers(&self, target_id: u128) -> Vec<Peer> {
        self.nearest_peers(target_id, self.k)
    }

    pub fn nearest_peers(&self, target_id: u128, num_results: usize) -> Vec<Peer> {
        let mut bucket_index = keyutil::common_prefix_length(self.local.id, target_id) as usize;

        if self.buckets.len() <= bucket_index {
            bucket_index = self.buckets.len() - 1
        }

        let mut out = self.buckets[bucket_index].peers.to_vec();

        // If we don't have enough results look at surrounding buckets
        if out.len() < num_results {
            if bucket_index > 0 {
                out.extend(self.buckets[bucket_index - 1].peers.to_vec());
            }
            if bucket_index < self.buckets.len() - 1 {
                out.extend(self.buckets[bucket_index + 1].peers.to_vec());
            }
        }

        out.sort_by(|a, b| (a.id ^ target_id).cmp(&(b.id ^ target_id)));
        out.truncate(num_results);

        out
    }

    fn add_peer_to_bucket(&mut self, bucket_index: usize, peer: Peer) {
        info!(
            "Added peer {:} to routing table in bucket {:}",
            &peer.address, bucket_index
        );
        self.buckets[bucket_index].peers.push(peer);
    }

    // recursively unfolds buckets in routing table
    fn unfold_buckets(&mut self) {
        let index = self.buckets.len() - 1;
        let new_bucket = self.buckets[index].split(self.local.id);
        let new_bucket_len = new_bucket.peers.len();
        self.buckets.push(new_bucket);
        if new_bucket_len >= self.k {
            self.unfold_buckets();
        }
    }
}

#[derive(Debug, Clone)]
pub struct NoCapacityError;

impl fmt::Display for NoCapacityError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "bucket capacity reached")
    }
}

impl error::Error for NoCapacityError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}

#[cfg(test)]
use rand::Rng;

#[test]
fn test_bucket_move_to_front() {
    let mut b1 = bucket_with_peers(0, 100);
    let random_index = rand::thread_rng().gen_range(0, 100);
    let random_peer = b1.peers[random_index].clone();

    assert_eq!(b1.move_to_front_if_exists(random_peer.clone()), true);
    assert_eq!(b1.peers.last().unwrap(), &random_peer);

    let new_peer = Peer {
        id: keyutil::create_id(),
        address: format!("1.2.3.4:1234"),
    };
    let length_before = b1.peers.len();
    assert_eq!(b1.move_to_front_if_exists(new_peer), false);
    assert_eq!(b1.peers.len(), length_before);
}

#[test]
fn test_bucket_split() {
    let mut b1 = bucket_with_peers(0, 100);
    let my_id = keyutil::create_id();
    let b2 = b1.split(my_id);
    assert_eq!(b2.index, 1);
    assert_eq!(b1.peers.len() + b2.peers.len(), 100);
    for peer in b1.peers.iter() {
        assert_eq!(keyutil::common_prefix_length(peer.id, my_id), b1.index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routing_table_update() {
        let k = 1;
        let (mut id1, mut id2, mut id3) = (vec![], vec![], vec![]);

        // zero initialize peer IDs
        for _i in 0..16 {
            id1.push(0x00);
            id2.push(0x00);
            id3.push(0x00);
        }

        // local ID
        id1[0] = 0x10;
        // peer 1, CPL = 2
        id2[0] = 0x20;
        // peer 2, CPL = 1
        id3[0] = 0x40;

        let mut r = RoutingTable::new(k, random_peer_with_id(keyutil::key_from_bytes(&id1)));
        let p1 = random_peer_with_id(keyutil::key_from_bytes(&id2));
        let p2 = random_peer_with_id(keyutil::key_from_bytes(&id3));

        // add p1 and p2 to routing table, they should end up in bucket 0 as we unfold lazily
        assert!(r.update(p1.clone()).is_ok());
        assert!(r.update(p2.clone()).is_ok());

        assert!(r.buckets[2].peers.contains(&p1));
        assert!(r.buckets[1].peers.contains(&p2));
    }
}

#[cfg(test)]
fn bucket_with_peers(index: u32, num: u32) -> Bucket {
    let mut b = Bucket::new(index);
    for _i in 0..num {
        b.peers.push(random_peer());
    }
    b
}

#[cfg(test)]
fn random_peer() -> Peer {
    Peer {
        id: keyutil::create_id(),
        address: format!("0.0.0.0:{:}", rand::thread_rng().gen_range(1024, 65536)),
    }
}

#[cfg(test)]
fn random_peer_with_id(id: u128) -> Peer {
    Peer {
        id: id,
        address: format!("0.0.0.0:{:}", rand::thread_rng().gen_range(1024, 65536)),
    }
}
