// Copyright 2021 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fmt::Display;
use std::marker::PhantomData;
use std::ops::Deref;
use std::ops::RangeBounds;

use common_exception::WithContext;
use common_meta_stoerr::MetaStorageError;
use common_meta_types::anyerror::AnyError;
use sled::transaction::ConflictableTransactionError;
use sled::transaction::TransactionResult;
use sled::transaction::TransactionalTree;
use sled::IVec;
use tracing::debug;
use tracing::warn;

use crate::sled::transaction::TransactionError;
use crate::store::Store;
use crate::SledBytesError;
use crate::SledKeySpace;

/// Get a ref to the key or to the value.
///
/// It is used as an abstract representation of key/value used in the sled store.
pub trait SledAsRef<K, V> {
    fn as_key(&self) -> &K;
    fn as_value(&self) -> &V;
}

impl<K, V> SledAsRef<K, V> for (K, V) {
    fn as_key(&self) -> &K {
        &self.0
    }

    fn as_value(&self) -> &V {
        &self.1
    }
}

/// SledTree is a wrapper of sled::Tree that provides access of more than one key-value
/// types.
/// A `SledKVType` defines a key-value type to be stored.
/// The key type `K` must be serializable with order preserved, i.e. impl trait `SledOrderedSerde`.
/// The value type `V` can be any serialize impl, i.e. for most cases, to impl trait `SledSerde`.
#[derive(Debug, Clone)]
pub struct SledTree {
    pub name: String,

    /// Whether to fsync after an write operation.
    /// With sync==false, it WONT fsync even when user tell it to sync.
    /// This is only used for testing when fsync is quite slow.
    /// E.g. File::sync_all takes 10 ~ 30 ms on a Mac.
    /// See: https://github.com/drmingdrmer/sledtest/blob/500929ab0b89afe547143a38fde6fe85d88f1f80/src/ben_sync.rs
    sync: bool,

    pub tree: sled::Tree,
}

/// A key-value item stored in sled store.
pub struct SledItem<KV: SledKeySpace> {
    key: IVec,
    value: IVec,
    _p: PhantomData<KV>,
}

impl<KV: SledKeySpace> SledItem<KV> {
    pub fn new(key: IVec, value: IVec) -> Self {
        //
        Self {
            key,
            value,
            _p: Default::default(),
        }
    }

    pub fn key(&self) -> Result<KV::K, SledBytesError> {
        KV::deserialize_key(&self.key)
    }

    pub fn value(&self) -> Result<KV::V, SledBytesError> {
        KV::deserialize_value(&self.value)
    }

    pub fn kv(&self) -> Result<(KV::K, KV::V), SledBytesError> {
        let k = self.key()?;
        let v = self.value()?;
        Ok((k, v))
    }
}

#[allow(clippy::type_complexity)]
impl SledTree {
    /// Open SledTree
    pub fn open<N: AsRef<[u8]> + Display>(
        db: &sled::Db,
        tree_name: N,
        sync: bool,
    ) -> Result<Self, MetaStorageError> {
        // During testing, every tree name must be unique.
        if cfg!(test) {
            let x = tree_name.as_ref();
            let x = &x[0..5];
            assert_eq!(x, b"test-");
        }
        let t = db
            .open_tree(&tree_name)
            .context(|| format!("open tree: {}", tree_name))?;

        debug!("SledTree opened tree: {}", tree_name);

        let rl = SledTree {
            name: tree_name.to_string(),
            sync,
            tree: t,
        };
        Ok(rl)
    }

    /// Borrows the SledTree and creates a wrapper with access limited to a specified key space `KV`.
    pub fn key_space<KV: SledKeySpace>(&self) -> AsKeySpace<KV> {
        AsKeySpace::<KV> {
            inner: self,
            phantom: PhantomData,
        }
    }

    /// Returns a vec of key-value paris:
    pub fn export(&self) -> Result<Vec<Vec<Vec<u8>>>, std::io::Error> {
        let it = self.tree.iter();

        let mut kvs = Vec::new();

        for rkv in it {
            let (k, v) = rkv?;

            kvs.push(vec![k.to_vec(), v.to_vec()]);
        }

        Ok(kvs)
    }

