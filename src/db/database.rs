use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bb8::Pool;
use bb8_postgres::PostgresConnectionManager;
use futures::{pin_mut, TryFutureExt as _, TryStreamExt as _};
use humantime::{format_duration, parse_duration};
use semver::{Version, VersionReq};
use tokio::sync::RwLock;
use tokio_postgres::{types::ToSql, Config, NoTls};

use super::error::DataBaseError;
use super::queries::{Queries, StaticQueries};
use crate::args::SyncSegment;
use crate::logger::info;
use crate::shutdown::Shutdown;
use crate::{AnyError, EmptyResult};

static BASE_QUERIES: StaticQueries = &[
    ("base", include_str!("./sql/base.sql")),
    ("shared", include_str!("./sql/shared.sql")),
];

#[derive(Debug)]
pub struct DataBase {
    coin: String,
    chain: String,
    version: u16,
    sync_segment: SyncSegment,
    stage: RwLock<(String, Option<f64>)>,

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
            sync_segment: SyncSegment::from_args(args),
            stage: RwLock::new(("#none".to_owned(), None)),
            queries,
            pool,
        }
    }

    pub async fn validate(&self, shutdown: &Arc<Shutdown>) -> EmptyResult {
        tokio::select! {
            v = self.validate_version().and_then(|_| self.validate_schema()) => v,
            e = shutdown.wait() => Err(e.into()),
        }
    }

    async fn validate_version(&self) -> EmptyResult {
        let version_query = &self.queries["base"]["selectVersion"];
        let version_req = VersionReq::parse("12.*").unwrap();

        let client = self.pool.get().await?;
        let row = client.query_one(version_query, &[]).await?;
        let mut version: String = row.get("version");
        if version.matches('.').count() == 1 {
            version += ".0";
        }

        let version = Version::parse(&version).unwrap();
        if version_req.matches(&version) {
            Ok(())
        } else {
            Err(DataBaseError::InvalidPostgreSQLVersion(
                format!("{}", version),
                format!("{}", version_req),
            )
            .into())
        }
    }

    async fn validate_schema(&self) -> EmptyResult {
        let queries = &self.queries["base"];
        let extra_data = serde_json::json!({
            "sync_segment": !self.sync_segment.is_full(),
        });

        let mut client = self.pool.get().await?;
        let tx = client.transaction().await?;

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
                &[
                    &self.coin,
                    &self.chain,
                    &(self.version as i16),
                    &extra_data,
                    &"#created",
                ],
            )
            .await?;

            for (name, query) in self.queries["create"].iter() {
                let st = SystemTime::now();
                tx.query(query, &[]).await?;
                let elapsed = format_duration(st.elapsed().unwrap());
                info!("[db] create.{} executed in {}", name, elapsed);
            }

            let shared = &self.queries["shared"];
            tx.query(&shared["blocksSkippedHeightsFnCreate"], &[])
                .await?;

            self.set_stage("#created").await;

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

            self.set_stage(row.get::<&str, &str>("stage")).await;

            info!("[db] schema verified");
        }

        tx.commit().await?;
        Ok(())
    }

    // Stage: setters/getters
    pub async fn set_stage<S: Into<String>>(&self, name: S) {
        *self.stage.write().await = (name.into(), None);
    }

    // pub async fn set_stage_with_progress<S: Into<String>>(&self, name: S, progress: f64) {
    //     *self.stage.write().await = (name.into(), Some(progress));
    // }

    pub async fn get_stage(&self) -> (String, Option<f64>) {
        let stage = self.stage.read().await;
        (stage.0.clone(), stage.1)
    }

    // This function return skipped block heights. This only relevant for
    // initial sync, when some blocks can be skipped due to indexer restarts.
    // This function executed only once on sync startup and only for initial
    // sync stage.
    pub async fn get_skipped_block_heights(&self, start_height: u32) -> AnyError<Vec<u32>> {
        let mut heights = vec![];

        let client = self.pool.get().await?;
        let it = client
            .query_raw(
                self.queries.get("shared", "blocksSelectSkippedHeights"),
                [start_height as i32].iter().map(|v| v as &dyn ToSql),
            )
            .await?;

        pin_mut!(it);
        while let Some(row) = it.try_next().await? {
            let height: u32 = row.get("height");
            heights.push(height);
        }

        Ok(heights)
    }
}

#[macro_export]
macro_rules! db_add_basic_methods {
    ($name:ident) => {
        impl $name {
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
