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

use std::sync::Arc;
use std::sync::Mutex;

use common_base::base::tokio;
use common_meta_raft_store::state_machine::testing::pretty_snapshot;
use common_meta_raft_store::state_machine::testing::snapshot_logs;
use common_meta_raft_store::state_machine::SerializableSnapshot;
use common_meta_sled_store::openraft::async_trait::async_trait;
use common_meta_sled_store::openraft::raft::Entry;
use common_meta_sled_store::openraft::raft::EntryPayload;
use common_meta_sled_store::openraft::storage::HardState;
use common_meta_sled_store::openraft::testing::StoreBuilder;
use common_meta_sled_store::openraft::EffectiveMembership;
use common_meta_sled_store::openraft::LogId;
use common_meta_sled_store::openraft::Membership;
use common_meta_sled_store::openraft::RaftStorage;
use common_meta_sled_store::openraft::StorageHelper;
use common_meta_types::AppliedState;
use common_meta_types::LogEntry;
use databend_meta::store::RaftStoreBare;
use databend_meta::Opened;
use maplit::btreeset;
use tracing::debug;
use tracing::info;

use crate::init_meta_ut;
use crate::tests::service::MetaSrvTestContext;

struct MetaStoreBuilder {
    pub test_contexts: Arc<Mutex<Vec<MetaSrvTestContext>>>,
}

#[async_trait]
impl StoreBuilder<LogEntry, AppliedState, RaftStoreBare> for MetaStoreBuilder {
    async fn build(&self) -> RaftStoreBare {
        let tc = MetaSrvTestContext::new(555);
        let ms = RaftStoreBare::open_create(&tc.config.raft_config, None, Some(()))
            .await
            .expect("fail to create store");

        {
            let mut tcs = self.test_contexts.lock().unwrap();
            tcs.push(tc);
        }
        ms
    }
}

#[test]
fn test_impl_raft_storage() -> anyhow::Result<()> {
    let (_log_guards, ut_span) = init_meta_ut!();
    let _ent = ut_span.enter();

    common_meta_sled_store::openraft::testing::Suite::test_all(MetaStoreBuilder {
        test_contexts: Arc::new(Mutex::new(vec![])),
    })?;

    Ok(())
}

#[async_entry::test(worker_threads = 3, init = "init_meta_ut!()", tracing_span = "debug")]
async fn test_meta_store_restart() -> anyhow::Result<()> {
    // - Create a meta store
    // - Update meta store
    // - Close and reopen it
    // - Test state is restored: hard state, log, state machine

    let id = 3;
    let tc = MetaSrvTestContext::new(id);

    info!("--- new meta store");
    {
        let sto = RaftStoreBare::open_create(&tc.config.raft_config, None, Some(())).await?;
        assert_eq!(id, sto.id);
        assert!(!sto.is_opened());
        assert_eq!(None, sto.read_hard_state().await?);

        info!("--- update metasrv");

        sto.save_hard_state(&HardState {
            current_term: 10,
            voted_for: Some(5),
        })
        .await?;

        sto.append_to_log(&[&Entry {
            log_id: LogId::new(1, 1),
            payload: EntryPayload::Blank,
        }])
        .await?;

        sto.apply_to_state_machine(&[&Entry {
            log_id: LogId::new(1, 2),
            payload: EntryPayload::Blank,
        }])
        .await?;
    }

    info!("--- reopen meta store");
    {
        let sto = RaftStoreBare::open_create(&tc.config.raft_config, Some(()), None).await?;
        assert_eq!(id, sto.id);
        assert!(sto.is_opened());
        assert_eq!(
            Some(HardState {
                current_term: 10,
                voted_for: Some(5),
            }),
            sto.read_hard_state().await?
        );

        assert_eq!(
            LogId::new(1, 1),
            StorageHelper::new(&sto).get_log_id(1).await?
        );
        assert_eq!(Some(LogId::new(1, 2)), sto.last_applied_state().await?.0);
    }
    Ok(())
}

