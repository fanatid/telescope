use std::io::Write;
use std::time::{Duration, SystemTime};

use base64::write::EncoderWriter as Base64Encoder;
use log::info;
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

#[derive(Debug)]
pub struct Bitcoind<'a> {
    coin: &'a str,
    chain: &'a str,
    rest: RESTClient,
    rpc: RPCClient,
}

impl<'a> Bitcoind<'a> {
    pub fn new(coin: &'a str, chain: &'a str, url: &'a str) -> BitcoindResult<Bitcoind<'a>> {
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
