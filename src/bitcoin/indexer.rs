use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::future::TryFutureExt as _;
use tokio::sync::RwLock;

use super::bitcoind::Bitcoind;
use super::database::IndexerDataBase;
use crate::args::SyncSegment;
use crate::error::CustomError;
use crate::fixed_hash::H256;
use crate::logger::info;
use crate::shutdown::Shutdown;
use crate::{AnyResult, AppFutFromArgs, EmptyResult};

#[derive(Debug)]
pub struct Indexer {
    shutdown: Arc<Shutdown>,
    db: IndexerDataBase,
    bitcoind: Bitcoind,
    status: RwLock<IndexerStatus>,
    sync_segment: SyncSegment,
}

impl Indexer {
    pub fn from_args(shutdown: Arc<Shutdown>, args: &clap::ArgMatches<'_>) -> AppFutFromArgs {
        // create indexer
        let indexer = Indexer {
            shutdown,
            db: IndexerDataBase::from_args(args),
            bitcoind: Bitcoind::from_args(args)?,
            status: RwLock::new(IndexerStatus::default()),
            sync_segment: SyncSegment::from_args(args),
        };

        Ok(Box::pin(async move { indexer.start().await }))
    }

    // Entry function, invoked from returned closure
    async fn start(&self) -> EmptyResult {
        // Try connect first
        self.connect().await?;

        // TODO: implement status updated with logging (and notifications in future)
        // Update status before actually start anything
        let mut status = self.status.write().await;
        status.update_node_status(&self.bitcoind).await?;
        drop(status); // Drop RwLockWriteGuard

        // Run sync loops
        tokio::try_join!(self.start_status_update_loop(), self.start_sync(),)?;
        Ok(())
    }

    // Indexer is component between `bitcoind` and `postgresql`,
    // so we try to connect to these components
    async fn connect(&self) -> EmptyResult {
        tokio::try_join!(
            self.db.validate(&self.shutdown),
            self.bitcoind.validate(&self.shutdown).map_err(|e| e.into()),
        )?;
        Ok(())
    }

    // Update bitcoind and service info in loop
    async fn start_status_update_loop(&self) -> EmptyResult {
        loop {
            // Check that loop should continue
            self.shutdown.is_recv().await?;
            let ts = SystemTime::now();

            // Create new status
            let mut status = IndexerStatus::default();
            status.update_node_status(&self.bitcoind).await?;

            // Read lock not block other futures, so we use it for comparison
            let status_self = self.status.read().await;
            if *status_self != status {
                info!("Update status to: {:?}", status);
                drop(status_self); // Drop RwLockWriteGuard because otherwise we will create deadlock
                *self.status.write().await = status;
            }

            // Sleep some time, if required
            let elapsed = ts.elapsed().unwrap();
            if let Some(sleep_duration) = Duration::from_millis(100).checked_sub(elapsed) {
                self.shutdown.delay_for(sleep_duration).await?;
            }
        }
    }

    // Initial or catch-up sync
    async fn start_sync(&self) -> EmptyResult {
        let mut heights = StartSyncBlockHeightsGenerator::from_indexer(&self).await?;
        while let Some(height) = heights.next().await? {
            match self.bitcoind.get_block_by_height(height).await? {
                Some(block) => self.db.push_block(&block).await?,
                None => panic!("No block on start sync for: {}", height),
            };
        }

        Ok(())
    }
}

#[derive(Default, Debug, PartialEq)]
struct IndexerStatus {
    pub node_syncing_height: u32,
    pub node_syncing_hash: H256,
}

impl IndexerStatus {
    pub async fn update_node_status(&mut self, bitcoind: &Bitcoind) -> EmptyResult {
        let info = bitcoind.get_blockchain_info().await?;
        self.node_syncing_height = info.blocks;
        self.node_syncing_hash = info.bestblockhash;
        Ok(())
    }
}

// Stream-like, iterator through all required block heights for import.
struct StartSyncBlockHeightsGenerator<'a> {
    indexer: &'a Indexer,
    skipped_heights: std::vec::IntoIter<u32>,
    next_height: u32,
}

impl<'a> StartSyncBlockHeightsGenerator<'a> {
    pub async fn from_indexer(
        indexer: &'a Indexer,
    ) -> AnyResult<StartSyncBlockHeightsGenerator<'a>> {
        // `#created` is for `initial_sync`, everything else for catch-up.
        let initial_sync = indexer.db.get_stage().await.0 == "#created";

        // Start height came from args (zero by default).
        let start_height = indexer.sync_segment.get_start();

        // Get best block height from db. If block exists in db, it's can not
        // be lower than start height, so we check it.
        let db_bestblock_info = indexer.db.get_bestblock_info().await?;
        let db_next_height = db_bestblock_info.map_or_else(|| 0, |(height, _hash)| height + 1);
        if db_bestblock_info.is_some() && db_next_height < start_height {
            return Err(CustomError::new(format!(
                "Next height ({}) based on best height from database can not be lower than start height: {}",
                db_next_height, start_height,
            )));
        }

        // Get skipped heights from lowest height (which is height from args).
        let skipped_heights = if initial_sync {
            indexer.db.get_skipped_block_heights(start_height).await?
        } else {
            vec![]
        };

        Ok(StartSyncBlockHeightsGenerator {
            indexer,
            skipped_heights: skipped_heights.into_iter(),
            // Everything lower than selected height, but not imported will be in `skipped_heights`.
            next_height: std::cmp::max(start_height, db_next_height),
        })
    }

    pub async fn next(&mut self) -> AnyResult<Option<u32>> {
        // Sync up to block depends from `latest` keyword, because sync process
        // can be require a lot of time `end` block should be changed with
        // every new generated block.
        let node_height = self.indexer.status.read().await.node_syncing_height;
        let end_height = self.indexer.sync_segment.get_end(node_height) - 3;

        if let Some(height) = self.skipped_heights.next() {
            if height <= end_height {
                return Ok(Some(height));
            } else {
                return Err(CustomError::new(format!(
                    "Skipped height in database ({}) can not be higher than end height: {}",
                    height, end_height
                )));
            };
        }

        let height = self.next_height;
        self.next_height += 1;

        Ok(if height <= end_height {
            Some(height)
        } else {
            None
        })
    }
}
