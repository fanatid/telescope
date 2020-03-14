use std::pin::Pin;

use futures::stream::Stream;
use futures::task::{Context, Poll};
use tokio::signal::unix;

use crate::logger::error;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShutdownSignal {
    SIGINT,
    SIGTERM,
    SIGHUP,
    SIGQUIT,
}

#[derive(Debug)]
pub struct ShutdownSignals {
    streams: Vec<(unix::Signal, ShutdownSignal)>,
}

impl ShutdownSignals {
    pub fn new() -> ShutdownSignals {
        let sig_map = [
            (unix::SignalKind::interrupt(), ShutdownSignal::SIGINT),
            (unix::SignalKind::terminate(), ShutdownSignal::SIGTERM),
            (unix::SignalKind::hangup(), ShutdownSignal::SIGHUP),
            (unix::SignalKind::quit(), ShutdownSignal::SIGQUIT),
        ];

        let mut streams = Vec::with_capacity(sig_map.len());

        for (kind, sig) in sig_map.iter() {
            match unix::signal(*kind) {
                Ok(stream) => streams.push((stream, *sig)),
                Err(e) => error!("Can not initialize stream handler for {:?} err: {}", sig, e),
            }
        }

        ShutdownSignals { streams }
    }
}

impl Stream for ShutdownSignals {
    type Item = ShutdownSignal;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut finished: usize = 0;
        for idx in 0..self.streams.len() {
            match self.streams[idx].0.poll_recv(cx) {
                Poll::Pending => {}
                Poll::Ready(None) => {
                    finished += 1;
                    if finished == self.streams.len() {
                        return Poll::Ready(None);
                    }
                }
                Poll::Ready(Some(_)) => {
                    let sig = self.streams[idx].1;
                    return Poll::Ready(Some(sig));
                }
            }
        }
        Poll::Pending
    }
}
