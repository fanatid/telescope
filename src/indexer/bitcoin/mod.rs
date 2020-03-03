use crate::shutdown::Shutdown;
use crate::AnyError;
use bitcoind::Bitcoind;

mod bitcoind;

pub struct Indexer {
    coin: String,
    chain: String,
    bitcoind: Bitcoind,
}

impl Indexer {
    pub async fn from_args(shutdown: Shutdown, args: &clap::ArgMatches<'static>) -> AnyError<()> {
        let coin = args.value_of("coin").unwrap().to_owned();
        let chain = args.value_of("chain").unwrap().to_owned();

        // bitcoind
        let bitcoind_url = args.value_of("bitcoind").unwrap();
        let bitcoind = Bitcoind::new(bitcoind_url)?;

        // indexer
        let mut indexer = Indexer {
            coin,
            chain,
            bitcoind,
        };

        // connect first
        indexer.connect().await?;

        //
        indexer.start(shutdown).await
    }

    async fn connect(&mut self) -> AnyError<()> {
        self.bitcoind.validate(&self.coin, &self.chain).await?;
        Ok(())
    }

    async fn start(&mut self, mut shutdown: Shutdown) -> AnyError<()> {
        loop {
            if shutdown.is_recv() {
                break;
            }

            let info = self.bitcoind.getblockchaininfo().await?;
            println!("{}", info.bestblockhash);

            tokio::select! {
                _ = tokio::time::delay_for(std::time::Duration::from_secs(1)) => {},
                _ = shutdown.wait() => { break },
            }
        }

        Ok(())
    }
}
