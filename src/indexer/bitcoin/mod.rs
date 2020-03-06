use std::sync::Arc;
use std::time::Duration;

use futures::TryFutureExt as _;
use tokio::time::delay_for;

use crate::db::DB;
use crate::shutdown::Shutdown;
use crate::AnyError;
use bitcoind::Bitcoind;

mod bitcoind;

#[derive(Debug)]
pub struct Indexer {
    shutdown: Arc<Shutdown>,
    db: DB,
    bitcoind: Bitcoind,
}

impl Indexer {
    pub async fn from_args(shutdown: Arc<Shutdown>, args: &clap::ArgMatches<'_>) -> AnyError<()> {
        // create indexer
        let mut indexer = Indexer {
            shutdown,
            db: DB::from_args(args),
            bitcoind: Bitcoind::from_args(args)?,
        };

        // connect first
        indexer.connect().await?;

        //
        indexer.start().await
    }

    async fn connect(&self) -> AnyError<()> {
        tokio::try_join!(
            self.db.connect(&self.shutdown),
            self.bitcoind.validate(&self.shutdown).map_err(|e| e.into()),
        )?;
        Ok(())
    }

    async fn start(&mut self) -> AnyError<()> {
        loop {
            self.shutdown.is_recv().await?;

            let info = self.bitcoind.getblockchaininfo().await?;
            log::info!("{}", info.bestblockhash);

            tokio::select! {
                _ = delay_for(Duration::from_secs(1)) => {},
                e = self.shutdown.wait() => return Err(e.into()),
            }
        }
    }
}
