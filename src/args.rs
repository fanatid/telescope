use std::net::ToSocketAddrs;

use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use humantime::parse_duration;
use tokio_postgres::Config as PgConfig;
use url::Url;

pub fn get_args<'a>() -> ArgMatches<'a> {
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
            .validator(validate_u32)
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
        // Sync segment for debug
        Arg::with_name("sync_segment")
            .long("sync-segment")
            .help("Sync only specified blocks. Range-like syntax: `start..end`, `x >= start` and `x <= end`. Special value: `latest`.")
            .validator(validate_sync_segment)
            .value_name("segment")
            .default_value("0..latest")
            .env("TELESCOPE_SYNC_SEGMENT"),
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

fn validate_sync_segment(value: String) -> ValidateResult {
    let parsed = SyncSegment::parse(&value);
    validate_transform_result(parsed)
}

fn validate_u32(value: String) -> ValidateResult {
    let parsed = value.parse::<usize>();
    validate_transform_result(parsed)
}

fn validate_url(url: String) -> ValidateResult {
    let parsed = Url::parse(&url);
    validate_transform_result(parsed)
}

fn validate_url_postgres(url: String) -> ValidateResult {
    let parsed = url.parse::<PgConfig>();
    validate_transform_result(parsed)
}

#[derive(Debug)]
pub struct SyncSegment {
    full: bool,
    start: u32,
    end: Option<u32>,
}

impl SyncSegment {
    pub fn parse(value: &str) -> Result<(u32, Option<u32>), String> {
        let mut parts = value.split("..");

        let start = parts.next().expect("first item should always exists");
        let start = match start.parse::<u32>() {
            Ok(v) => v,
            Err(_) => return Err(format!("`start` part is not valid: {}", start)),
        };

        let end = match parts.next() {
            Some(s) => s,
            None => return Err("`end` part is not exists".to_owned()),
        };
        let end = if end == "latest" {
            None
        } else {
            Some(match end.parse::<u32>() {
                Ok(v) => v,
                Err(_) => return Err(format!("`end` part is not valid: {}", end)),
            })
        };

        Ok((start, end))
    }

    pub fn from_args(args: &clap::ArgMatches<'_>) -> SyncSegment {
        let value = args.value_of("sync_segment").unwrap();
        let (start, end) = SyncSegment::parse(value).unwrap();
        let full = start == 0 && end.is_none();
        SyncSegment { full, start, end }
    }

    pub fn is_full(&self) -> bool {
        self.full
    }

    pub fn get_start(&self) -> u32 {
        self.start
    }

    pub fn get_end(&self, node_best_height: u32) -> u32 {
        match self.end {
            Some(end) => end,
            None => node_best_height,
        }
    }
}
