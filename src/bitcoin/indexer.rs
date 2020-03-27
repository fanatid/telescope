use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::future::TryFutureExt as _;
use tokio::sync::{broadcast, Mutex, RwLock};

use super::bitcoind::{json::Block, Bitcoind};
use super::database::IndexerDataBase;
use crate::error::CustomError;
use crate::fixed_hash::H256;
use crate::logger::info;
use crate::shutdown::Shutdown;
use crate::{AnyResult, AppFutFromArgs, EmptyResult};

#[derive(Debug)]
pub struct Indexer {
    shutdown: Arc<Shutdown>,
    db: IndexerDataBase,
    bitcoind: Arc<Bitcoind>,
    status: Arc<RwLock<IndexerStatus>>,
}

impl Indexer {
    pub fn from_args(shutdown: Arc<Shutdown>, args: &clap::ArgMatches<'_>) -> AppFutFromArgs {
        // create indexer
        let indexer = Indexer {
            shutdown,
            db: IndexerDataBase::from_args(args),
            bitcoind: Arc::new(Bitcoind::from_args(args)?),
            status: Arc::new(RwLock::new(IndexerStatus::from_args(args))),
        };

        Ok(Box::pin(async move { indexer.start().await }))
    }

    // Entry function, invoked from returned closure
    async fn start(&self) -> EmptyResult {
        // Try connect first
        self.connect().await?;

        // Initialize status through update before actually start anything.
        let mut status = IndexerStatus::default();
        status.update_node_status(&self.bitcoind).await?;
        self.update_status(status).await;

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

    async fn update_status(&self, status: IndexerStatus) {
        // Read lock not require block other futures, so we use it for comparison
        if *self.status.read().await != status {
            self.status.write().await.merge(status);
        }
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
            self.update_status(status).await;

            // Sleep some time, if required
            let elapsed = ts.elapsed().unwrap();
            if let Some(sleep_duration) = Duration::from_millis(100).checked_sub(elapsed) {
                self.shutdown.delay_for(sleep_duration).await?;
            }
        }
    }

    // Initial or catch-up sync
    async fn start_sync(&self) -> EmptyResult {
        let heights = StartSyncBlockHeightsGenerator::new(&self).await?;

        let bitcoind = Arc::clone(&self.bitcoind);
        let get_block =
            move |height| -> Pin<Box<dyn Future<Output = AnyResult<Option<Block>>> + Send>> {
                let client = Arc::clone(&bitcoind);
                Box::pin(async move { Ok(client.get_block_by_height(height).await?) })
            };

        let blocks = StartSyncBlocksGenerator::new(heights, get_block, 3).await;
        while let Some(block) = blocks.next().await? {
            self.db.push_block(&block).await?;
        }

        Ok(())
    }
}

#[derive(Clone, Default, Debug, PartialEq)]
struct IndexerStatus {
    pub node_syncing_height: u32,
    pub node_syncing_hash: H256,
    pub service_sync_from: u32,
}

impl IndexerStatus {
    pub fn from_args(args: &clap::ArgMatches<'_>) -> IndexerStatus {
        let mut status = IndexerStatus::default();
        status.service_sync_from = args.value_of("sync_from").unwrap().parse().unwrap();
        status
    }

    pub async fn update_node_status(&mut self, bitcoind: &Bitcoind) -> EmptyResult {
        let info = bitcoind.get_blockchain_info().await?;
        self.node_syncing_height = info.blocks;
        self.node_syncing_hash = info.bestblockhash;
        Ok(())
    }

    // pub async fn update_service_status(&mut self, indexer: &Indexer) -> EmptyResult { Ok(()) }

    pub fn merge(&mut self, other: IndexerStatus) {
        macro_rules! update_field {
            ($dest:expr, $src:expr) => {
                if $dest != $src {
                    $dest = $src;
                }
            };
        }

        update_field!(self.node_syncing_height, other.node_syncing_height);
        update_field!(self.node_syncing_hash, other.node_syncing_hash);
        // update_field!(self.service_sync_from, other.service_sync_from);

        info!("Update status to: {:?}", other);
    }
}

// Stream-like, iterator through all required block heights for import.
struct StartSyncBlockHeightsGenerator {
    finished: bool,
    skipped_heights: Vec<u32>,
    status: Arc<RwLock<IndexerStatus>>,
    next_height: u32,
}

