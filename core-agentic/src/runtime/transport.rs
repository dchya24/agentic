use super::protocol::{ProtocolEvent, ProtocolRequest};
use std::io;
use std::sync::{mpsc, Mutex};

pub trait Transport: Send + Sync + 'static {
    fn read_request(&self) -> io::Result<Option<ProtocolRequest>>;
    fn write_event(&self, event: &ProtocolEvent) -> io::Result<()>;
}

pub struct MemoryTransport {
    request_rx: Mutex<mpsc::Receiver<ProtocolRequest>>,
    event_tx: mpsc::Sender<ProtocolEvent>,
}

impl MemoryTransport {
    pub fn new(
        request_rx: mpsc::Receiver<ProtocolRequest>,
        event_tx: mpsc::Sender<ProtocolEvent>,
    ) -> Self {
        Self {
            request_rx: Mutex::new(request_rx),
            event_tx,
        }
    }
}

impl Transport for MemoryTransport {
    fn read_request(&self) -> io::Result<Option<ProtocolRequest>> {
        match self.request_rx.lock().unwrap().recv() {
            Ok(request) => Ok(Some(request)),
            Err(_) => Ok(None),
        }
    }

    fn write_event(&self, event: &ProtocolEvent) -> io::Result<()> {
        self.event_tx
            .send(event.clone())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "event receiver closed"))
    }
}
