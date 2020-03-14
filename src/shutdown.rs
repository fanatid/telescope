use std::fmt;
use std::sync::Arc;

use futures::stream::StreamExt as _;
use tokio::sync::{Notify, RwLock};

use crate::logger::info;
use crate::signals::ShutdownSignals;

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
