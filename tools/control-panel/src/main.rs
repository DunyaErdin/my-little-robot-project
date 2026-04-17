mod model;
mod routes;
mod store;
mod transport;

use std::net::SocketAddr;

use anyhow::Result;
use tokio::net::TcpListener;

use crate::{routes::serve, transport::MockRobotTransport};

#[tokio::main]
async fn main() -> Result<()> {
    let transport = MockRobotTransport::new();

    let addr = SocketAddr::from(([127, 0, 0, 1], 8090));
    let listener = TcpListener::bind(addr).await?;

    println!("robot control panel listening on http://{}", addr);
    serve(listener, transport).await?;

    Ok(())
}
