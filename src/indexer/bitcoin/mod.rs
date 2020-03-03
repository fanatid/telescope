use crate::shutdown::Shutdown;
use crate::AnyError;
use bitcoind::Bitcoind;

mod bitcoind;

pub struct Indexer<'a> {
    shutdown: Shutdown,
    bitcoind: Bitcoind<'a>,
}

impl<'a> Indexer<'a> {
    pub async fn from_args(shutdown: Shutdown, args: &clap::ArgMatches<'a>) -> AnyError<()> {
        let coin = args.value_of("coin").unwrap();
        let chain = args.value_of("chain").unwrap();

        // bitcoind
        let bitcoind_url = args.value_of("bitcoind").unwrap();
        let bitcoind = Bitcoind::new(coin, chain, bitcoind_url)?;

        // create indexer
        let mut indexer = Indexer { shutdown, bitcoind };

        // connect first
        indexer.connect().await?;

        //
        indexer.start().await
    }

    async fn connect(&mut self) -> AnyError<()> {
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
