use std::net::ToSocketAddrs;

use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use humantime::parse_duration;
use tokio_postgres::Config as PgConfig;
use url::Url;

// warning: explicit lifetimes given in parameter types where they could be elided (or replaced with `'_` if needed by type declaration)
#[allow(clippy::needless_lifetimes)]
pub fn get_args<'a>(num_cpus: &'a str) -> ArgMatches<'a> {
    let version = include_str!("./args.rs-version").trim();

    // Settings, global and leaf
    let settings_global = [
        AppSettings::DisableHelpSubcommand,
        AppSettings::DeriveDisplayOrder,
        AppSettings::SubcommandRequiredElseHelp,
        AppSettings::VersionlessSubcommands,
    ];
    let settings_leaf = [
        AppSettings::DisableHelpSubcommand,
        AppSettings::DeriveDisplayOrder,
        AppSettings::VersionlessSubcommands,
    ];

    // We can set `global` and `required` flags at one moment,
    // because clap panic with error: "Global arguments cannot be required."
    // So we will add these flags to each subcommand manually.
    let args_global = [
        // PostgreSQL args
        Arg::with_name("postgres")
            .long("postgres")
            .help("libpq-style connection string")
            .required(true)
            .validator(validate_url_postgres)
            .value_name("conn_str")
            .env("TELESCOPE_POSTGRES"),
        Arg::with_name("postgres_connection_timeout")
            .long("postgres-connection-timeout")
            .help("PostgreSQL connection timeout")
            .validator(validate_duration)
            .value_name("time")
            .default_value("3sec")
            .env("TELESCOPE_POSTGRES_CONNECTION_TIMEOUT"),
        Arg::with_name("postgres_pool_size")
            .long("postgres-pool-size")
            .help("PostgreSQL connections pool size")
            .validator(validate_u32_gt0)
            .value_name("number")
            .default_value("10")
            .env("TELESCOPE_POSTGRES_POOL_SIZE"),
        Arg::with_name("postgres_schema")
            .long("postgres-schema")
            .help("PostgreSQL schema name")
            .validator(validate_pg_schema)
            .value_name("name")
            .default_value("public")
            .env("TELESCOPE_POSTGRES_SCHEMA"),
        // HTTP (http-api / ws / prometheus)
        Arg::with_name("listen_http")
            .long("listen-http")
            .help("Start HTTP server at host:port (probes, prometheus and etc)")
            .validator(validate_addr)
            .value_name("addr")
            .default_value("localhost:8000")
            .env("TELESCOPE_LISTEN_HTTP"),
    ];

    // Indexer global shared args
    let args_global_indexer = [
        // Sync blocks from specified height for debug
        Arg::with_name("sync_from")
            .long("sync-from")
            .help("Sync only from specified block (only for development)")
            .validator(validate_u32_gt0)
            .value_name("block")
            .default_value("0")
            .env("TELESCOPE_SYNC_FROM"),
        // On initial stage we can import blocks parallel
        Arg::with_name("sync_threads")
            .long("sync-threads")
            .help("Use N threads for blocks processing on initial sync")
            .validator(validate_u32_gt0)
            .value_name("threads")
            .default_value(num_cpus)
            .env("TELESCOPE_SYNC_THREADS"),
    ];
    // Client global shared args
    let args_global_client = [];

    // Bitcoin shared args
    let args_bitcoin = [
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
    ];
    let args_bitcoin_indexer = [
        // Client: bitcoind
        Arg::with_name("bitcoind")
            .long("bitcoind")
            .help("Bitcoind URL to RPC & Rest")
            .required(true)
            .validator(validate_url)
            .value_name("url")
            .default_value("http://bitcoinrpc:password@localhost:8332/")
            .env("TELESCOPE_BITCOIND"),
    ];
    let args_bitcoin_client = [];

    // Bitcoin shared SubCommand
    let subcommand_bitcoin = SubCommand::with_name("bitcoin")
        .about("Bitcoin, bitcoin forks and bitcoin like coins")
        .settings(&settings_leaf)
        .args(&args_global)
        .args(&args_bitcoin);

    // App and SubCommands
    App::new("telescope")
        .about("Set of blockchains indexers")
        .version(version)
        .settings(&settings_global)
        .subcommands(vec![
            SubCommand::with_name("indexer")
                .about("Transform blockchain client data to our database")
                .settings(&settings_global)
                .subcommands(vec![subcommand_bitcoin
                    .clone()
                    .args(&args_global_indexer)
                    .args(&args_bitcoin_indexer)]),
            SubCommand::with_name("client")
                .about("API to transformed data in database")
                .settings(&settings_global)
                .subcommands(vec![subcommand_bitcoin
                    .clone()
                    .args(&args_global_client)
                    .args(&args_bitcoin_client)]),
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

fn validate_duration(value: String) -> ValidateResult {
    let parsed = parse_duration(&value);
    validate_transform_result(parsed)
}

// https://til.hashrocket.com/posts/8f87c65a0a-postgresqls-max-identifier-length-is-63-bytes
// Max identifier length is 63 symbols.
fn validate_pg_schema(value: String) -> ValidateResult {
    if value.len() > 63 {
        return Err(format!(
            "Schema name should not be long than 63 symbols, current length: {}",
            value.len()
        ));
    }

    if value.starts_with("pg_") {
        return Err(r#"Schema name started with "pg_" not allowed"#.to_owned());
    }

    Ok(())
}

fn validate_u32_gt0(value: String) -> ValidateResult {
    match value.parse::<u32>() {
        Ok(v) => {
            if v > 0 {
                Ok(())
            } else {
                Err("Value should be greater than zero".to_owned())
            }
        }
        Err(e) => Err(format!("{}", e)),
    }
}

fn validate_url(url: String) -> ValidateResult {
    let parsed = Url::parse(&url);
    validate_transform_result(parsed)
}

fn validate_url_postgres(url: String) -> ValidateResult {
    let parsed = url.parse::<PgConfig>();
    validate_transform_result(parsed)
}
