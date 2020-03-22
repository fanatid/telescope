use std::sync::Arc;

use super::bitcoind::json::Block;
use crate::db::{DataBase, StaticQueries};
use crate::fixed_hash::H256;
use crate::shutdown::Shutdown;
use crate::{AnyError, EmptyResult};

static DATABASE_VERSION: u16 = 1;
static DATABASE_QUERIES: StaticQueries = &[
    ("create", include_str!("./sql/create.sql")),
    ("transform", include_str!("./sql/transform.sql")),
    ("indexer", include_str!("./sql/indexer.sql")),
];

// Move macros to databse: add_from_args, add_shared_methods
macro_rules! add_basic_methods {
    ($name:ident) => {
        impl $name {
            pub fn from_args<'a>(args: &clap::ArgMatches<'a>) -> $name {
                $name {
                    db: DataBase::from_args(args, DATABASE_VERSION, DATABASE_QUERIES),
                }
            }

            pub async fn validate(&self, shutdown: &Arc<Shutdown>) -> EmptyResult {
                self.db.validate(shutdown).await
            }

            // pub async fn set_stage<S: Into<String>>(&self, name: S, progress: Option<f64>) {
            //     self.db.set_stage(name, progress).await
            // }

            pub async fn get_stage(&self) -> (String, Option<f64>) {
                self.db.get_stage().await
            }

            pub async fn get_skipped_block_heights(&self, start_height: u32) -> AnyError<Vec<u32>> {
                self.db.get_skipped_block_heights(start_height).await
            }
        }
    };
}

add_basic_methods!(IndexerDataBase);

#[derive(Debug)]
pub struct IndexerDataBase {
    db: DataBase,
}

impl IndexerDataBase {
    // Return `(height, hash)` for best block
    pub async fn get_bestblock_info(&self) -> AnyError<Option<(u32, H256)>> {
        let query = self.db.queries.get("indexer", "blocksSelectBestInfo");
        let client = self.db.pool.get().await?;
        let row = client.query_opt(query, &[]).await?;
        Ok(row.map(|row| {
            let height: u32 = row.get("height");
            let hash: Vec<u8> = row.get("hash");
            (height, H256::from_slice(&hash))
        }))
    }

    pub async fn push_block(&self, _block: &Block) -> AnyError<()> {
        Ok(())
    }
}
