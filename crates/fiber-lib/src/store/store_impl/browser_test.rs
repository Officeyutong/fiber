use crate::store::store_impl::StoreKeyValue;
use std::{collections::BTreeMap, path::Path, sync::Arc};
use tokio::sync::Mutex;

use super::KeyValue;

#[derive(Clone, Debug)]
pub struct Store {
    data: Arc<Mutex<BTreeMap<Vec<u8>, Vec<u8>>>>,
}
impl Store {
    pub fn new<P: AsRef<Path>>(_path: P) -> Result<Self, String> {
        Ok(Self {
            data: Default::default(),
        })
    }
    pub fn open_db(_path: &Path) -> Result<Self, String> {
        Ok(Self {
            data: Default::default(),
        })
    }
    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Option<Vec<u8>> {
        let guard = self.data.blocking_lock();
        guard.get(key.as_ref()).cloned()
    }
    pub fn delete<K: AsRef<[u8]>>(&self, key: K) {
        self.data.blocking_lock().remove(key.as_ref());
    }
    pub fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&self, key: K, value: V) {
        self.data
            .blocking_lock()
            .insert(key.as_ref().to_vec(), value.as_ref().to_vec());
    }
    pub fn batch(&self) -> Batch {
        Batch {
            data: self.data.clone(),
            opts: vec![],
        }
    }
    #[allow(clippy::type_complexity)]
    pub fn prefix_iterator_with_skip_while_and_start<'a>(
        &'a self,
        _prefix: &'a [u8],
        _mode: IteratorMode<'a>,
        _skip_while: Box<dyn Fn(&[u8]) -> bool + 'static>,
    ) -> impl Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a {
        vec![].into_iter()
    }
    pub fn prefix_iterator<'a>(
        &'a self,
        prefix: &'a [u8],
    ) -> impl Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a {
        self.prefix_iterator_with_skip_while_and_start(
            prefix,
            IteratorMode::From(prefix, DbDirection::Forward),
            Box::new(|_| false),
        )
    }
}

enum Operations {
    Put(Vec<u8>, Vec<u8>),
    Delete(Vec<u8>),
}
pub struct Batch {
    data: Arc<Mutex<BTreeMap<Vec<u8>, Vec<u8>>>>,
    opts: Vec<Operations>,
}
impl Batch {
    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Option<Vec<u8>> {
        let guard = self.data.blocking_lock();
        guard.get(key.as_ref()).cloned()
    }

    pub fn put_kv(&mut self, key_value: KeyValue) {
        self.opts
            .push(Operations::Put(key_value.key(), key_value.value()));
    }

    pub fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        self.opts.push(Operations::Put(
            key.as_ref().to_vec(),
            value.as_ref().to_vec(),
        ));
    }

    pub fn delete<K: AsRef<[u8]>>(&mut self, key: K) {
        self.opts.push(Operations::Delete(key.as_ref().to_vec()));
    }

    pub fn commit(self) {
        for item in self.opts.into_iter() {
            match item {
                Operations::Put(items, items1) => self.data.blocking_lock().insert(items, items1),
                Operations::Delete(items) => self.data.blocking_lock().remove(&items),
            };
        }
    }
}
pub enum IteratorMode<'a> {
    Start,
    End,
    From(&'a [u8], DbDirection),
}

pub enum DbDirection {
    Forward,
    Reverse,
}