    pub fn txn<T>(
        &self,
        sync: bool,
        f: impl Fn(TransactionSledTree<'_>) -> Result<T, MetaStorageError>,
    ) -> Result<T, MetaStorageError> {
        let sync = sync && self.sync;

        let result: TransactionResult<T, MetaStorageError> = self.tree.transaction(move |tree| {
            let txn_sled_tree = TransactionSledTree { txn_tree: tree };
            let r = f(txn_sled_tree.clone());
            match r {
                Ok(r) => {
                    if sync {
                        txn_sled_tree.txn_tree.flush();
                    }
                    Ok(r)
                }
                Err(meta_sto_err) => {
                    warn!("txn error: {:?}", meta_sto_err);

                    match &meta_sto_err {
                        MetaStorageError::BytesError(_e) => {
                            Err(ConflictableTransactionError::Abort(meta_sto_err))
                        }
                        MetaStorageError::SledError(_e) => {
                            Err(ConflictableTransactionError::Abort(meta_sto_err))
                        }
                        MetaStorageError::TransactionConflict => {
                            Err(ConflictableTransactionError::Conflict)
                        }
                        MetaStorageError::SnapshotError(_e) => {
                            Err(ConflictableTransactionError::Abort(meta_sto_err))
                        }
                    }
                }
            }
        });

        match result {
            Ok(x) => Ok(x),
            Err(txn_err) => match txn_err {
                TransactionError::Abort(meta_sto_err) => Err(meta_sto_err),
                TransactionError::Storage(sto_err) => {
                    Err(MetaStorageError::SledError(AnyError::new(&sto_err)))
                }
            },
        }
    }

    /// Retrieve the value of key.
    pub(crate) fn get<KV: SledKeySpace>(
        &self,
        key: &KV::K,
    ) -> Result<Option<KV::V>, MetaStorageError> {
        let got = self
            .tree
            .get(KV::serialize_key(key)?)
            .context(|| format!("get: {}:{}", self.name, key))?;

        let v = match got {
            None => None,
            Some(v) => Some(KV::deserialize_value(v)?),
        };

        Ok(v)
    }

    /// Delete kvs that are in `range`.
    #[tracing::instrument(level = "debug", skip(self, range))]
    pub(crate) async fn range_remove<KV, R>(
        &self,
        range: R,
        flush: bool,
    ) -> Result<(), MetaStorageError>
    where
        KV: SledKeySpace,
        R: RangeBounds<KV::K>,
    {
        let mut batch = sled::Batch::default();

        // Convert K range into sled::IVec range
        let sled_range = KV::serialize_range(&range)?;

        let range_mes = self.range_message::<KV, _>(&range);

        for item in self.tree.range(sled_range) {
            let (k, _) = item.context(|| format!("range_remove: {}", range_mes,))?;
            batch.remove(k);
        }

        self.tree
            .apply_batch(batch)
            .context(|| format!("batch remove: {}", range_mes,))?;

        self.flush_async(flush).await?;

        Ok(())
    }

    /// Get key-values in `range`
    pub(crate) fn range<KV, R>(
        &self,
        range: R,
    ) -> Result<
        impl DoubleEndedIterator<Item = Result<SledItem<KV>, MetaStorageError>>,
        MetaStorageError,
    >
    where
        KV: SledKeySpace,
        R: RangeBounds<KV::K>,
    {
        let range_mes = self.range_message::<KV, _>(&range);

        // Convert K range into sled::IVec range
        let range = KV::serialize_range(&range)?;

        let it = self.tree.range(range);
        let it = it.map(move |item| {
            let (k, v) = item.context(|| format!("range_get: {}", range_mes,))?;

            let item = SledItem::new(k, v);
            Ok(item)
        });

        Ok(it)
    }

    /// Get key-values in with the same prefix
    pub(crate) fn scan_prefix<KV>(
        &self,
        prefix: &KV::K,
    ) -> Result<Vec<(KV::K, KV::V)>, MetaStorageError>
    where
        KV: SledKeySpace,
    {
        let mut res = vec![];

        let mes = || format!("scan_prefix: {}", prefix);

        let pref = KV::serialize_key(prefix)?;
        for item in self.tree.scan_prefix(pref) {
            let (k, v) = item.context(mes)?;

            let key = KV::deserialize_key(k)?;
            let value = KV::deserialize_value(v)?;
            res.push((key, value));
        }

        Ok(res)
    }

    /// Append many key-values into SledTree.
    pub(crate) async fn append<KV, T>(&self, kvs: &[T]) -> Result<(), MetaStorageError>
    where
        KV: SledKeySpace,
        T: SledAsRef<KV::K, KV::V>,
    {
        let mut batch = sled::Batch::default();

        for t in kvs.iter() {
            let key = t.as_key();
            let value = t.as_value();

            let k = KV::serialize_key(key)?;
            let v = KV::serialize_value(value)?;

            batch.insert(k, v);
        }

        self.tree.apply_batch(batch).context(|| "batch append")?;

        self.flush_async(true).await?;

        Ok(())
    }

    /// Insert a single kv.
    /// Returns the last value if it is set.
    pub(crate) async fn insert<KV>(
        &self,
        key: &KV::K,
        value: &KV::V,
    ) -> Result<Option<KV::V>, MetaStorageError>
    where
        KV: SledKeySpace,
    {
        let k = KV::serialize_key(key)?;
        let v = KV::serialize_value(value)?;

        let prev = self
            .tree
            .insert(k, v)
            .context(|| format!("insert_value {}", key))?;

        let prev = match prev {
            None => None,
            Some(x) => Some(KV::deserialize_value(x)?),
        };

        self.flush_async(true).await?;

        Ok(prev)
    }

    /// Build a string describing the range for a range operation.
    fn range_message<KV, R>(&self, range: &R) -> String
    where
        KV: SledKeySpace,
        R: RangeBounds<KV::K>,
    {
        format!(
            "{}:{}/[{:?}, {:?}]",
            self.name,
            KV::NAME,
            range.start_bound(),
            range.end_bound()
        )
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn flush_async(&self, flush: bool) -> Result<(), MetaStorageError> {
        if flush && self.sync {
            self.tree
                .flush_async()
                .await
                .context(|| "flush sled-tree")?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct TransactionSledTree<'a> {
    pub txn_tree: &'a TransactionalTree,
}

impl TransactionSledTree<'_> {
    pub fn key_space<KV: SledKeySpace>(&self) -> AsTxnKeySpace<KV> {
        AsTxnKeySpace::<KV> {
            inner: self,
            phantom: PhantomData,
        }
    }
}

/// It borrows the internal SledTree with access limited to a specified namespace `KV`.
pub struct AsKeySpace<'a, KV: SledKeySpace> {
    inner: &'a SledTree,
    phantom: PhantomData<KV>,
}

pub struct AsTxnKeySpace<'a, KV: SledKeySpace> {
    inner: &'a TransactionSledTree<'a>,
    phantom: PhantomData<KV>,
}

impl<'a, KV: SledKeySpace> Store<KV> for AsTxnKeySpace<'a, KV> {
    type Error = MetaStorageError;

