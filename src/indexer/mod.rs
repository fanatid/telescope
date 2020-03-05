use crate::shutdown::Shutdown;
use crate::AnyError;

mod bitcoin;

pub async fn main(shutdown: Shutdown, args: &clap::ArgMatches<'_>) -> AnyError<()> {
    match args.subcommand() {
        ("bitcoin", Some(args)) => bitcoin::Indexer::from_args(shutdown, args).await,
        _ => unreachable!("Unknow subcommand"),
    }
}
