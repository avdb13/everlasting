use std::io::Error;
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
};

use tokio::sync::mpsc::Sender;

#[derive(Default, Clone)]
pub struct MyWriter {
    buffer: Vec<u8>,
    pub tx: Option<Sender<Vec<String>>>,
}

impl Write for MyWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.append(&mut buf.to_vec());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let s = std::str::from_utf8(&self.buffer).unwrap();
        if let Some(tx) = self.tx {
            tx.send(s.split('\n').map(ToOwned::to_owned).collect());
            self.buffer.clear();
            Ok(())
        } else {
            Err(Error::new(
                std::io::ErrorKind::WouldBlock,
                "receive channel non-existent",
            ))
        }
    }

    fn split(&mut self) -> (Vec<u8>, MyWriter) {}
}
