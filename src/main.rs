#[macro_use]
extern crate quick_error;

mod args;
mod db;
mod error;
mod fixed_hash;
mod logger;
mod shutdown;
mod signals;

// SubCommands
mod bitcoin;

type AnyError = Box<dyn std::error::Error + Send + Sync>;
type AnyResult<T> = Result<T, AnyError>;
type EmptyResult = AnyResult<()>;
type AppFutFromArgs = AnyResult<futures::future::LocalBoxFuture<'static, EmptyResult>>;

fn build_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new()
        .core_threads(num_cpus::get())
        .enable_io()
        .enable_time()
        .threaded_scheduler()
        .build()
        .expect("error on building runtime")
}

fn main() {
    logger::init();

    let num_cpus_str = &num_cpus::get().to_string();
    let args = args::get_args(num_cpus_str);
    let mut runtime = build_runtime();

    let main_fut = async move {
        let shutdown = shutdown::subscribe();

        let fut = match args.subcommand() {
            ("indexer", Some(args)) => match args.subcommand() {
                ("bitcoin", Some(args)) => bitcoin::Indexer::from_args(shutdown, args),
                _ => unreachable!("Unknow subcommand"),
            },
            ("client", Some(args)) => match args.subcommand() {
                ("bitcoin", Some(args)) => bitcoin::Client::from_args(shutdown, args),
                _ => unreachable!("Unknow subcommand"),
            },
            _ => unreachable!("Unknow subcommand"),
        }?;

        drop(args); // do not need args anymore

        fut.await
    };

    if let Err(error) = runtime.block_on(main_fut) {
        // Shutdown signal is not an error, but provide nice way exit from app with `?` operator.
        // We can not check that `error` is `ShutdownSignal`, because `Box<dyn Error>` loose info.
        // More over, ShutdownSignal can be sub-error, see BitcoindError as example.
        if format!("{:?}", error).find("ShutdownSignal").is_none() {
            logger::error!("{}", error);
            std::process::exit(1);
        }
    }

    std::process::exit(0);
}
