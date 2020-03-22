use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::TryFutureExt as _;
use tokio::sync::RwLock;

use super::bitcoind::Bitcoind;
use super::database::IndexerDataBase;
use crate::args::SyncSegment;
use crate::fixed_hash::H256;
use crate::logger::info;
use crate::shutdown::Shutdown;
use crate::{AppFutFromArgs, EmptyResult};

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
        // `#created` is for `initial_sync`, everything else for catch-up
        let initial_sync = self.db.get_stage().await.0 == "#created";

        // Start height came from args, zero by default.
        let start_height = self.sync_segment.get_start();

        // Get best block height from db. If block exists in db, it's can not
        // be lower than start height, so we check it.
        let db_bestblock_info = self.db.get_bestblock_info().await?;
        let db_best_height = db_bestblock_info.map_or_else(|| 0, |(height, _)| height + 1);
        if db_bestblock_info.is_some() && db_best_height < start_height {
            panic!(
                "Best height from database ({}) can not be lower than start height: {}",
                db_best_height, start_height
            );
        }
        let zero_blocks = RefCell::new(db_bestblock_info.is_none());

        // Get skipped heights from lowest height (which is height from args).
        let skipped_heights = if initial_sync {
            self.db.get_skipped_block_heights(start_height).await?
        } else {
            vec![]
        };
        let skipped_heights = RefCell::new(skipped_heights.into_iter());

        // Get best height. Everything lower up to selected start height from
        // args will be in `skipped_heights` if required.
        let best_height = RefCell::new(std::cmp::max(start_height, db_best_height));

        // Sync up to block depends from `latest` keyword, because sync process
        // can be require a lot of time `end` block should be changed with
        // every new generated block.
        let get_end_height = || async {
            let status = self.status.read().await;
            self.sync_segment.get_end(status.node_syncing_height) - 3
        };

        // Iterator-like, return Option<u32>
        // Have no idea, how handle next error, so RefCell.
        // "returns a reference to a captured variable which escapes the closure body"
        let get_next_block_height = || async {
            let end_height = get_end_height().await;

            if let Some(height) = skipped_heights.borrow_mut().next() {
                if height <= end_height {
                    return Some(height);
                } else {
                    panic!(
                        "Skipped height in database ({}) can not be higher than end height: {}",
                        height, end_height
                    );
                };
            }

            // If we have zero blocks, we return height 0.
            // Otherwise, add 1 to current best height and return result.
            let best_height = if *zero_blocks.borrow() {
                *zero_blocks.borrow_mut() = false;
                0
            } else {
                *best_height.borrow_mut() += 1;
                *best_height.borrow()
            };

            // Like in iter, None if no more results
            if best_height <= end_height {
                Some(best_height)
            } else {
                None
            }
        };

        loop {
            let height = get_next_block_height().await;
            if height.is_none() {
                return Ok(());
            }

            let height = height.unwrap();
            match self.bitcoind.get_block_by_height(height).await? {
                Some(block) => self.db.push_block(&block).await?,
                None => panic!("No block on start sync for: {}", height),
            };
        }
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
