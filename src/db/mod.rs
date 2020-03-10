use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bb8::Pool;
use bb8_postgres::PostgresConnectionManager;
use humantime::parse_duration;
use regex::Regex;
use tokio_postgres::{Config, NoTls};

use self::error::DBError;
use self::yesql::parse_sql;
use crate::shutdown::Shutdown;
use crate::AnyError;

mod error;
mod yesql;

static BASE_QUERIES: &[(&str, &str)] = &[("base", include_str!("./base.sql"))];

#[derive(Debug)]
pub struct DB {
    pool: Pool<PostgresConnectionManager<NoTls>>,
    queries: HashMap<String, HashMap<String, String>>,
}

impl DB {
    pub fn from_args<'a>(args: &clap::ArgMatches<'a>, queries: &[(&str, &str)]) -> DB {
        // unwrap is safe because values validated in args
        let conn_str = args.value_of("postgres").unwrap();
        let conn_timeout = args.value_of("postgres_connection_timeout").unwrap();
        let pool_size = args.value_of("postgres_pool_size").unwrap();
        let schema = args.value_of("postgres_schema").unwrap();

        DB::new(
            conn_str.parse::<Config>().unwrap(),
            parse_duration(conn_timeout).unwrap(),
            pool_size.parse().unwrap(),
            schema,
            queries,
        )
    }

    pub fn new(
        conf: Config,
        conn_timeout: Duration,
        pool_size: u32,
        schema: &str,
        app_queries: &[(&str, &str)],
    ) -> DB {
        let manager = PostgresConnectionManager::new(conf, NoTls);
        let pool = Pool::builder()
            .max_size(pool_size)
            .min_idle(None)
            .max_lifetime(None)
            .idle_timeout(Some(Duration::from_secs(10 * 60)))
            .connection_timeout(conn_timeout)
            // .build(manager) -- will check nothing, because minimum number of idle connections is 0
            .build_unchecked(manager);

        // Parse queries in text to Map
        let mut queries = HashMap::new();
        for (name, text) in BASE_QUERIES.iter().chain(app_queries.iter()) {
            let mut group = parse_sql(text);
            for (_, query) in group.iter_mut() {
                *query = query.replace("{SCHEMA}", schema);
            }

            if queries.insert((*name).to_owned(), group).is_some() {
                panic!(format!("Duplicate queries for group: {}", name));
            }
        }

        DB { pool, queries }
    }

    pub async fn connect(&self, shutdown: &Arc<Shutdown>) -> AnyError<()> {
        tokio::select! {
            v = self.validate_version() => v,
            e = shutdown.wait() => Err(e.into()),
        }
    }

    async fn validate_version(&self) -> AnyError<()> {
        let version_query = self.queries["base"]["selectVersion"].as_str();
        let version_required = "12.*";
        let version_re = Regex::new(r#"12\..*"#).unwrap();

        let conn = self.pool.get().await?;
        let row = conn.query_one(version_query, &[]).await?;
        let version: &str = row.get("version");

        if version_re.is_match(version) {
            Ok(())
        } else {
            Err(DBError::InvalidVersion(version.to_owned(), version_required.to_owned()).into())
        }
    }
}
