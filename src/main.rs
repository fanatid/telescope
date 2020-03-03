#[macro_use]
extern crate quick_error;

mod args;
mod logger;
// mod signals;

mod indexer;
// mod client;

pub(crate) type AnyError<T> = Result<T, Box<dyn std::error::Error>>;

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
    // todo: signal shutdown

    let app_fut = match args.subcommand() {
        ("indexer", Some(args)) => indexer::main(args),
        // ("client", Some(args)) => client::main(args),
        _ => unreachable!("Unknow subcommand"),
    };

    let mut runtime = build_runtime();
    if let Err(error) = runtime.block_on(app_fut) {
        log::error!("{}", error);
        std::process::exit(1);
    }

    std::process::exit(0);
}
