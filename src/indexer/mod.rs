use crate::AnyError;

mod bitcoin;

pub async fn main(args: &clap::ArgMatches<'static>) -> AnyError<()> {
    let fut = match args.subcommand() {
        ("bitcoin", Some(args)) => bitcoin::Indexer::from_args(args),
        _ => unreachable!("Unknow subcommand"),
    };
    fut.await
}
