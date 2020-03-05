use std::net::ToSocketAddrs;

use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use humantime::parse_duration;
use tokio_postgres::Config as PgConfig;
use url::Url;

pub fn get_args<'a>() -> ArgMatches<'a> {
    let version = include_str!("./args.rs-version").trim();
    let settings = [
        AppSettings::DisableHelpSubcommand,
        AppSettings::DeriveDisplayOrder,
        AppSettings::SubcommandRequiredElseHelp,
        AppSettings::VersionlessSubcommands,
    ];
    let settings_child = [
        AppSettings::DisableHelpSubcommand,
        AppSettings::DeriveDisplayOrder,
        AppSettings::VersionlessSubcommands,
    ];

    // We can set `global` and `required` flags at one moment,
    // because clap panic with error: "Global arguments cannot be required."
    // So we will add these flags to each subcommand manually.
    let global_args = [
        // PostgreSQL args
        Arg::with_name("postgres")
            .long("postgres")
            .help("libpq-style connection string")
            .required(true)
            .validator(validate_url_postgres)
            .value_name("conn_str")
            .env("TELESCOPE_POSTGRES"),
        Arg::with_name("postgres_connection_timeout")
            .long("postgres_connection_timeout")
            .help("PostgreSQL connection timeout")
            .validator(validate_duration)
            .value_name("time")
            .default_value("3sec")
            .env("TELESCOPE_POSTGRES_CONNECTION_TIMEOUT"),
        Arg::with_name("postgres_pool_size")
            .long("postgres_pool_size")
            .help("PostgreSQL connections pool size")
            .validator(validate_u32)
            .value_name("number")
            .default_value("10")
            .env("TELESCOPE_POSTGRES_POOL_SIZE"),
        // HTTP (http-api / ws / prometheus)
        Arg::with_name("listen_http")
            .long("listen-http")
            .help("Start HTTP server at host:port (probes, prometheus and etc)")
            .validator(validate_addr)
            .value_name("addr")
            .default_value("localhost:8000")
            .env("TELESCOPE_LISTEN_HTTP"),
    ];

    App::new("telescope")
        .about("Set of blockchains indexers")
        .version(version)
        .settings(&settings)
        .subcommands(vec![
            SubCommand::with_name("indexer")
                .about("Start indexer")
                .settings(&settings)
                .subcommands(vec![SubCommand::with_name("bitcoin")
                    .about("Start indexer for bitcoin and bitcoin forks")
                    .settings(&settings_child)
                    .args(&global_args)
                    .args(&[
                        // Bitcoin forks and chains
                        Arg::with_name("coin")
                            .long("coin")
                            .help("Coin name")
                            .possible_values(&["bitcoin"])
                            .value_name("name")
                            .default_value("bitcoin")
                            .env("TELESCOPE_COIN"),
                        Arg::with_name("chain")
                            .long("chain")
                            .help("Coin chain")
                            .possible_values(&["main", "test"])
                            .value_name("name")
                            .default_value("main")
                            .env("TELESCOPE_CHAIN"),
                        // Client: bitcoind
                        Arg::with_name("bitcoind")
                            .long("bitcoind")
                            .help("Bitcoind URL to RPC & Rest")
                            .required(true)
                            .validator(validate_url)
                            .value_name("url")
                            .default_value("http://bitcoinrpc:password@localhost:8332/")
                            .env("TELESCOPE_BITCOIND"),
                    ])]),
            SubCommand::with_name("client").about("TODO"),
        ])
        .get_matches()
}

type ValidateResult = Result<(), String>;

fn validate_transform_result<T, E>(value: Result<T, E>) -> ValidateResult
where
    E: std::fmt::Display,
{
    match value {
        Err(e) => Err(format!("{}", e)),
        _ => Ok(()),
    }
}

fn validate_addr(addr: String) -> ValidateResult {
    let addrs = addr.to_socket_addrs();
    validate_transform_result(addrs)
}

fn validate_url(url: String) -> ValidateResult {
    let parsed = Url::parse(&url);
    validate_transform_result(parsed)
}

fn validate_url_postgres(url: String) -> ValidateResult {
    let parsed = url.parse::<PgConfig>();
    validate_transform_result(parsed)
}

fn validate_duration(value: String) -> ValidateResult {
    let parsed = parse_duration(&value);
    validate_transform_result(parsed)
}

fn validate_u32(value: String) -> ValidateResult {
    let parsed = value.parse::<usize>();
    validate_transform_result(parsed)
}
