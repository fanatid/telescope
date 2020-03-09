use std::sync::Arc;

use crate::shutdown::Shutdown;
use crate::AppFutFromArgs;

#[derive(Debug)]
pub struct Client {
    shutdown: Arc<Shutdown>,
}

impl Client {
    pub fn from_args(_shutdown: Arc<Shutdown>, _args: &clap::ArgMatches<'_>) -> AppFutFromArgs {
        Ok(Box::pin(async move { Ok(()) }))
    }
}
