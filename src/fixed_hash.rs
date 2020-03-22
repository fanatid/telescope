use std::fmt;

use fixed_hash::construct_fixed_hash;
use serde::{de, Deserializer};

construct_fixed_hash! {
    pub struct H256(32);
}

macro_rules! impl_serde_hex {
    ($name:ident, $len:expr) => {
        impl $name {
            pub fn deserialize_hex<'de, D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct Visitor;

                impl<'de> de::Visitor<'de> for Visitor {
                    type Value = H256;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        write!(formatter, "a hex encoded string with len 64")
                    }

                    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                        let mut hash = $name::zero();
                        match hex::decode_to_slice(v, &mut hash.0 as &mut [u8]) {
                            Ok(_) => Ok(hash),
                            Err(_) => Err(E::invalid_length(v.len(), &self)),
                        }
                    }
                }

                deserializer.deserialize_str(Visitor)
            }

            pub fn deserialize_hex_some<'de, D>(deserializer: D) -> Result<Option<Self>, D::Error>
            where
                D: Deserializer<'de>,
            {
                H256::deserialize_hex(deserializer).map(Some)
            }
        }
    };
}

impl_serde_hex!(H256, 32);

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn fixed_hash_deserialize_without_prefix() {
        #[derive(Debug, Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "H256::deserialize_hex")]
            pub hash: H256,
        }

        let hash = H256::random();
        let raw = format!(r#"{{"hash":"{}"}}"#, hex::encode(hash));
        let st: TestStruct = serde_json::from_str(&raw).unwrap();
        assert_eq!(st.hash, hash);
    }

    #[test]
    fn fixed_hash_deserialize_without_prefix_option() {
        #[derive(Debug, Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "H256::deserialize_hex_some", default)]
            pub hash: Option<H256>,
        }

        let hash = H256::random();
        let raw = format!(r#"{{"hash":"{}"}}"#, hex::encode(hash));
        let st: TestStruct = serde_json::from_str(&raw).unwrap();
        assert_eq!(st.hash, Some(hash));

        let raw = "{}";
        let st: TestStruct = serde_json::from_str(&raw).unwrap();
        assert_eq!(st.hash, None);
    }
}
