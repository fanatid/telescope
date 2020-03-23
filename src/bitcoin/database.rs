use std::sync::Arc;

use super::bitcoind::json::Block;
use crate::db::{DataBase, StaticQueries};
use crate::fixed_hash::H256;
use crate::shutdown::Shutdown;
use crate::{AnyResult, EmptyResult};

static DATABASE_VERSION: u16 = 1;
static DATABASE_QUERIES: StaticQueries = &[
    ("create", include_str!("./sql/create.sql")),
    ("transform", include_str!("./sql/transform.sql")),
    ("indexer", include_str!("./sql/indexer.sql")),
];

crate::db_add_basic_methods!(IndexerDataBase);

// Move macros to databse: add_from_args, add_shared_methods
macro_rules! add_basic_methods {
    ($name:ident) => {
        impl $name {
            pub fn from_args<'a>(args: &clap::ArgMatches<'a>) -> $name {
                $name {
                    db: DataBase::from_args(args, DATABASE_VERSION, DATABASE_QUERIES),
                }
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
    pub async fn get_bestblock_info(&self) -> AnyResult<Option<(u32, H256)>> {
        let query = self.db.queries.get("indexer", "blocksSelectBestInfo");
        let client = self.db.pool.get().await?;
        let row = client.query_opt(query, &[]).await?;
        Ok(row.map(|row| {
            let height: u32 = row.get("height");
            let hash: Vec<u8> = row.get("hash");
            (height, H256::from_slice(&hash))
        }))
    }

    pub async fn push_block(&self, block: &Block) -> AnyResult<()> {
        println!("Push block: {} => {}", block.height, block.hash);
        Ok(())
    }
}
