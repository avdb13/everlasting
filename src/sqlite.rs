use sled;

fn ok() {
    let ok = sled::open("./db");
}
// use std::sync::mpsc::{self, Sender};

// use color_eyre::Report;
// use rusqlite::Connection;
// use tokio::runtime::Runtime;

// type Job<T> = Box<dyn FnOnce(&mut Option<T>) + Send>;

// pub struct Executor<T> {
//     inner: Sender<Job<T>>,
// }

// impl<T> Executor<T> {
//     fn new(inner: Sender<Job<T>>) -> Self {
//         Self { inner }
//     }
// }

// struct DHT {
//     inner: Executor<Connection>,
// }

// impl DHT {
//     fn new() -> Self {
//         let (rx, tx) = mpsc::channel();
//         let exec = Executor::new(rx);

//         Ok(Self {
//             inner: Connection::open_in_memory()?,
//         })
//     }

//     fn spawn<F>(&self, job: F)
//     where
//         F: FnOnce(&mut Option<T>) + Send + 'static,
//     {
//     }

//     fn run() {}
// }
