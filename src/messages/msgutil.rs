use crate::buckets;
use crate::messages::routing::{self, message};
use crate::keyutil;
use super::super::RPCError;

use std::{io, str};
use protobuf::ProtobufEnumOrUnknown;

pub fn msg_peer_to_peer(mp: &message::Peer) -> buckets::Peer {
    buckets::Peer {
        id: keyutil::key_from_bytes(&mp.id),
        address: str::from_utf8(&mp.addrs).unwrap().to_string(),
    }
}

pub fn msg_peer_from_peer(p: &buckets::Peer) -> message::Peer {
    let mut message_peer = message::Peer::new();
    message_peer.id = keyutil::key_to_bytes(p.id);
    message_peer.addrs = p.address.clone().into_bytes();
    message_peer
}

pub fn create_find_node_request(target: u128, myself: buckets::Peer) -> routing::Message {
    let mut request = routing::Message::new();
    request.field_type = ProtobufEnumOrUnknown::new(message::MessageType::FIND_NODE);
    request.key = keyutil::key_to_bytes(target);
    request.myself = ::protobuf::SingularPtrField::some(msg_peer_from_peer(&myself));
    request
}

pub fn create_find_value_request(key: u128, myself: buckets::Peer) -> routing::Message {
    let mut request = routing::Message::new();
    request.field_type = ProtobufEnumOrUnknown::new(message::MessageType::FIND_VALUE);
    request.key = keyutil::key_to_bytes(key);
    request.myself = ::protobuf::SingularPtrField::some(msg_peer_from_peer(&myself));
    request
}

pub fn create_store_request(myself: buckets::Peer, key: u128, value: Vec<u8>) -> routing::Message {
    let mut request = routing::Message::new();
    request.field_type = ProtobufEnumOrUnknown::new(message::MessageType::STORE);
    request.key = keyutil::key_to_bytes(key);
    request.value = value;
    request.myself = ::protobuf::SingularPtrField::some(msg_peer_from_peer(&myself));
    request
}

pub fn validate_store_result(r: io::Result<routing::Message>) -> Result<(), RPCError> {
    match r {
        Ok(message) => {
            if message.code.enum_value_or_default() != routing::message::ErrorCode::OK {
                return Err(RPCError {
                    error_code: message.code.enum_value_or_default(),
                    message: "RPC failed".to_string(),
                });
            }
        }
        Err(e) => {
            return Err(RPCError {
                error_code: routing::message::ErrorCode::INTERNAL_ERROR,
                message: e.to_string(),
            })
        }
    }
    Ok(())
}

pub fn create_invalid_response() -> routing::Message {
    let mut response = routing::Message::new();
    response.code = ProtobufEnumOrUnknown::new(routing::message::ErrorCode::BAD_REQUEST);
    response
}

pub fn create_store_response(success: bool) -> routing::Message {
    let mut response = routing::Message::new();
    response.field_type =  ProtobufEnumOrUnknown::new(routing::message::MessageType::STORE);
    if success {
        response.code =  ProtobufEnumOrUnknown::new(routing::message::ErrorCode::OK);
    } else {
        response.code =  ProtobufEnumOrUnknown::new(routing::message::ErrorCode::INTERNAL_ERROR);
    }

    response
}

pub fn create_find_value_response(key: Vec<u8>, value: Vec<u8>) -> routing::Message {
    let mut response = routing::Message::new();
    response.key = key;
    response.value = value;
    response
}

pub fn create_find_node_response(peers: Vec<buckets::Peer>) -> routing::Message {
    let mut message = routing::Message::new();
    for p in peers.iter() {
        message.closerPeers.push(msg_peer_from_peer(&p));
    }
    message
}

pub fn validate_request(msg: &routing::Message) -> bool {
    if msg.myself.is_none() {
        return false;
    }
    if msg.myself.get_ref().id.len() == 0 {
        return false;
    }
    if msg.myself.get_ref().addrs.len() == 0 {
        return false;
    }
    if str::from_utf8(msg.myself.get_ref().addrs.as_slice()).is_err() {
        return false;
    }
    if msg.key.len() == 0 {
        return false;
    }

    true
}
