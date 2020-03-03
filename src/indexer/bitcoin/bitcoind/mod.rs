use std::io::Write;
use std::time::{Duration, SystemTime};

use base64::write::EncoderWriter as Base64Encoder;
use log::info;
use url::Url;

pub use self::error::{BitcoindError, BitcoindResult};
use self::json::ResponseBlockchainInfo;
use self::rest::RESTClient;
use self::rpc::RPCClient;

mod error;
pub mod json;
mod rest;
mod rpc;

// Q: Sync + Send -- safe?
#[derive(Debug)]
pub struct Bitcoind {
    rest: RESTClient,
    rpc: RPCClient,
}

impl Bitcoind {
    pub fn new(url: &str) -> BitcoindResult<Bitcoind> {
        let (url, auth) = Self::parse_url(url)?;

        Ok(Bitcoind {
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

    pub async fn validate(&self, _coin: &str, _chain: &str) -> BitcoindResult<()> {
        self.validate_client_initialized().await?;
        self.validate_clients_to_same_node().await
    }

    async fn validate_client_initialized(&self) -> BitcoindResult<()> {
        let mut ts = SystemTime::now();
        let mut last_message = String::new();

        loop {
            match self.rpc.getblockchaininfo().await {
                Ok(_) => return Ok(()),
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
