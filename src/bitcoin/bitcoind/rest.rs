// We use REST interfance, because extract block blocks through RPC was slow,
// not all coin clients fixed it.
// See issue in bitcoin repo: https://github.com/bitcoin/bitcoin/issues/15925

use std::fmt;
use std::time::Duration;

use reqwest::{header, redirect, Client, ClientBuilder, RequestBuilder};
use url::Url;

use super::error::{BitcoindError, BitcoindResult};
use super::json::ResponseBlockchainInfo;

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

    pub async fn getblockchaininfo(&self) -> BitcoindResult<ResponseBlockchainInfo> {
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
}
