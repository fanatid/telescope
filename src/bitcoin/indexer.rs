use std::sync::Arc;
use std::time::Duration;

use futures::TryFutureExt as _;
use tokio::time::delay_for;

use super::bitcoind::Bitcoind;
use super::database::IndexerDataBase;
use crate::logger::info;
use crate::shutdown::Shutdown;
use crate::{AnyError, AppFutFromArgs};

#[derive(Debug)]
pub struct Indexer {
    shutdown: Arc<Shutdown>,
    db: IndexerDataBase,
    bitcoind: Bitcoind,
}

impl Indexer {
    pub fn from_args(shutdown: Arc<Shutdown>, args: &clap::ArgMatches<'_>) -> AppFutFromArgs {
        // create indexer
        let mut indexer = Indexer {
            shutdown,
            db: IndexerDataBase::from_args(args),
            bitcoind: Bitcoind::from_args(args)?,
        };

        Ok(Box::pin(async move {
            // connect first
            indexer.connect().await?;

            //
            indexer.start().await
        }))
    }

    async fn connect(&self) -> AnyError<()> {
        tokio::try_join!(
            self.db.validate(&self.shutdown),
            self.bitcoind.validate(&self.shutdown).map_err(|e| e.into()),
        )?;
        Ok(())
    }

    async fn start(&mut self) -> AnyError<()> {
        loop {
            self.shutdown.is_recv().await?;

            let info = self.bitcoind.getblockchaininfo().await?;
            info!("{}", info.bestblockhash);

            tokio::select! {
                _ = delay_for(Duration::from_secs(1)) => {},
                e = self.shutdown.wait() => return Err(e.into()),
            }
        }
    }
}
