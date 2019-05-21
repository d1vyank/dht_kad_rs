use std::collections::HashMap;
use std::error;
use std::fmt;

#[derive(Debug, Clone)]
pub struct DatastoreError;

impl fmt::Display for DatastoreError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "data store error")
    }
}

impl error::Error for DatastoreError {
    fn description(&self) -> &str {
        "data store error"
    }

    fn cause(&self) -> Option<&error::Error> {
        None
    }
}

pub trait KVStore {
    fn new() -> Self;
    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), DatastoreError>;
    fn get(&self, key: Vec<u8>) -> Result<Option<Vec<u8>>, DatastoreError>;
    fn delete(&mut self, key: Vec<u8>) -> Result<Option<Vec<u8>>, DatastoreError>;
}

#[derive(Clone)]
pub struct MemoryStore {
    store: HashMap<Vec<u8>, Vec<u8>>,
}

impl KVStore for MemoryStore {
    fn new() -> Self {
        MemoryStore {
            store: HashMap::new(),
        }
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), DatastoreError> {
        let _ = self.store.insert(key, value.clone());
        Ok(())
    }

    fn get(&self, key: Vec<u8>) -> Result<Option<Vec<u8>>, DatastoreError> {
        match self.store.get(&key) {
            Some(v) => return Ok(Some(v.clone())),
            None => return Ok(None),
        };
    }

    fn delete(&mut self, key: Vec<u8>) -> Result<Option<Vec<u8>>, DatastoreError> {
        Ok(self.store.remove(&key))
    }
}
