use std::future::Future;
use std::net::Ipv4Addr;
use std::net::SocketAddrV4;

use hyper::service::make_service_fn;
use hyper::service::service_fn;
use hyper::Body;
use hyper::Response;
use miette::Context;
use miette::IntoDiagnostic;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::instrument;

use crate::runner::RunnerEvent;
use crate::server::ServerRead;
use crate::server::ServerWrite;

use super::ServerNotification;

/// Wraps a [`TcpListener`] and creates reader tasks corresponding to connections to a
/// port.
pub struct Server {
    sender: mpsc::Sender<RunnerEvent>,
    receiver: broadcast::Receiver<ServerNotification>,
    http_server: Box<dyn Future<Output = Result<(), hyper::Error>> + Send + Unpin>,
}

impl Server {
    /// Create a new server binding to the given TCP port.
    pub async fn new(
        port: u16,
        sender: mpsc::Sender<RunnerEvent>,
        receiver: broadcast::Receiver<ServerNotification>,
    ) -> miette::Result<Self> {
        let make_svc = make_service_fn(|_addr_stream| async {
            Ok::<_, hyper::Error>(service_fn(|_req| async {
                Ok::<_, hyper::Error>(Response::new(Body::from("Hello World")))
            }))
        });

        let http_server =
            hyper::Server::try_bind(&SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into())
                .into_diagnostic()
                .wrap_err_with(|| format!("Failed to bind TCP port {port}"))?
                .serve(make_svc);

        Ok(Self {
            http_server: Box::new(http_server),
            sender,
            receiver,
        })
    }

    /// Run the server.
    #[instrument(skip_all, name = "server", level = "debug")]
    pub async fn run(mut self) -> miette::Result<()> {
        match self.http_server.await {
            Ok(()) => {}
            Err(err) => {
                tracing::error!("{err:?}");
            }
        }

        Ok(())
    }
}