impl StartSyncBlockHeightsGenerator {
    pub async fn new(indexer: &Indexer) -> AnyResult<StartSyncBlockHeightsGenerator> {
        // Get next block height from db.
        let db_bestblock_info = indexer.db.get_bestblock_info().await?;
        let db_next_height = db_bestblock_info.map_or_else(|| 0, |(height, _hash)| height + 1);

        // Start height came from args, but stored in status (zero by default).
        let status = Arc::clone(&indexer.status);
        let start_height = status.read().await.service_sync_from;

        // `#created` is for `initial_sync`, everything else for catch-up.
        let initial_sync = indexer.db.get_stage().await.0 == "#created";

        // Get skipped heights from `start_height` (should be smallest height).
        let skipped_heights = if initial_sync {
            indexer.db.get_skipped_block_heights(start_height).await?
        } else {
            vec![]
        };

        Ok(StartSyncBlockHeightsGenerator {
            finished: false,
            skipped_heights,
            status,
            // In case if `start_height` not zero (only for development).
            next_height: std::cmp::max(start_height, db_next_height),
        })
    }

    pub async fn next(&mut self) -> Option<u32> {
        if self.finished {
            return None;
        }

        // Return skipped height first, if have some
        if let Some(height) = self.skipped_heights.pop() {
            return Some(height);
        }

        // Sync up to block depends from `latest` keyword, because sync process
        // can be require a lot of time `end` block should be changed with
        // every new generated block.
        let node_height = self.status.read().await.node_syncing_height;
        let end_height = node_height - 3;

        // Return Some only if `next_height` less or equal `end_height`
        if self.next_height <= end_height {
            let height = self.next_height;
            self.next_height += 1;

            Some(height)
        } else {
            self.finished = true;
            None
        }
    }
}

// Stream-like, iterator through all blocks for import with prefetch.
struct StartSyncBlocksGenerator<T> {
    heights: Mutex<StartSyncBlockHeightsGenerator>,
    #[allow(clippy::type_complexity)]
    get_block: Box<
        dyn Fn(u32) -> Pin<Box<dyn Future<Output = AnyResult<Option<T>>> + Send>>
            + Send
            + Sync
            + 'static,
    >,
    blocks_tx: Mutex<broadcast::Sender<()>>,
    #[allow(clippy::type_complexity)]
    blocks: Mutex<VecDeque<(u32, Option<AnyResult<Option<T>>>)>>,
}

impl<T: Send + 'static> StartSyncBlocksGenerator<T> {
    pub async fn new<F>(
        heights: StartSyncBlockHeightsGenerator,
        get_block: F,
        prefetch_size: u32,
    ) -> Arc<StartSyncBlocksGenerator<T>>
    where
        F: Fn(u32) -> Pin<Box<dyn Future<Output = AnyResult<Option<T>>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        assert!(
            prefetch_size > 0,
            "StartSyncBlocksGenerator not work with zero prefetch size"
        );

        let gen = Arc::new(StartSyncBlocksGenerator {
            heights: Mutex::new(heights),
            get_block: Box::new(get_block),
            blocks_tx: Mutex::new(broadcast::channel(128).0), // 128 should be enough
            blocks: Mutex::new(VecDeque::new()),
        });
        for _ in 0..prefetch_size {
            gen.prefetch().await;
        }
        gen
    }

    async fn prefetch(self: &Arc<StartSyncBlocksGenerator<T>>) {
        let mut blocks = self.blocks.lock().await;
        if let Some(height) = self.heights.lock().await.next().await {
            blocks.push_back((height, None));
            // No needed, because not moved to spawn
            // drop(blocks);

            let self1 = Arc::clone(self);
            tokio::spawn(async move {
                let result = match (self1.get_block)(height).await {
                    Ok(result) => match result {
                        Some(block) => Ok(Some(block)),
                        None => {
                            let msg = format!("No block on start sync for: {}", height);
                            Err(CustomError::new_any(msg))
                        }
                    },
                    Err(e) => Err(e),
                };

                let mut blocks = self1.blocks.lock().await;
                for block in blocks.iter_mut() {
                    if block.0 == height {
                        block.1 = Some(result);
                        let _ = self1.blocks_tx.lock().await.send(());
                        return;
                    }
                }

                unreachable!("No item for block on start sync: {}", height);
            });
        }
    }

    pub async fn next(self: &Arc<StartSyncBlocksGenerator<T>>) -> AnyResult<Option<T>> {
        // Start fetch block for this request. We do not care if block already
        // exists in list, if so this block will be for next function call.
        self.prefetch().await;

        // Subscribe before lock list in loop, otherwise we can be trapped.
        let mut rx = self.blocks_tx.lock().await.subscribe();

        // Try get, wait signal, repeat
        loop {
            let mut blocks = self.blocks.lock().await;

            // If list is empty, not more blocks
            if blocks.is_empty() {
                return Ok(None);
            }

            // Try get block from list
            let mut index = blocks.len();
            for (i, (_height, result)) in blocks.iter().enumerate() {
                if result.is_some() {
                    index = i;
                    break;
                }
            }
            if let Some((_height, result)) = blocks.remove(index) {
                return result.unwrap();
            }

            // drop blocks lock
            drop(blocks);

            // Wait signal and try get block again...
            rx.recv().await.unwrap();
        }
    }
}
