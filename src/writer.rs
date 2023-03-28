use std::{io::Write, sync::Arc};
use tokio::sync::mpsc::UnboundedSender as Sender;
use tracing::info;

#[derive(Clone)]
pub struct Writer {
    tx: Sender<String>,
}

impl Writer {
    pub fn new(tx: Sender<String>) -> Self {
        Self { tx }
    }
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let s = std::str::from_utf8(buf).unwrap();
        info!("wrote {s}");
        self.tx.send(s.to_owned()).unwrap();

        Ok(s.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