    fn insert(&self, key: &KV::K, value: &KV::V) -> Result<Option<KV::V>, Self::Error> {
        let k = KV::serialize_key(key)?;
        let v = KV::serialize_value(value)?;

        let prev = self.txn_tree.insert(k, v)?;
        match prev {
            Some(v) => Ok(Some(KV::deserialize_value(v)?)),
            None => Ok(None),
        }
    }

    fn get(&self, key: &KV::K) -> Result<Option<KV::V>, Self::Error> {
        let k = KV::serialize_key(key)?;
        let got = self.txn_tree.get(k)?;

        match got {
            Some(v) => Ok(Some(KV::deserialize_value(v)?)),
            None => Ok(None),
        }
    }

    fn remove(&self, key: &KV::K) -> Result<Option<KV::V>, Self::Error> {
        let k = KV::serialize_key(key)?;
        let removed = self.txn_tree.remove(k)?;

        match removed {
            Some(v) => Ok(Some(KV::deserialize_value(v)?)),
            None => Ok(None),
        }
    }

    fn update_and_fetch<F>(&self, key: &KV::K, mut f: F) -> Result<Option<KV::V>, Self::Error>
    where F: FnMut(Option<KV::V>) -> Option<KV::V> {
        let key_ivec = KV::serialize_key(key)?;

        let old_val_ivec = self.txn_tree.get(&key_ivec)?;
        let old_val: Result<Option<KV::V>, MetaStorageError> = match old_val_ivec {
            Some(v) => Ok(Some(KV::deserialize_value(v)?)),
            None => Ok(None),
        };

        let old_val = old_val?;

        let new_val = f(old_val);
        let _ = match new_val {
            Some(ref v) => self.txn_tree.insert(key_ivec, KV::serialize_value(v)?)?,
            None => self.txn_tree.remove(key_ivec)?,
        };

        Ok(new_val)
    }
}

/// Some methods that take `&TransactionSledTree` as parameter need to be called
/// in subTree method, since subTree(aka: AsTxnKeySpace) already ref to `TransactionSledTree`
/// we impl deref here to fetch inner `&TransactionSledTree`.
/// # Example:
///
/// ```
/// fn txn_incr_seq(&self, key: &str, txn_tree: &TransactionSledTree) {}
///
/// fn sub_txn_tree_do_update<'s, KS>(&'s self, sub_tree: &AsTxnKeySpace<'s, KS>) {
///     seq_kv_value.seq = self.txn_incr_seq(KS::NAME, &*sub_tree);
///     sub_tree.insert(key, &seq_kv_value);
/// }
/// ```
impl<'a, KV: SledKeySpace> Deref for AsTxnKeySpace<'a, KV> {
    type Target = &'a TransactionSledTree<'a>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[allow(clippy::type_complexity)]
impl<'a, KV: SledKeySpace> AsKeySpace<'a, KV> {
    pub fn get(&self, key: &KV::K) -> Result<Option<KV::V>, MetaStorageError> {
        self.inner.get::<KV>(key)
    }

