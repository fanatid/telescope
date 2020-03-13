use std::sync::Arc;
use std::time::Duration;

use bb8::Pool;
use bb8_postgres::PostgresConnectionManager;
use futures::TryFutureExt;
use humantime::parse_duration;
use log::info;
use regex::Regex;
use tokio_postgres::{Config, NoTls};

use super::error::DataBaseError;
use super::queries::{Queries, StaticQueries};
use crate::shutdown::Shutdown;
use crate::AnyError;

static BASE_QUERIES: StaticQueries = &[("base", include_str!("./base.sql"))];

#[derive(Debug)]
pub struct DataBase {
    coin: String,
    chain: String,
    version: u16,

    pub queries: Queries,
    pub pool: Pool<PostgresConnectionManager<NoTls>>,
}

impl DataBase {
    pub fn from_args<'a>(
        args: &clap::ArgMatches<'a>,
        version: u16,
        app_queries: StaticQueries,
    ) -> DataBase {
        // unwrap is safe because values validated in args
        let coin = args.value_of("coin").unwrap();
        let chain = args.value_of("chain").unwrap();
        let conn_str = args.value_of("postgres").unwrap();
        let conn_timeout = args.value_of("postgres_connection_timeout").unwrap();
        let pool_size = args.value_of("postgres_pool_size").unwrap();
        let schema = args.value_of("postgres_schema").unwrap();

        // Parse queries in text to Map
        let mut queries = Queries::new();
        queries.load(BASE_QUERIES, schema);
        queries.load(app_queries, schema);

        // Create connections pool
        let conf = conn_str.parse::<Config>().unwrap();
        let manager = PostgresConnectionManager::new(conf, NoTls);
        let pool = Pool::builder()
            .max_size(pool_size.parse().unwrap())
            .min_idle(None)
            .max_lifetime(None)
            .idle_timeout(Some(Duration::from_secs(10 * 60)))
            .connection_timeout(parse_duration(conn_timeout).unwrap())
            // .build(manager) -- will check nothing, because minimum number of idle connections is 0
            .build_unchecked(manager);

        // Instance
        DataBase {
            coin: coin.to_owned(),
            chain: chain.to_owned(),
            version,
            queries,
            pool,
        }
    }

    pub async fn validate(&self, shutdown: &Arc<Shutdown>) -> AnyError<()> {
        tokio::select! {
            v = self.validate_version().and_then(|_| self.validate_schema()) => v,
            e = shutdown.wait() => Err(e.into()),
        }
    }

    async fn validate_version(&self) -> AnyError<()> {
        let version_query = &self.queries["base"]["selectVersion"];
        let version_required = "12.*".to_owned();
        let version_re = Regex::new(r#"12\..*"#).unwrap();

        let conn = self.pool.get().await?;
        let row = conn.query_one(version_query, &[]).await?;
        let version: String = row.get("version");

        if version_re.is_match(&version) {
            Ok(())
        } else {
            Err(DataBaseError::InvalidPostgreSQLVersion(version, version_required).into())
        }
    }

    async fn validate_schema(&self) -> AnyError<()> {
        let queries = &self.queries["base"];
        let extra_data = serde_json::json!({});

        let mut conn = self.pool.get().await?;
        let tx = conn.transaction().await?;

        // create schema if not exists
        let q = tx.query(&queries["schemaExists"], &[]);
        if q.await?.is_empty() {
            tx.execute(&queries["schemaCreate"], &[]).await?;
        }

        // create table `schema_info`, or validate data from it
        let q = tx.query(&queries["schemaInfoExists"], &[]);
        if q.await?.is_empty() {
            tx.query(&queries["schemaInfoCreate"], &[]).await?;
            tx.query(
                &queries["schemaInfoInsert"],
                &[&self.coin, &self.chain, &(self.version as i16), &extra_data],
            )
            .await?;

            info!("[db] tables created");
        } else {
            let q = tx.query_one(&queries["schemaInfoSelect"], &[]);
            let row = q.await.expect("data in schema_info should exists");

            macro_rules! assert {
                ($name:expr, $vtype:ty, $actual:expr) => {
                    let value: $vtype = row.get($name);
                    if value != $actual {
                        let name = $name.to_owned();
                        let value = value.to_string();
                        let actual = $actual.to_string();
                        return Err(DataBaseError::InvalidSchemaItem(name, value, actual).into());
                    }
                };
            }

            assert!("coin", String, self.coin);
            assert!("chain", String, self.chain);
            assert!("version", i16, self.version as i16);
            assert!("extra", serde_json::Value, extra_data);

            info!("[db] schema verified");
        }

        tx.commit().await?;
        Ok(())
    }
}