#[async_entry::test(worker_threads = 3, init = "init_meta_ut!()", tracing_span = "debug")]
async fn test_meta_store_build_snapshot() -> anyhow::Result<()> {
    // - Create a metasrv
    // - Apply logs
    // - Create a snapshot check snapshot state

    let id = 3;
    let tc = MetaSrvTestContext::new(id);

    let sto = RaftStoreBare::open_create(&tc.config.raft_config, None, Some(())).await?;

    info!("--- feed logs and state machine");

    let (logs, want) = snapshot_logs();

    sto.log.append(&logs).await?;
    for l in logs.iter() {
        sto.state_machine.write().await.apply(l).await?;
    }

    let curr_snap = sto.build_snapshot().await?;
    assert_eq!(
        Some(LogId { term: 1, index: 9 }),
        curr_snap.meta.last_log_id
    );

    info!("--- check snapshot");
    {
        let data = curr_snap.snapshot.into_inner();

        let ser_snap: SerializableSnapshot = serde_json::from_slice(&data)?;
        let res = pretty_snapshot(&ser_snap.kvs);
        debug!("res: {:?}", res);

        assert_eq!(want, res);
    }

    Ok(())
}

#[async_entry::test(worker_threads = 3, init = "init_meta_ut!()", tracing_span = "debug")]
async fn test_meta_store_current_snapshot() -> anyhow::Result<()> {
    // - Create a metasrv
    // - Apply logs
    // - Create a snapshot check snapshot state

    let id = 3;
    let tc = MetaSrvTestContext::new(id);

    let sto = RaftStoreBare::open_create(&tc.config.raft_config, None, Some(())).await?;

    info!("--- feed logs and state machine");

    let (logs, want) = snapshot_logs();

    sto.log.append(&logs).await?;
    for l in logs.iter() {
        sto.state_machine.write().await.apply(l).await?;
    }

    sto.build_snapshot().await?;

    info!("--- check get_current_snapshot");

    let curr_snap = sto.get_current_snapshot().await?.unwrap();
    assert_eq!(
        Some(LogId { term: 1, index: 9 }),
        curr_snap.meta.last_log_id
    );

    info!("--- check snapshot");
    {
        let data = curr_snap.snapshot.into_inner();

        let ser_snap: SerializableSnapshot = serde_json::from_slice(&data)?;
        let res = pretty_snapshot(&ser_snap.kvs);
        debug!("res: {:?}", res);

        assert_eq!(want, res);
    }

    Ok(())
}

#[async_entry::test(worker_threads = 3, init = "init_meta_ut!()", tracing_span = "debug")]
async fn test_meta_store_install_snapshot() -> anyhow::Result<()> {
    // - Create a metasrv
    // - Feed logs
    // - Create a snapshot
    // - Create a new metasrv and restore it by install the snapshot

    let (logs, want) = snapshot_logs();

    let id = 3;
    let snap;
    {
        let tc = MetaSrvTestContext::new(id);

        let sto = RaftStoreBare::open_create(&tc.config.raft_config, None, Some(())).await?;

        info!("--- feed logs and state machine");

        sto.log.append(&logs).await?;
        for l in logs.iter() {
            sto.state_machine.write().await.apply(l).await?;
        }
        snap = sto.build_snapshot().await?;
    }

    let data = snap.snapshot.into_inner();

    info!("--- reopen a new metasrv to install snapshot");
    {
        let tc = MetaSrvTestContext::new(id);

        let sto = RaftStoreBare::open_create(&tc.config.raft_config, None, Some(())).await?;

        info!("--- rejected because old sm is not cleaned");
        {
            sto.raft_state.write_state_machine_id(&(1, 2)).await?;
            let res = sto.install_snapshot(&data).await;
            assert!(res.is_err(), "different ids disallow installing snapshot");
            assert!(
                res.unwrap_err()
                    .to_string()
                    .starts_with("another snapshot install is not finished yet: 1 2")
            );
        }

        info!("--- install snapshot");
        {
            sto.raft_state.write_state_machine_id(&(0, 0)).await?;
            sto.install_snapshot(&data).await?;
        }

        info!("--- check installed meta");
        {
            assert_eq!((1, 1), sto.raft_state.read_state_machine_id()?);

            let mem = sto.state_machine.write().await.get_membership()?;
            assert_eq!(
                Some(EffectiveMembership {
                    log_id: LogId::new(1, 5),
                    membership: Membership::new_single(btreeset! {4,5,6})
                }),
                mem
            );

            let last_applied = sto.state_machine.write().await.get_last_applied()?;
            assert_eq!(Some(LogId::new(1, 9)), last_applied);
        }

        info!("--- check snapshot");
        {
            let curr_snap = sto.build_snapshot().await?;
            let data = curr_snap.snapshot.into_inner();

            let ser_snap: SerializableSnapshot = serde_json::from_slice(&data)?;
            let res = pretty_snapshot(&ser_snap.kvs);
            debug!("res: {:?}", res);

            assert_eq!(want, res);
        }
    }

    Ok(())
}
