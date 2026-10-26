use std::net::SocketAddr;

use color_eyre::Result;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::TcpListener;

pub fn bind(port: u16) -> Result<TcpListener> {
    let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_only_v6(false)?;
    socket.set_reuse_address(true)?;

    let addr_str = format!("[::]:{port}");
    tracing::info!("Binding {addr_str}");
    let addr: SocketAddr = addr_str.parse()?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;

    let std_listener: std::net::TcpListener = socket.into();
    std_listener.set_nonblocking(true)?;

    Ok(TcpListener::from_std(std_listener)?)
}
