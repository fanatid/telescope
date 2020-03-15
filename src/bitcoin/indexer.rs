use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::TryFutureExt as _;
use tokio::sync::RwLock;

use crate::logger::info;
use super::bitcoind::Bitcoind;
use super::database::IndexerDataBase;
use crate::shutdown::Shutdown;
use crate::{AppFutFromArgs, EmptyResult};

#[derive(Debug)]
pub struct Indexer {
    shutdown: Arc<Shutdown>,
    db: IndexerDataBase,
    bitcoind: Bitcoind,
    status: RwLock<IndexerStatus>,
}

impl Indexer {
    pub fn from_args(shutdown: Arc<Shutdown>, args: &clap::ArgMatches<'_>) -> AppFutFromArgs {
        // create indexer
        let indexer = Indexer {
            shutdown,
            db: IndexerDataBase::from_args(args),
            bitcoind: Bitcoind::from_args(args)?,
            status: RwLock::new(IndexerStatus::default()),
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
            status.node.update(&self.bitcoind).await?;

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
        // TODO
        Ok(())
    }
}

#[derive(Default, Debug, PartialEq)]
struct IndexerStatus {
    pub node: IndexerStatusNode,
    pub service: IndexerStatusService,
}

#[derive(Default, Debug, PartialEq)]
struct IndexerStatusNode {
    pub syncing_height: u32,
    pub syncing_hash: String,
}

impl IndexerStatusNode {
    pub async fn update(&mut self, bitcoind: &Bitcoind) -> EmptyResult {
        let info = bitcoind.getblockchaininfo().await?;
        self.syncing_height = info.blocks;
        self.syncing_hash = info.bestblockhash;
        Ok(())
    }
}

#[derive(Default, Debug, PartialEq)]
struct IndexerStatusService {}
