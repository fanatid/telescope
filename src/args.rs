use std::net::ToSocketAddrs;

use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
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

    App::new("telescope")
        .about("Set of blockchains indexers")
        .version(version)
        .settings(&settings)
        .args(&[
            Arg::with_name("postgres").help("TODO"),
            Arg::with_name("listen_http")
                .long("listen-http")
                .help("Start HTTP server at host:port (probes, prometheus and etc)")
                .global(true)
                .validator(validate_addr)
                .value_name("addr")
                .default_value("localhost:8000")
                .env("TELESCOPE_LISTEN_HTTP"),
        ])
        .subcommands(vec![
            SubCommand::with_name("indexer")
                .about("Start indexer")
                .settings(&settings)
                .args(&[])
                .subcommands(vec![SubCommand::with_name("bitcoin")
                    .about("Start indexer for bitcoin and bitcoin forks")
                    .settings(&settings_child)
                    .args(&[
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
                            .possible_values(&["mainnet", "testnet"])
                            .value_name("name")
                            .default_value("mainnet")
                            .env("TELESCOPE_CHAIN"),
                        Arg::with_name("bitcoind")
                            .long("bitcoind")
                            .help("Bitcoind URL to RPC & Rest")
                            .required(true)
                            .validator(validate_url)
                            .value_name("url")
                            .env("TELESCOPE_BITCOIND"),
                    ])]),
            SubCommand::with_name("client").about("TODO"),
        ])
        .get_matches()
}

fn validate_transform_result<T, E>(value: Result<T, E>) -> Result<(), String>
where
    E: std::fmt::Display,
{
    match value {
        Err(e) => Err(format!("{}", e)),
        _ => Ok(()),
    }
}

fn validate_addr(addr: String) -> Result<(), String> {
    let addrs = addr.to_socket_addrs();
    validate_transform_result(addrs)
}

fn validate_url(url: String) -> Result<(), String> {
    let parsed = Url::parse(&url);
    validate_transform_result(parsed)
}
