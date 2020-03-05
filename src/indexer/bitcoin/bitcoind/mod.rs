use std::io::Write;
use std::time::{Duration, SystemTime};

use base64::write::EncoderWriter as Base64Encoder;
use log::info;
use regex::Regex;
use semver::{Version, VersionReq};
use url::Url;

use self::error::{BitcoindError, BitcoindResult};
use self::json::ResponseBlockchainInfo;
use self::rest::RESTClient;
use self::rpc::RPCClient;
use crate::shutdown::Shutdown;

pub mod error;
pub mod json;
mod rest;
mod rpc;

static EXPECTED_BITCOIND_VERSION: &[(&str, &str)] = &[("bitcoin", ">= 0.19.0")];

static EXPECTED_BITCOIND_USERAGENT: &[(&str, &str)] = &[("bitcoin", "Satoshi")];

#[derive(Debug)]
pub struct Bitcoind {
    coin: String,
    chain: String,
    rest: RESTClient,
    rpc: RPCClient,
}

impl Bitcoind {
    pub fn from_args(args: &clap::ArgMatches<'_>) -> BitcoindResult<Bitcoind> {
        let coin = args.value_of("coin").unwrap().to_owned();
        let chain = args.value_of("chain").unwrap().to_owned();
        let url = args.value_of("bitcoind").unwrap();
        Bitcoind::new(coin, chain, url)
    }

    pub fn new(coin: String, chain: String, url: &str) -> BitcoindResult<Bitcoind> {
        let (url, auth) = Bitcoind::parse_url(url)?;

        Ok(Bitcoind {
            coin,
            chain,
            rest: RESTClient::new(url.clone())?,
            rpc: RPCClient::new(url, auth)?,
        })
    }

    // Prase given URL with username/password
    fn parse_url(url: &str) -> BitcoindResult<(Url, Vec<u8>)> {
        let mut parsed = Url::parse(url).map_err(BitcoindError::InvalidUrl)?;
        match parsed.scheme() {
            "http" | "https" => {}
            scheme => return Err(BitcoindError::InvalidUrlScheme(scheme.to_owned())),
        }

        // https://docs.rs/reqwest/0.10.1/src/reqwest/async_impl/request.rs.html#183-199
        let mut auth = b"Basic ".to_vec();
        {
            let mut encoder = Base64Encoder::new(&mut auth, base64::STANDARD);
            // The unwraps here are fine because Vec::write* is infallible.
            write!(encoder, "{}:", parsed.username()).unwrap();
            if let Some(password) = parsed.password() {
                write!(encoder, "{}", password).unwrap();
            }
        }

        // Return Err only if `.cannot_be_a_base` is true
        // Since we already verified that scheme is http/https, unwrap is safe
        parsed.set_username("").unwrap();
        parsed.set_password(None).unwrap();

        Ok((parsed, auth))
    }

    pub async fn validate(&self, shutdown: &mut Shutdown) -> BitcoindResult<()> {
        self.validate_client_initialized(shutdown).await?;
        if !shutdown.is_recv() {
            self.validate_chain().await?;
            self.validate_version().await?;
            self.validate_clients_to_same_node().await?;
        }

        Ok(())
    }

    async fn validate_client_initialized(&self, shutdown: &mut Shutdown) -> BitcoindResult<()> {
        let mut ts = SystemTime::now();
        let mut last_message = String::new();

        loop {
            tokio::select! {
                info = self.rpc.getblockchaininfo() => {
                    match info {
                        Ok(_) => break,
                        Err(BitcoindError::ResultRPC(error)) => {
                            // Client warming up error code is "-28"
                            // https://github.com/bitcoin/bitcoin/pull/5007
                            if error.code != -28 {
                                return Err(BitcoindError::ResultRPC(error));
                            }

                            let elapsed = ts.elapsed().unwrap();
                            if elapsed > Duration::from_secs(3) || last_message != error.message {
                                ts = SystemTime::now();
                                last_message = error.message;
                                info!("Waiting coin client: {}", &last_message);
                            }

                            let sleep_duration = Duration::from_millis(10);
                            tokio::time::delay_for(sleep_duration).await;
                        }
                        Err(e) => return Err(e),
                    }
                }
                _ = shutdown.wait() => break,
            }
        }

        Ok(())
    }

    async fn validate_chain(&self) -> BitcoindResult<()> {
        let info = self.rpc.getblockchaininfo().await?;
        if info.chain != self.chain {
            Err(BitcoindError::ClientInvalidX(
                "chain".to_owned(),
                info.chain,
                self.chain.to_owned(),
            ))
        } else {
            Ok(())
        }
    }

    async fn validate_version(&self) -> BitcoindResult<()> {
        let info = self.rpc.getnetworkinfo().await?;

        // Split useragent and version from strings like: "/Satoshi:0.19.0.1/"
        let re_split = Regex::new(r#"^/([a-zA-Z ]+):([0-9.]+)/$"#).unwrap();
        let (useragent, mut version) = match re_split.captures(&info.subversion) {
            Some(cap) => (cap.get(1).unwrap().as_str(), cap.get(2).unwrap().as_str()),
            None => {
                return Err(BitcoindError::ClientInvalidVersionX(
                    "subversion".to_owned(),
                    info.subversion,
                ))
            }
        };

        // Validate useragent
        for (coin, value) in EXPECTED_BITCOIND_USERAGENT {
            if coin == &self.coin {
                if value != &useragent {
                    return Err(BitcoindError::ClientInvalidX(
                        "useragent".to_owned(),
                        useragent.to_owned(),
                        value.to_owned().to_owned(),
                    ));
                }

                break;
            }
        }

        // Remove extra digits in version and validate it
        while version.matches('.').count() > 2 {
            version = &version[0..version.rfind('.').unwrap()];
        }
        for (coin, value) in EXPECTED_BITCOIND_VERSION {
            if coin == &self.coin {
                let actual = match Version::parse(version) {
                    Ok(v) => v,
                    Err(_) => {
                        return Err(BitcoindError::ClientInvalidVersionX(
                            "version".to_owned(),
                            version.to_owned(),
                        ))
                    }
                };
                let required = VersionReq::parse(value).unwrap();
                if !required.matches(&actual) {
                    return Err(BitcoindError::ClientInvalidX(
                        "version".to_owned(),
                        version.to_owned(),
                        value.to_owned().to_owned(),
                    ));
                }
            }
        }

        Ok(())
    }

    async fn validate_clients_to_same_node(&self) -> BitcoindResult<()> {
        let rpc_fut = self.rpc.getblockchaininfo();
        let rest_fut = self.rest.getblockchaininfo();
        let (rpc, rest) = tokio::try_join!(rpc_fut, rest_fut)?;
        if rpc != rest {
            Err(BitcoindError::ClientMismatch)
        } else {
            Ok(())
        }
    }

    pub async fn getblockchaininfo(&self) -> BitcoindResult<ResponseBlockchainInfo> {
        self.rpc.getblockchaininfo().await
    }
}
