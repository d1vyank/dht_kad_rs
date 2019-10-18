# dht_kad_rs

[![Build Status](https://travis-ci.com/d1vyank/dht_kad_rs.svg?branch=master)](https://travis-ci.com/d1vyank/dht_kad_rs)

An implementation of the Kademlia DHT protocol in Rust.

## Install

NOTE: This library is currently only compatible with the Rust beta version as it relies on async-await syntax which is not yet stable.
```
$ cargo --version
cargo 1.39.0-beta (1c6ec66d5 2019-09-30)
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
