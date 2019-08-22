# Cached rust builds with docker by: http://whitfin.io/speeding-up-rust-docker-builds/

FROM rustlang/rust:nightly

# create a new empty shell project
RUN USER=root cargo new --lib dht_kad_rs
WORKDIR /dht_kad_rs

RUN USER=root cargo new --bin testbed

# copy over your manifests
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
COPY ./testbed/Cargo.lock ./testbed/Cargo.lock
COPY ./testbed/Cargo.toml ./testbed/Cargo.toml

# this build step will cache your dependencies
RUN cargo build
RUN rm src/*.rs
RUN rm testbed/src/*.rs

# copy your source tree
COPY ./ ./

# build for release
RUN rm ./target/debug/deps/dht*
RUN rm ./target/debug/deps/libdht*
RUN rm ./target/debug/deps/testbed*
RUN cargo build

# set the startup command to run your binary
CMD ["./target/debug/testbed"]
