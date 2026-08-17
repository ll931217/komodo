use std::{sync::Arc, time::Duration};

use anyhow::{Context, anyhow};
use encoding::Decode as _;
use mogh_resolver::HasResponse;
use periphery_client::api;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use transport::channel::channel;
use uuid::Uuid;

use crate::{
  connection::{
    PeripheryConnection, PeripheryConnectionArgs, ResponseChannels,
  },
  state::periphery_connections,
};

pub mod terminal;

#[derive(Debug)]
pub struct PeripheryClient {
  /// Usually the server id
  pub id: String,
  pub responses: Arc<ResponseChannels>,
}

impl PeripheryClient {
  pub async fn new(
    args: PeripheryConnectionArgs<'_>,
    insecure_tls: bool,
  ) -> anyhow::Result<PeripheryClient> {
    let connections = periphery_connections();

    let id = args.id.to_string();

    // Spawn client side connection if one doesn't exist.
    let Some(connection) = connections.get(&id).await else {
      if args.address.is_none() {
        return Err(anyhow!("Server {id} is not connected"));
      }
      return args
        .spawn_client_connection(id.clone(), insecure_tls)
        .await;
    };

    // Ensure the connection args are unchanged.
    if args.matches(&connection.args) {
      return Ok(PeripheryClient {
        id,
        responses: connection.responses.clone(),
      });
    }

    // The args have changed.
    if args.address.is_none() {
      // Periphery -> Core connection
      // Remove this connection, wait and see if client reconnects
      connections.remove(&id).await;
      tokio::time::sleep(Duration::from_millis(500)).await;
      let connection = connections
        .get(&id)
        .await
        .with_context(|| format!("Server {id} is not connected"))?;
      Ok(PeripheryClient {
        id,
        responses: connection.responses.clone(),
      })
    } else {
      // Core -> Periphery connection
      args.spawn_client_connection(id.clone(), insecure_tls).await
    }
  }

  pub async fn cleanup(self) -> Option<Arc<PeripheryConnection>> {
    periphery_connections().remove(&self.id).await
  }

  pub async fn health_check(&self) -> anyhow::Result<()> {
    self.request(api::GetHealth {}).await?;
    Ok(())
  }

  pub async fn request<T>(
    &self,
    request: T,
  ) -> anyhow::Result<T::Response>
  where
    T: std::fmt::Debug + Serialize + HasResponse,
    T::Response: DeserializeOwned,
  {
    self.request_inner(request, None).await
  }

  /// [PeripheryClient::request], but firing `cancel` kills the command
  /// the request is running on the host.
  ///
  /// Periphery keys its cancellation on the request's channel id,
  /// which is generated inside [PeripheryClient::request_inner] and
  /// never escapes it - so before this there was no way for Core to
  /// name an in-flight execution, and the only thing that stopped a
  /// running command was dropping the websocket.
  ///
  /// Cancelling is a second request on the same connection, not a
  /// property of this one. Periphery spawns every request, so it lands
  /// while this one is still blocked in its command.
  pub async fn request_cancellable<T>(
    &self,
    request: T,
    cancel: &CancellationToken,
  ) -> anyhow::Result<T::Response>
  where
    T: std::fmt::Debug + Serialize + HasResponse,
    T::Response: DeserializeOwned,
  {
    // The canceller lives here rather than inside request_inner on
    // purpose. It calls request_inner itself, and a future that
    // spawns a task containing its own type has no finite size - the
    // compiler reports it as "cannot satisfy impl Future: Send",
    // which reads like a trait problem rather than the recursion it
    // is.
    let (id_sender, id_receiver) = oneshot::channel();
    let cancel = cancel.clone();
    let periphery = PeripheryClient {
      id: self.id.clone(),
      responses: self.responses.clone(),
    };
    let canceller = tokio::spawn(async move {
      // Waits for request_inner to report the channel id, which it
      // only does once the request is actually on the wire.
      // Cancelling before that would tell Periphery to stop an
      // execution it has not been given yet.
      let Ok(execution_id) = id_receiver.await else {
        return;
      };
      cancel.cancelled().await;
      if let Err(e) = periphery
        .request(api::CancelExecution { execution_id })
        .await
      {
        // Nearly always benign: the command finished between the
        // token firing and this landing.
        debug!("CancelExecution for {execution_id} | {e:#}");
      }
    });

    let res = self.request_inner(request, Some(id_sender)).await;

    // The request is over either way; a still-waiting canceller would
    // otherwise hold the token for the rest of the process.
    canceller.abort();
    res
  }

  async fn request_inner<T>(
    &self,
    request: T,
    report_id: Option<oneshot::Sender<Uuid>>,
  ) -> anyhow::Result<T::Response>
  where
    T: std::fmt::Debug + Serialize + HasResponse,
    T::Response: DeserializeOwned,
  {
    let connection =
      periphery_connections().get(&self.id).await.with_context(
        || format!("No connection found for server {}", self.id),
      )?;

    // Polls connected 3 times before bailing
    connection.bail_if_not_connected().await?;

    let channel_id = Uuid::new_v4();
    let (response_sender, mut response_receiever) = channel();
    self.responses.insert(channel_id, response_sender).await;

    if let Err(e) = connection
      .sender
      .send_request(
        channel_id,
        &json!({
          "type": T::req_type(),
          "params": request
        }),
      )
      .await
      .context("Failed to send request over channel")
    {
      self.responses.remove(&channel_id).await;
      return Err(e);
    }

    // Only once the request is on the wire: see request_cancellable.
    if let Some(report_id) = report_id {
      let _ = report_id.send(channel_id);
    }

    let res = async {
      // Poll for the associated response
      loop {
        let message = response_receiever
          .recv()
          // Periphery request handler sends pings every 4s
          // *on this channel specifically* so Core knows
          // request is being processed. Hardcoded 11s
          // allows for missed 5s ping due to network reconnect.
          .with_timeout(Duration::from_secs(10))
          .await?;

        let Some(message) = message.decode()? else {
          // Just a ping from periphery request handler
          continue;
        };

        return message.decode();
      }
    }
    .await;

    self.responses.remove(&channel_id).await;

    res
  }
}
