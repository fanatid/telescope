use crate::db::DB;
use crate::shutdown::Shutdown;
use crate::AnyError;
use bitcoind::Bitcoind;

mod bitcoind;

#[derive(Debug)]
pub struct Indexer {
    shutdown: Shutdown,
    db: DB,
    bitcoind: Bitcoind,
}

impl Indexer {
    pub async fn from_args(shutdown: Shutdown, args: &clap::ArgMatches<'_>) -> AnyError<()> {
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

    async fn connect(&mut self) -> AnyError<()> {
        self.db.connect(&mut self.shutdown).await?;
        self.bitcoind.validate(&mut self.shutdown).await?;
        Ok(())
    }

    async fn start(&mut self) -> AnyError<()> {
        loop {
            if self.shutdown.is_recv() {
                break;
            }

            let info = self.bitcoind.getblockchaininfo().await?;
            log::info!("{}", info.bestblockhash);

            tokio::select! {
                _ = tokio::time::delay_for(std::time::Duration::from_secs(1)) => {},
                _ = self.shutdown.wait() => break,
            }
        }

        Ok(())
    }
}
