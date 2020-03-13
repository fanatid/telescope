use std::sync::Arc;

use crate::db::{DataBase, StaticQueries};
use crate::shutdown::Shutdown;
use crate::AnyError;

static DATABASE_VERSION: u16 = 1;
static DATABASE_QUERIES: StaticQueries = &[];

macro_rules! add_basic_methods {
    ($name:ident) => {
        impl $name {
            pub fn from_args<'a>(args: &clap::ArgMatches<'a>) -> $name {
                $name {
                    db: DataBase::from_args(args, DATABASE_VERSION, DATABASE_QUERIES),
                }
            }

            pub async fn validate(&self, shutdown: &Arc<Shutdown>) -> AnyError<()> {
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

impl IndexerDataBase {}
