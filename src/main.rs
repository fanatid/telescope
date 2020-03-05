#[macro_use]
extern crate quick_error;

mod args;
mod db;
mod logger;
mod shutdown;
mod signals;

// SubCommands
mod client;
mod indexer;

type AnyError<T> = Result<T, Box<dyn std::error::Error>>;

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

    let args = args::get_args();
    let mut runtime = build_runtime();

    let main_fut = async move {
        let shutdown = shutdown::subscribe();

        match args.subcommand() {
            ("indexer", Some(args)) => indexer::main(shutdown, args).await,
            ("client", Some(args)) => client::main(shutdown, args).await,
            _ => unreachable!("Unknow subcommand"),
        }
    };

    if let Err(error) = runtime.block_on(main_fut) {
        log::error!("{}", error);
        std::process::exit(1);
    }

    std::process::exit(0);
}
