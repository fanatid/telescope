use std::collections::LinkedList;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::future::TryFutureExt as _;
use tokio::sync::{broadcast, Mutex, RwLock};

use super::bitcoind::{json::Block, Bitcoind};
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
    bitcoind: Arc<Bitcoind>,
    status: Arc<RwLock<IndexerStatus>>,
    sync_segment: SyncSegment,
}

impl Indexer {
    pub fn from_args(shutdown: Arc<Shutdown>, args: &clap::ArgMatches<'_>) -> AppFutFromArgs {
        // create indexer
        let indexer = Indexer {
            shutdown,
            db: IndexerDataBase::from_args(args),
            bitcoind: Arc::new(Bitcoind::from_args(args)?),
            status: Arc::new(RwLock::new(IndexerStatus::default())),
            sync_segment: SyncSegment::from_args(args),
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

        let blocks = StartSyncBlocksGenerator::new(heights, get_block, 3);
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
}

impl IndexerStatus {
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

        info!("Update status to: {:?}", other);
    }
}

// Stream-like, iterator through all required block heights for import.
struct StartSyncBlockHeightsGenerator {
    status: Arc<RwLock<IndexerStatus>>,
    sync_segment: SyncSegment,
    skipped_heights: std::vec::IntoIter<u32>,
    next_height: u32,
}

impl StartSyncBlockHeightsGenerator {
    pub async fn new(indexer: &Indexer) -> AnyResult<StartSyncBlockHeightsGenerator> {
        let sync_segment = indexer.sync_segment.clone();

        // `#created` is for `initial_sync`, everything else for catch-up.
        let initial_sync = indexer.db.get_stage().await.0 == "#created";

        // Start height came from args (zero by default).
        let start_height = sync_segment.get_start();

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
            status: Arc::clone(&indexer.status),
            sync_segment,
            skipped_heights: skipped_heights.into_iter(),
            // Everything lower than selected height, but not imported will be in `skipped_heights`.
            next_height: std::cmp::max(start_height, db_next_height),
        })
    }

    pub async fn next(&mut self) -> AnyResult<Option<u32>> {
        // Sync up to block depends from `latest` keyword, because sync process
        // can be require a lot of time `end` block should be changed with
        // every new generated block.
        let node_height = self.status.read().await.node_syncing_height;
        let end_height = self.sync_segment.get_end(node_height) - 3;

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

        // Will continue return `None` while `end_height` will not be increased
        Ok(if self.next_height <= end_height {
            let height = self.next_height;
            self.next_height += 1;

            Some(height)
        } else {
            None
        })
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
    blocks: Mutex<LinkedList<AnyResult<Option<T>>>>,
}

impl<T: Send + 'static> StartSyncBlocksGenerator<T> {
    pub fn new<F>(
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
            blocks: Mutex::new(LinkedList::new()),
        });
        for _ in 0..prefetch_size {
            gen.prefetch();
        }
        gen
    }

    fn prefetch(self: &Arc<StartSyncBlocksGenerator<T>>) {
        let self1 = Arc::clone(self);
        let self2 = Arc::clone(self);
        tokio::spawn(async move {
            // In block we can use operator question mark operator
            let fut = async move {
                match self1.heights.lock().await.next().await? {
                    Some(height) => match (self1.get_block)(height).await? {
                        Some(block) => Ok(Some(block)),
                        None => {
                            let msg = format!("No block on start sync for: {}", height);
                            Err(CustomError::new(msg).into())
                        }
                    },
                    None => Ok(None),
                }
            };

            // Get block, push to list and send notification
            let result = fut.await;
            self2.blocks.lock().await.push_back(result);
            let _ = self2.blocks_tx.lock().await.send(());
        });
    }

    pub async fn next(self: &Arc<StartSyncBlocksGenerator<T>>) -> AnyResult<Option<T>> {
        // Start fetch block for this request. We do not care if block already
        // exists in list, if so this block will be for next function call.
        self.prefetch();

        // Subscribe before lock list in loop, otherwise we can be trapped.
        let mut rx = self.blocks_tx.lock().await.subscribe();

        // Try get, wait signal, repeat
        loop {
            // Try get block from list
            if let Some(block) = self.blocks.lock().await.pop_front() {
                return block;
            }

            // Wait signal and try get block again...
            rx.recv().await.unwrap();
        }
    }
}
