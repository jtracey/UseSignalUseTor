use tokio::net::{TcpSocket, TcpStream};
use tokio_socks::tcp::Socks5Stream;

use crate::messenger::error::MessengerError;

/// Parameters used in establishing a connection, optionally through a socks proxy.
/// (Members may be useful elsewhere as well, but that's the primary purpose.)
#[derive(Clone)]
pub struct SocksParams {
    /// Optional socks proxy address.
    pub socks: Option<String>,
    /// The target server or peer.
    pub target: String,
    /// The user who owns this connection.
    pub user: String,
    /// The recipient of messages sent on this connection.
    /// Group for client-server, user for p2p.
    pub recipient: String,
}

pub async fn connect(str_params: &SocksParams) -> Result<TcpStream, MessengerError> {
    match &str_params.socks {
        Some(socks) => {
            let socks_addr = &socks.as_str().parse().unwrap();
            let socks_socket = TcpSocket::new_v4()?;
            socks_socket.set_nodelay(true)?;
            let socks_connection = socks_socket.connect(*socks_addr).await?;
            let target_connection = Socks5Stream::connect_with_password_and_socket(
                socks_connection,
                str_params.target.as_str(),
                &str_params.user,
                &str_params.recipient,
            )
            .await;
            match target_connection {
                Ok(stream) => Ok(stream.into_inner()),
                Err(e) => Err(e.into()),
            }
        }
        None => {
            let addr = &str_params.target.parse().unwrap();
            let socket = TcpSocket::new_v4()?;
            socket.set_nodelay(true)?;
            socket.connect(*addr).await.map_err(|e| e.into())
        }
    }
}
