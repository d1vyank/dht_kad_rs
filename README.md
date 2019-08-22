# dht_kad_rs

[![Build Status](https://travis-ci.com/d1vyank/dht_kad_rs.svg?branch=master)](https://travis-ci.com/d1vyank/dht_kad_rs)

An implementation of the Kademlia DHT protocol in Rust.

## Install

NOTE: This library is currently only compatible with the Rust nightly version as it relies on async-await syntax which is not yet stable.
```
$ cargo --version
cargo 1.38.0-nightly (e853aa976 2019-08-09)
$ cargo build
```

## Usage

```
use dht;

async fn dht_example() {
  let s = dht::DHTService::new(dht::DHTConfig {
      k: 20,
      alpha: 10,
      bootstrap_peers: bootstrap_peers,
      address: address,
      id: id,
  });

  s.init().await.unwrap();
  s.put(key, value).await?;
  s.get(key).await?;
}
```

See `testbed/src/main.rs` for an example CLI application that uses this library.

## Contributing

PRs accepted.

## License

MIT © Divyank Katira
