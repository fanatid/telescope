use reqwest::Error as ReqwestError;
use serde_json::Error as SerdeError;
use url::ParseError as UrlParseError;

use super::json::ResponseError;

quick_error! {
    #[derive(Debug)]
    pub enum BitcoindError {
        InvalidUrl(err: UrlParseError) {
            display("Invalid URL ({})", err)
        }
        InvalidUrlScheme(scheme: String) {
            display(r#"URL scheme "{}" is not supported"#, scheme)
        }
        Reqwest(err: ReqwestError) {
            display("{}", err)
        }
        ResponseParse(err: SerdeError) {
            display("Invalid JSON response ({})", err)
        }
        NonceMismatch {
            display("Nonce mismatch")
        }
        ResultRest(code: u16, msg: String) {
            display("Bitcoind REST error (code: {}): {}", code, msg)
        }
        ResultRPC(err: ResponseError) {
            display("{}", err)
        }
        ResultNotFound {
            display("Requested object not found")
        }
        ResultMismatch {
            display("Result object not match to requested")
        }
        ClientMismatch {
            display("Chain, height or best block hash did not match between clients")
        }
    }
}

pub type BitcoindResult<T> = Result<T, BitcoindError>;
