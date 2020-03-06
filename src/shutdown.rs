use std::fmt;

use futures::stream::StreamExt as _;
use log::info;
use tokio::sync::broadcast;

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
    received: bool,
    tx: broadcast::Sender<()>,
    rx: broadcast::Receiver<()>,
}

impl Shutdown {
    pub fn new() -> Shutdown {
        let (tx, rx) = broadcast::channel::<()>(1);
        Shutdown {
            received: false,
            tx,
            rx,
        }
    }

    pub fn set(&mut self) {
        // unwrap is safe because `self` have Receiver for this Sender
        self.tx.send(()).unwrap();
    }

    pub fn is_recv(&mut self) -> bool {
        if !self.received {
            match self.rx.try_recv() {
                Ok(_) => {
                    self.received = true;
                }
                Err(broadcast::TryRecvError::Empty) => {}
                Err(err) => panic!("Shutdown channel error: {:?}", err),
            }
        }

        self.received
    }

    pub async fn wait(&mut self) -> ShutdownSignal {
        if !self.received {
            match self.rx.recv().await {
                Ok(_) => {
                    self.received = true;
                }
                Err(err) => panic!("Shutdown channel error: {:?}", err),
            }
        }

        ShutdownSignal {}
    }
}

impl Clone for Shutdown {
    fn clone(&self) -> Shutdown {
        Shutdown {
            received: self.received,
            tx: self.tx.clone(),
            rx: self.tx.subscribe(),
        }
    }
}

pub fn subscribe() -> Shutdown {
    let shutdown = Shutdown::new();
    let mut notifier = shutdown.clone();

    tokio::spawn(async move {
        let mut s = ShutdownSignals::new();

        if let Some(sig) = s.next().await {
            info!("{:?} received, shutting down...", sig);
            notifier.set();

            if let Some(sig) = s.next().await {
                info!("{:?} received, exit now...", sig);
            }
        }

        // In case if we received 2 signals, or tokio::signal return None
        std::process::exit(1);
    });

    shutdown
}
