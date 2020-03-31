use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use reqwest::{header, redirect, Client, ClientBuilder};
use serde::Deserialize;
use tokio::sync::Mutex;
use url::Url;

use super::error::{BitcoindError, BitcoindResult};
use super::json::{Block, BlockchainInfo, NetworkInfo, Request, Response};
use crate::fixed_hash::H256;

pub struct RPCClient {
    client: Client,
    url: Url,
    req_id: Arc<Mutex<u64>>,
}

impl fmt::Debug for RPCClient {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RPCClient")
            .field("url", &self.url)
            .field("req_id", &self.req_id)
            .finish()
    }
}

impl RPCClient {
    // Construct new RPCClient for specified URL
    pub fn new(url: Url, auth: Vec<u8>) -> BitcoindResult<RPCClient> {
        let mut headers = header::HeaderMap::with_capacity(2);
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_bytes(&auth)
                .expect("Not possible build auth from provided username/password"),
        );
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

        Ok(RPCClient {
            client: client.build().map_err(BitcoindError::Reqwest)?,
            url,
            req_id: Arc::new(Mutex::new(0)),
        })
    }

    async fn get_next_req_id(&self) -> u64 {
        let mut req_id = self.req_id.lock().await;
        *req_id = req_id.wrapping_add(1);
        *req_id
    }

    async fn request<T: serde::de::DeserializeOwned>(
        &self,
        body: Vec<u8>,
    ) -> BitcoindResult<Response<T>> {
        let res_fut = self.client.post(self.url.clone()).body(body).send();
        let res = res_fut.await.map_err(BitcoindError::Reqwest)?;

        // We ignore status, because expect error information in the body
        // let status = res.status();

        // Should be serde_json::from_reader
        let body_fut = res.bytes();
        let body = body_fut.await.map_err(BitcoindError::Reqwest)?;
        serde_json::from_slice(&body).map_err(BitcoindError::ResponseParse)
    }

    async fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<&[serde_json::Value]>,
    ) -> BitcoindResult<T> {
        let req_id = self.get_next_req_id().await;

        let body = serde_json::to_vec(&Request {
            method,
            params,
            id: req_id,
        })
        .expect("Invalid data for building JSON");

        let data = self.request::<T>(body).await?;
        if data.id != req_id {
            return Err(BitcoindError::NonceMismatch);
        }
        if let Some(error) = data.error {
            return Err(BitcoindError::ResultRPC(error));
        }
        match data.result {
            None => Err(BitcoindError::ResultNotFound),
            Some(result) => Ok(result),
        }
    }

    pub async fn get_network_info(&self) -> BitcoindResult<NetworkInfo> {
        self.call("getnetworkinfo", None).await
    }

    pub async fn get_blockchain_info(&self) -> BitcoindResult<BlockchainInfo> {
        self.call("getblockchaininfo", None).await
    }

    pub async fn get_block_hash(&self, height: u32) -> BitcoindResult<Option<H256>> {
        #[derive(Debug, Deserialize)]
        struct Response(#[serde(deserialize_with = "H256::deserialize_hex")] H256);

        let params = [height.into()];
        match self.call::<Response>("getblockhash", Some(&params)).await {
            Ok(st) => Ok(Some(st.0)),
            Err(BitcoindError::ResultRPC(error)) => {
                // Block height out of range
                if error.code == -8 {
                    Ok(None)
                } else {
                    Err(BitcoindError::ResultRPC(error))
                }
            }
            Err(error) => Err(error),
        }
    }

    pub async fn get_block_by_hash(&self, hash: H256) -> BitcoindResult<Option<Block>> {
        let params = [hex::encode(hash).into(), 2.into()];
        match self.call::<Block>("getblock", Some(&params)).await {
            Ok(block) => {
                if block.hash == hash {
                    Ok(Some(block))
                } else {
                    Err(BitcoindError::ResultMismatch)
                }
            }
            Err(BitcoindError::ResultRPC(error)) => {
                // Block not found
                if error.code == -5 {
                    Ok(None)
                } else {
                    Err(BitcoindError::ResultRPC(error))
                }
            }
            Err(error) => Err(error),
        }
    }
}
