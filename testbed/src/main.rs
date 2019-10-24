#![allow(dead_code)]

use clap::{App, Arg};
use dht::{self, keyutil, routing_table};
use log::{info, trace};
use pretty_env_logger;
use rand::Rng;
use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::timer::Interval;

fn main() {
    pretty_env_logger::init_timed();
    color_backtrace::install();

    info!("Logger initialized");

    let flags = App::new("DHT")
        .version("0.1")
        .about("A Kademlia based DHT")
        .arg(
            Arg::with_name("address")
                .short("a")
                .long("address")
                .value_name("SOCKET")
                .help("Sets the address to listen on")
                .required(true),
        )
        .arg(
            Arg::with_name("bootstrap-address")
                .short("b")
                .long("bootstrap-address")
                .value_name("SOCKET")
                .help("Bootstrap address")
                .multiple(true),
        )
        .arg(
            Arg::with_name("bootstrap-id")
                .short("i")
                .long("bootstrap-id")
                .value_name("ID")
                .help("Bootstrap ID")
                .multiple(true),
        )
        .arg(
            Arg::with_name("node-id")
                .short("n")
                .long("node-id")
                .value_name("ID")
                .help("Start node with optionally specified node ID"),
        )
        .arg(
            Arg::with_name("test-iterations")
                .short("t")
                .long("test-iterations")
                .value_name("INTEGER")
                .help("Number of test iterations(puts and gets) to execute")
                .default_value("10"),
        )
        .get_matches();

    let address = flags.value_of("address").unwrap();
    let address = address.to_socket_addrs().unwrap().next().unwrap();

    let mut bootstrap_peers = vec![];
    let mut addresses = vec![];
    let mut id_list = vec![];
    match flags.values_of("bootstrap-address") {
        Some(addrs) => {
            for a in addrs {
                addresses.push(a);
            }
        }
        None => {}
    }

    match flags.values_of("bootstrap-id") {
        Some(ids) => {
            for i in ids {
                id_list.push(i);
            }
        }
        None => {}
    }
    if addresses.len() != id_list.len() {
        panic!(
            "Number of bootstrap IDs and Addresses provided are not equal {:?} {:?}",
            addresses, id_list
        )
    }

    for (index, a) in addresses.iter().enumerate() {
        bootstrap_peers.push(routing_table::Peer {
            id: id_list[index].parse().unwrap(),
            address: a.to_socket_addrs().unwrap().next().unwrap().to_string(),
        })
    }

    let id = match flags.value_of("node-id") {
        Some(value) => value.parse().unwrap(),
        None => keyutil::create_id(),
    };

    let iterations = match flags.value_of("test-iterations") {
        Some(value) => value.parse().unwrap(),
        None => 150,
    };

    let s = dht::DHTService::new(dht::DHTConfig {
        k: 20,
        alpha: 10,
        bootstrap_peers: bootstrap_peers,
        address: address,
        id: id,
    });

    info!(
        "Starting DHT service at address {:} with id {:}",
        address, id
    );

    info!("Running testbed with {:} iterations", iterations);
    let r = Runtime::new().unwrap();
    r.block_on(run_testbed(s, iterations));
}

async fn run_testbed(s: dht::DHTService, iterations: usize) {
    trace!("initializing DHT");
    s.init().await.unwrap();

    // wait for bootstrap
    Interval::new(Instant::now(), Duration::from_millis(5000))
        .next()
        .await;

    let mut test_data: Vec<([u8; 16], [u8; 16])> = vec![];
    let mut rng = rand::thread_rng();
    for _i in 0..iterations {
        let key: [u8; 16] = rng.gen();
        let value: [u8; 16] = rng.gen();
        test_data.push((key, value));
    }

    for i in 0..iterations {
        let (k, v) = &test_data[i];
        trace!("issuing PUT");
        s.put(k.to_vec(), v.to_vec()).await.unwrap();
    }

    for i in 0..iterations {
        let (k, v) = &test_data[i];
        trace!("issuing GET");
        assert_eq!(&s.get(k.to_vec()).await.unwrap(), v);
    }

    // seed values for 2 seconds
    Interval::new(Instant::now(), Duration::from_millis(2000))
        .next()
        .await;

    // exit
    info!("Test complete!");
}
