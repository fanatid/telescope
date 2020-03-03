use crate::AnyError;
use bitcoind::Bitcoind;

mod bitcoind;

pub struct Indexer {
    coin: String,
    chain: String,
    bitcoind: Bitcoind,
}

impl Indexer {
    pub async fn from_args(args: &clap::ArgMatches<'static>) -> AnyError<()> {
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
        indexer.start().await
    }

    async fn connect(&mut self) -> AnyError<()> {
        self.bitcoind.validate(&self.coin, &self.chain).await?;
        Ok(())
    }

    async fn start(&mut self) -> AnyError<()> {
        let info = self.bitcoind.getblockchaininfo().await?;
        println!("{}", info.bestblockhash);

        Ok(())
    }
}
