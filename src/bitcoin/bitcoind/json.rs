use std::fmt;

use serde::{de, Deserialize, Deserializer, Serialize};

use crate::fixed_hash::H256;

#[derive(Debug, Serialize)]
pub struct Request<'a, 'b> {
    pub method: &'a str,
    pub params: Option<&'b [serde_json::Value]>,
    pub id: u64,
}

#[derive(Debug, Deserialize)]
pub struct Response<T> {
    pub id: u64,
    pub error: Option<ResponseError>,
    pub result: Option<T>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

impl fmt::Display for ResponseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Bitcoind RPC error (code: {}): {}",
            self.code, self.message
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct NetworkInfo {
    pub subversion: String,
}

#[derive(Debug, PartialEq, Deserialize)]
pub struct BlockchainInfo {
    pub chain: String,
    pub blocks: u32,
    #[serde(deserialize_with = "H256::deserialize_hex")]
    pub bestblockhash: H256,
}

#[derive(Debug, Deserialize)]
pub struct Block {
    pub height: u32,
    #[serde(deserialize_with = "H256::deserialize_hex")]
    pub hash: H256,
    #[serde(
        rename = "previousblockhash",
        deserialize_with = "H256::deserialize_hex_some",
        default
    )]
    pub prev_hash: Option<H256>,
    #[serde(
        rename = "nextblockhash",
        deserialize_with = "H256::deserialize_hex_some",
        default
    )]
    pub next_hash: Option<H256>,
    #[serde(rename = "tx")]
    pub transactions: Vec<Transaction>,
    pub size: u32,
    pub time: u32,
}

#[derive(Debug, Deserialize)]
pub struct Transaction {
    #[serde(deserialize_with = "H256::deserialize_hex")]
    pub hash: H256,
    #[serde(deserialize_with = "hex::deserialize")]
    pub hex: Vec<u8>,
    #[serde(rename = "vin")]
    pub inputs: Vec<TransactionInput>,
    #[serde(rename = "vout")]
    pub outputs: Vec<TransactionOutput>,
}

#[derive(Debug)]
pub enum TransactionInput {
    Coinbase { hex: Vec<u8> },
    Usual { txid: Option<H256>, vout: u32 },
}

impl<'de> Deserialize<'de> for TransactionInput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<TransactionInput, D::Error> {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = TransactionInput;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a JSON object as transaction input")
            }

            fn visit_map<V>(self, mut visitor: V) -> Result<TransactionInput, V::Error>
            where
                V: de::MapAccess<'de>,
            {
                let mut coinbase: Option<Vec<u8>> = None;
                let mut txid: Option<Option<H256>> = None;
                let mut vout: Option<u32> = None;

                macro_rules! check_duplicate {
                    ($var:ident, $name:expr) => {
                        if $var.is_some() {
                            return Err(de::Error::duplicate_field($name));
                        }
                    };
                }

                while let Some(key) = visitor.next_key()? {
                    match key {
                        "coinbase" => {
                            check_duplicate!(coinbase, "coinbase");
                            let value = visitor.next_value()?;
                            coinbase = Some(hex::decode(value).map_err(|_| {
                                de::Error::invalid_value(de::Unexpected::Str(value), &self)
                            })?);
                        }
                        "txid" => {
                            check_duplicate!(txid, "txid");
                            let value = visitor.next_value()?;
                            let mut hash = H256::zero();
                            hex::decode_to_slice(value, &mut hash.0 as &mut [u8]).map_err(
                                |_| de::Error::invalid_value(de::Unexpected::Str(value), &self),
                            )?;
                            txid = Some(Some(hash));
                        }
                        "vout" => {
                            check_duplicate!(vout, "vout");
                            vout = Some(visitor.next_value::<u32>()?);
                        }
                        "sequence" => {
                            visitor.next_value::<u32>()?;
                        }
                        "scriptSig" => {
                            #[derive(Deserialize)]
                            struct ScriptSig {};

                            visitor.next_value::<ScriptSig>()?;
                        }
                        "txinwitness" => {
                            visitor.next_value::<Vec<&str>>()?;
                        }
                        _ => {
                            return Err(de::Error::unknown_field(key, &[]));
                        }
                    }
                }

                macro_rules! extra_field {
                    ($var:ident, $name:expr, $expected:expr) => {
                        if $var.is_some() {
                            return Err(de::Error::unknown_field($name, $expected));
                        }
                    };
                }

                Ok(if coinbase.is_some() {
                    let coinbase_fields = &["coinbase"];
                    extra_field!(txid, "txid", coinbase_fields);
                    extra_field!(vout, "vout", coinbase_fields);

                    TransactionInput::Coinbase {
                        hex: coinbase.ok_or_else(|| de::Error::missing_field("coinbase"))?,
                    }
                } else {
                    let usual_fields = &["txid", "vout"];
                    extra_field!(coinbase, "coinbase", usual_fields);

                    TransactionInput::Usual {
                        txid: txid.ok_or_else(|| de::Error::missing_field("txid"))?,
                        vout: vout.ok_or_else(|| de::Error::missing_field("vout"))?,
                    }
                })
            }
        }

        deserializer.deserialize_map(Visitor)
    }
}

#[derive(Debug, Deserialize)]
pub struct TransactionOutput {
    // Require `serde_json` with feature `arbitrary_precision`
    #[serde(deserialize_with = "de_vout_value")]
    pub value: String,
    #[serde(rename = "scriptPubKey")]
    pub script: TransactionOutputScript,
}

fn de_vout_value<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    struct Visitor;

    impl<'de> de::Visitor<'de> for Visitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a JSON number")
        }

        fn visit_map<V>(self, mut visitor: V) -> Result<String, V::Error>
        where
            V: de::MapAccess<'de>,
        {
            let value = visitor.next_key::<String>()?;
            if value.is_none() {
                return Err(de::Error::invalid_type(de::Unexpected::Map, &self));
            }
            visitor.next_value()
        }
    }

    deserializer.deserialize_any(Visitor)
}

#[derive(Debug, Deserialize)]
pub struct TransactionOutputScript {
    pub addresses: Option<Vec<String>>,
}

// TODO: return parsed value in satoshi
// impl TransactionOutput {
//     pub fn get_value_satoshi(&self, coin: &str) {
//     }
// }