    pub fn last(&self) -> Result<Option<(KV::K, KV::V)>, MetaStorageError> {
        let mut it = self.inner.range::<KV, _>(..)?.rev();
        let last = it.next();
        let last = match last {
            None => return Ok(None),
            Some(x) => x,
        };

        let kv = last?.kv()?;
        Ok(Some(kv))
    }

    pub async fn range_remove<R>(&self, range: R, flush: bool) -> Result<(), MetaStorageError>
    where R: RangeBounds<KV::K> {
        self.inner.range_remove::<KV, R>(range, flush).await
    }

    pub fn clear(&self) -> Result<(), MetaStorageError> {
        let err = self.inner.tree.clear();
        match err {
            Err(err) => Err(MetaStorageError::SledError(AnyError::new(&err))),
            Ok(()) => Ok(()),
        }
    }

    pub fn range<R>(
        &self,
        range: R,
    ) -> Result<
        impl DoubleEndedIterator<Item = Result<SledItem<KV>, MetaStorageError>>,
        MetaStorageError,
    >
    where
        R: RangeBounds<KV::K>,
    {
        self.inner.range::<KV, R>(range)
    }

    pub fn scan_prefix(&self, prefix: &KV::K) -> Result<Vec<(KV::K, KV::V)>, MetaStorageError> {
        self.inner.scan_prefix::<KV>(prefix)
    }

    pub fn range_values<R>(&self, range: R) -> Result<Vec<KV::V>, MetaStorageError>
    where R: RangeBounds<KV::K> {
        let it = self.inner.range::<KV, R>(range)?;
        let mut res = vec![];
        for r in it {
            let item = r?;
            let v = item.value()?;
            res.push(v);
        }

        Ok(res)
    }

    pub async fn append<T>(&self, kvs: &[T]) -> Result<(), MetaStorageError>
    where T: SledAsRef<KV::K, KV::V> {
        self.inner.append::<KV, _>(kvs).await
    }

    pub async fn insert(
        &self,
        key: &KV::K,
        value: &KV::V,
    ) -> Result<Option<KV::V>, MetaStorageError> {
        self.inner.insert::<KV>(key, value).await
    }
}
