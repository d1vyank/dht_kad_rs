#![feature(
    futures_api,
    arbitrary_self_types,
    await_macro,
    async_await,
    proc_macro_hygiene,
    vec_remove_item,
    existential_type
)]
#![allow(dead_code)]

use clap::{App, Arg};
use dht::{self, buckets, keyutil};
use futures::{compat::Executor01CompatExt, prelude::*};
use log::info;
use pretty_env_logger;
use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};
use tokio::timer::Delay;
use tokio_async_await::compat::forward::IntoAwaitable;

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
                .default_value("150"),
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
        bootstrap_peers.push(buckets::Peer {
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

    tarpc::init(tokio::executor::DefaultExecutor::current().compat());

    info!("Running testbed with {:} iterations", iterations);
    tokio::run(
        run_testbed(s, iterations)
            .map_err(|e| panic!("Error {}", e))
            .boxed()
            .compat(),
    );
}

async fn run_testbed(s: dht::DHTService, iterations: usize) -> Result<(), dht::RPCError> {
    await!(s.init()).unwrap();

    // wait for bootstrap
    let when = Instant::now() + Duration::from_millis(5000);
    await!(Delay::new(when).into_awaitable()).unwrap();

    let mut test_data: Vec<(Vec<u8>, Vec<u8>)> = vec![];
    for _i in 0..iterations {
        let key = vec![
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
        ];
        let value = vec![
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
            rand::random::<u8>(),
        ];
        test_data.push((key, value));
    }

    for i in 0..iterations {
        let (k, v) = &test_data[i];
        await!(s.put(k.to_vec(), v.to_vec()))?;
    }

    for i in 0..iterations {
        let (k, v) = &test_data[i];
        assert_eq!(&await!(s.get(k.to_vec()))?, v);
    }

    info!("Test complete!");

    Ok(())
}
