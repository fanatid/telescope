// We use REST interfance, because extract block blocks through RPC was slow,
// not all coin clients fixed it.
// See issue in bitcoin repo: https://github.com/bitcoin/bitcoin/issues/15925

use std::fmt;
use std::time::Duration;

use reqwest::{header, redirect, Client, ClientBuilder, RequestBuilder};
use url::Url;

use super::error::{BitcoindError, BitcoindResult};
use super::json::{Block, BlockchainInfo};
use crate::fixed_hash::H256;

pub struct RESTClient {
    client: Client,
    url: Url,
}

impl fmt::Debug for RESTClient {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RESTClient")
            .field("url", &self.url)
            .finish()
    }
}

impl RESTClient {
    pub fn new(url: Url) -> BitcoindResult<RESTClient> {
        let mut headers = header::HeaderMap::with_capacity(1);
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("applicaiton/json"),
        );

        let client = ClientBuilder::new()
            .connect_timeout(Duration::from_millis(250))
            .timeout(Duration::from_secs(30))
            .default_headers(headers)
            .no_gzip()
            .redirect(redirect::Policy::none());

        Ok(RESTClient {
            client: client.build().map_err(BitcoindError::Reqwest)?,
            url,
        })
    }

    fn request(&self, path: &str) -> RequestBuilder {
        let mut url = self.url.clone();
        url.set_path(path);
        self.client.get(url)
    }

    pub async fn get_blockchain_info(&self) -> BitcoindResult<BlockchainInfo> {
        let timeout = Duration::from_millis(250);

        let res_fut = self.request("rest/chaininfo.json").timeout(timeout).send();
        let res = res_fut.await.map_err(BitcoindError::Reqwest)?;
        let status_code = res.status().as_u16();

        let body = res.bytes().await.map_err(BitcoindError::Reqwest)?;

        match status_code {
            200 => serde_json::from_slice(&body).map_err(BitcoindError::ResponseParse),
            code => {
                let msg = String::from_utf8_lossy(&body).trim().to_owned();
                Err(BitcoindError::ResultRest(code, msg))
            }
        }
    }

    pub async fn get_block_by_hash(&self, hash: H256) -> BitcoindResult<Option<Block>> {
        let res_fut = self
            .request(&format!("rest/block/{}.json", hex::encode(hash)))
            .send();
        let res = res_fut.await.map_err(BitcoindError::Reqwest)?;

        let status_code = res.status().as_u16();
        if status_code == 404 {
            return Ok(None);
        }

        // Should be serde_json::from_reader
        let body_fut = res.bytes();
        let body = body_fut.await.map_err(BitcoindError::Reqwest)?;
        if status_code != 200 {
            let msg = String::from_utf8_lossy(&body).trim().to_owned();
            return Err(BitcoindError::ResultRest(status_code, msg));
        }

        // In `release` can take up to 60ms (and more), for `debug` ~10x more time comapre to `release`.
        // See also https://github.com/fanatid/bitcoin-rust-learning
        let parsed = serde_json::from_slice(&body);
        let block: Block = parsed.map_err(BitcoindError::ResponseParse)?;

        // Check that received block match to requested
        if block.hash != hash {
            return Err(BitcoindError::ResultMismatch);
        }

        Ok(Some(block))
    }
}
