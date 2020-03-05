use crate::shutdown::Shutdown;
use crate::AnyError;

pub async fn main(_shutdown: Shutdown, _args: &clap::ArgMatches<'_>) -> AnyError<()> {
    panic!("TODO");
}
