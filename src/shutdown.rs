use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::StreamExt as _;
use tokio::sync::{Notify, RwLock};
use tokio::time::delay_for;

use crate::logger::info;
use crate::signals::ShutdownSignals;
use crate::EmptyResult;

// Shutdown signal like std::error::Error
#[derive(Debug)]
pub struct ShutdownSignal;

impl fmt::Display for ShutdownSignal {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl std::error::Error for ShutdownSignal {}

// Shutdown
#[derive(Debug)]
pub struct Shutdown {
    notified: RwLock<bool>,
    notify: Notify,
}

impl Shutdown {
    pub fn new() -> Shutdown {
        Shutdown {
            notified: RwLock::new(false),
            notify: Notify::new(),
        }
    }

    pub async fn set(&self) {
        let mut notified = self.notified.write().await;
        *notified = true;
        self.notify.notify();
    }

    pub async fn is_recv(&self) -> Result<(), ShutdownSignal> {
        if *self.notified.read().await {
            Err(ShutdownSignal {})
        } else {
            Ok(())
        }
    }

    pub async fn wait(&self) -> ShutdownSignal {
        if !*self.notified.read().await {
            self.notify.notified().await;
        }

        ShutdownSignal {}
    }

    pub async fn delay_for(&self, duration: Duration) -> EmptyResult {
        tokio::select! {
            _ = delay_for(duration) => Ok(()),
            e = self.wait() => Err(e.into()),
        }
    }

    // pub async fn run_fut<F, T, R, E>(&self, fut: F, transform: T) -> Result<R, E>
    // where
    //     F: std::future::Future<Output = Result<R, E>>,
    //     T: Fn(ShutdownSignal) -> Result<R, E>,
    // {
    //     tokio::select! {
    //         v = fut => v,
    //         e = self.wait() => transform(e),
    //     }
    // }
}

pub fn subscribe() -> Arc<Shutdown> {
    let shutdown = Arc::new(Shutdown::new());

    let notifier = shutdown.clone();
    tokio::spawn(async move {
        let mut s = ShutdownSignals::new();

        if let Some(sig) = s.next().await {
            info!("{:?} received, shutting down...", sig);
            notifier.set().await;

            if let Some(sig) = s.next().await {
                info!("{:?} received, exit now...", sig);
            }
        }

        // In case if we received 2 signals, or tokio::signal return None
        std::process::exit(1);
    });

    shutdown
}
