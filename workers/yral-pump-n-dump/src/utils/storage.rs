use std::{fmt::Debug, ops::Deref};

use serde::{de::DeserializeOwned, Serialize};
use serde_bytes::ByteBuf;
use worker::{console_error, ListOptions, Result, Storage};

pub struct SafeStorage(Storage);

impl From<Storage> for SafeStorage {
    fn from(value: Storage) -> Self {
        Self(value)
    }
}

fn deser_bbuf<T: DeserializeOwned>(key: &str, buf: ByteBuf) -> Result<T> {
    let res = postcard::from_bytes(&buf);
    match res {
        Ok(v) => Ok(v),
        Err(e) => {
            console_error!("failed to get from storage. Key {}, err: {e}", key);
            Err(worker::Error::RustError(e.to_string()))
        }
    }
}

impl SafeStorage {
    pub async fn put(&mut self, key: impl AsRef<str>, v: &impl Serialize) -> worker::Result<()> {
        let v_ser = postcard::to_stdvec(&v).map_err(|e| worker::Error::RustError(e.to_string()))?;
        let v_raw = ByteBuf::from(v_ser);
        let v_js = serde_wasm_bindgen::to_value(&v_raw)?;

        self.0.put_raw(key.as_ref(), v_js).await?;

        Ok(())
    }

    pub async fn get<T: DeserializeOwned>(
        &self,
        key: impl AsRef<str>,
    ) -> worker::Result<Option<T>> {
        let key = key.as_ref();
        let res = self.0.get::<ByteBuf>(key).await;
        let v_js = match res {
            Ok(v) => v,
            Err(worker::Error::JsError(err)) if err.contains("No such value in storage") => {
                return Ok(None)
            }
            Err(e) => {
                console_error!("failed to get from storage. Key {}, err: {e}", key);
                return Err(e);
            }
        };

        deser_bbuf(key, v_js)
    }

    pub async fn list_with_prefix<T: DeserializeOwned>(
        &self,
        prefix: impl AsRef<str>,
    ) -> impl Iterator<Item = worker::Result<(String, T)>> {
        let v_idx = self
            .0
            .list_with_options(ListOptions::new().prefix(prefix.as_ref()))
            .await
            .unwrap_or_default();

        v_idx.entries().into_iter().map(|entry| {
            let raw_entry = entry.expect("invalid prefixed value stored?!");
            let (key, v_raw): (String, ByteBuf) =
                serde_wasm_bindgen::from_value(raw_entry).expect("invalid prefixed value stored?!");
            let v: T = deser_bbuf(&key, v_raw)?;
            Ok((key, v))
        })
    }

    pub async fn delete(&mut self, key: impl AsRef<str>) -> Result<bool> {
        self.0.delete(key.as_ref()).await
    }

    pub async fn delete_multiple(&mut self, keys: Vec<impl Deref<Target = str>>) -> Result<usize> {
        self.0.delete_multiple(keys).await
    }

    pub async fn delete_all(&mut self) -> Result<()> {
        self.0.delete_all().await
    }
}

pub struct StorageCell<T: Serialize + DeserializeOwned + Clone + Debug> {
    key: String,
    hot_cache: Option<T>,
    initial_value: Option<Box<dyn FnOnce() -> T>>,
}

impl<T: Serialize + DeserializeOwned + Clone + Debug> StorageCell<T> {
    pub fn new(key: impl AsRef<str>, initial_value: impl FnOnce() -> T + 'static) -> Self {
        Self {
            key: key.as_ref().to_string(),
            hot_cache: None,
            initial_value: Some(Box::new(initial_value)),
        }
    }

    pub async fn set(&mut self, storage: &mut SafeStorage, v: T) -> worker::Result<()> {
        self.hot_cache = Some(v.clone());
        storage.put(&self.key, &v).await
    }

    pub async fn update(
        &mut self,
        storage: &mut SafeStorage,
        updater: impl FnOnce(&mut T),
    ) -> worker::Result<()> {
        let mutated_val = if let Some(v) = self.hot_cache.as_mut() {
            v
        } else {
            let stored_val = storage.get(&self.key).await?.unwrap_or_else(|| {
                (self
                    .initial_value
                    .take()
                    .expect("initial value borrow error"))()
            });
            self.hot_cache = Some(stored_val);
            self.hot_cache.as_mut().unwrap()
        };
        updater(mutated_val);

        storage.put(&self.key, mutated_val).await?;

        Ok(())
    }

    pub async fn read(&mut self, storage: &SafeStorage) -> Result<&T> {
        if self.hot_cache.is_some() {
            return Ok(self.hot_cache.as_ref().unwrap());
        }

        let stored_val = storage.get(&self.key).await?.unwrap_or_else(|| {
            (self
                .initial_value
                .take()
                .expect("initial value borrow error"))()
        });
        self.hot_cache = Some(stored_val);

        Ok(self.hot_cache.as_ref().unwrap())
    }
}
