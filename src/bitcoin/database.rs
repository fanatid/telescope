use std::sync::Arc;

use crate::db::{DataBase, StaticQueries};
use crate::shutdown::Shutdown;
use crate::EmptyResult;

static DATABASE_VERSION: u16 = 1;
static DATABASE_QUERIES: StaticQueries = &[
    ("create", include_str!("./sql/create.sql")),
    ("transform", include_str!("./sql/transform.sql")),
    ("indexer", include_str!("./sql/indexer.sql")),
];

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
        }
    };
}

add_basic_methods!(IndexerDataBase);

#[derive(Debug)]
pub struct IndexerDataBase {
    db: DataBase,
}
