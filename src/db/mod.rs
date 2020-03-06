use std::time::Duration;

use bb8::Pool;
use bb8_postgres::PostgresConnectionManager;
use humantime::parse_duration;
use regex::Regex;
use tokio_postgres::{Config, NoTls};

use self::error::DBError;
use crate::shutdown::Shutdown;
use crate::AnyError;

mod error;

#[derive(Debug)]
pub struct DB {
    pool: Pool<PostgresConnectionManager<NoTls>>,
}

impl DB {
    pub fn from_args<'a>(args: &clap::ArgMatches<'a>) -> DB {
        // unwrap is safe because values validated in args
        let conn_str = args.value_of("postgres").unwrap();
        let conn_timeout = args.value_of("postgres_connection_timeout").unwrap();
        let pool_size = args.value_of("postgres_pool_size").unwrap();

        DB::new(
            conn_str.parse::<Config>().unwrap(),
            parse_duration(conn_timeout).unwrap(),
            pool_size.parse().unwrap(),
        )
    }

    pub fn new(conf: Config, conn_timeout: Duration, pool_size: u32) -> DB {
        let manager = PostgresConnectionManager::new(conf, NoTls);
        let pool = Pool::builder()
            .max_size(pool_size)
            .min_idle(None)
            .max_lifetime(None)
            .idle_timeout(Some(Duration::from_secs(10 * 60)))
            .connection_timeout(conn_timeout)
            // .build(manager) -- will check nothing, because minimum number of idle connections is 0
            .build_unchecked(manager);

        DB { pool }
    }

    pub async fn connect(&self, shutdown: &mut Shutdown) -> AnyError<()> {
        tokio::select! {
            v = self.validate_version() => v,
            e = shutdown.wait() => Err(e.into()),
        }
    }

    async fn validate_version(&self) -> AnyError<()> {
        let version_query = "SELECT current_setting('server_version') AS version;";
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
