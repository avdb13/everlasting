use std::{net::SocketAddr, sync::Arc};

use color_eyre::Report;
use futures::stream::FuturesUnordered;
use tokio::net::UdpSocket;

pub type Message = (SocketAddr, Vec<u8>);

pub async fn socket() -> Result<(), Report> {
    let mut f: FuturesUnordered<_> = FuturesUnordered::new();
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:1317").await?);
    let peers: Vec<SocketAddr> = Vec::new();

    let iter = peers.iter().map(|p| f.push(future));

    Ok(())
}
