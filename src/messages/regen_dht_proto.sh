#!/bin/sh

docker run --rm -v `pwd`:/usr/code:z -w /usr/code d1vyank/rust:nightly-2019-03-20 /bin/bash -c " \
    apt-get update; \
    apt-get install -y protobuf-compiler; \
    cargo install --git https://github.com/stepancheg/rust-protobuf protobuf-codegen; \
    protoc --rust_out=\"serde_derive=true\":. routing.proto; \
    sed -i 's/Serialize/serde::Serialize/' routing.rs; \
    sed -i 's/Deserialize/serde::Deserialize/' routing.rs;"


#     cargo install --git https://github.com/stepancheg/rust-protobuf --rev f2e9b257 protobuf-codegen; \
