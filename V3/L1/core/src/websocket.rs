// WebSocket Subscriptions Server for ZION V3
//
// Provides real-time event streaming for:
// - New blocks
// - Pending transactions (mempool)
// - Address-specific updates
// - Network status changes

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::NodeRuntime;

// ── Subscription Types ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionType {
    /// Subscribe to all new blocks
    NewBlocks,
    /// Subscribe to pending transactions in mempool
    PendingTransactions,
    /// Subscribe to transactions for a specific address
    Address(String),
    /// Subscribe to network status changes
    NetworkStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WsMessage {
    /// Server notification
    Notification {
        subscription: SubscriptionType,
        data: serde_json::Value,
    },
    /// Subscription confirmation
    Subscribed {
        subscription: SubscriptionType,
    },
    /// Unsubscription confirmation
    Unsubscribed {
        subscription: SubscriptionType,
    },
    /// Error message
    Error {
        code: i64,
        message: String,
    },
    /// Ping/Pong for keepalive
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    /// Subscribe to an event type
    Subscribe { subscription: SubscriptionType },
    /// Unsubscribe from an event type
    Unsubscribe { subscription: SubscriptionType },
    /// Ping for keepalive
    Ping,
    /// Pong response
    Pong,
}

// ── Client Session ─────────────────────────────────────────────────────────

struct ClientSession {
    #[allow(dead_code)]
    addr: SocketAddr,
    subscriptions: HashSet<SubscriptionType>,
    /// Channel to send messages to this client
    sender: tokio::sync::mpsc::UnboundedSender<WsMessage>,
}

impl ClientSession {
    fn new(addr: SocketAddr, sender: tokio::sync::mpsc::UnboundedSender<WsMessage>) -> Self {
        Self {
            addr,
            subscriptions: HashSet::new(),
            sender,
        }
    }

    fn has_subscription(&self, sub: &SubscriptionType) -> bool {
        self.subscriptions.contains(sub)
    }

    fn add_subscription(&mut self, sub: SubscriptionType) {
        self.subscriptions.insert(sub);
    }

    fn remove_subscription(&mut self, sub: &SubscriptionType) {
        self.subscriptions.remove(sub);
    }

    /// Send a message to this client's outbound channel.
    ///
    /// The body is synchronous (`UnboundedSender::send` does not block), so this
    /// is intentionally a plain `fn`. It was previously `async`, which meant
    /// callers in synchronous contexts (e.g. `broadcast`) dropped the returned
    /// future without polling it — silently sending nothing.
    fn send(&self, msg: WsMessage) -> Result<()> {
        self.sender
            .send(msg)
            .context("failed to send message to client")?;
        Ok(())
    }
}

// ── WebSocket Server ──────────────────────────────────────────────────────

pub struct WebSocketServer {
    clients: Arc<Mutex<HashMap<SocketAddr, ClientSession>>>,
    runtime: Arc<Mutex<NodeRuntime>>,
}

impl WebSocketServer {
    pub fn new(runtime: Arc<Mutex<NodeRuntime>>) -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            runtime,
        }
    }

    /// Start the WebSocket server
    pub async fn serve(&self, bind_addr: &str) -> Result<()> {
        let listener = TcpListener::bind(bind_addr)
            .await
            .with_context(|| format!("failed to bind WebSocket listener on {bind_addr}"))?;

        println!("WebSocket server listening on {}", bind_addr);

        while let Ok((stream, addr)) = listener.accept().await {
            let clients = Arc::clone(&self.clients);
            let runtime = Arc::clone(&self.runtime);

            tokio::spawn(async move {
                if let Err(e) = Self::handle_connection(stream, addr, clients, runtime).await {
                    eprintln!("WebSocket connection error from {}: {}", addr, e);
                }
            });
        }

        Ok(())
    }

    /// Handle a single WebSocket connection
    async fn handle_connection(
        stream: TcpStream,
        addr: SocketAddr,
        clients: Arc<Mutex<HashMap<SocketAddr, ClientSession>>>,
        runtime: Arc<Mutex<NodeRuntime>>,
    ) -> Result<()> {
        println!("Incoming connection from: {}", addr);

        // Perform WebSocket handshake
        let ws_stream = match tokio_tungstenite::accept_async(stream).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("WebSocket handshake failed from {}: {}", addr, e);
                return Err(e.into());
            }
        };

        println!("WebSocket client connected: {}", addr);

        // Create channels for this client
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<WsMessage>();
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // Register client
        {
            let mut clients_guard = clients
                .lock()
                .map_err(|_| anyhow::anyhow!("clients lock poisoned"))?;
            clients_guard.insert(addr, ClientSession::new(addr, tx));
        }

        // Spawn task to handle outgoing messages
        let _clients_clone = Arc::clone(&clients);
        let (ping_tx, mut ping_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (text_tx, mut text_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let sender_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(text) = text_rx.recv() => {
                        if ws_sender.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                    Some(data) = ping_rx.recv() => {
                        let _ = ws_sender.send(Message::Pong(data)).await;
                    }
                    else => break,
                }
            }
        });

        // Handle incoming messages
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                        Self::handle_client_message(client_msg, addr, &clients, &runtime, &text_tx)
                            .await?;
                    }
                }
                Ok(Message::Ping(data)) => {
                    // Respond with pong via channel
                    let _ = ping_tx.send(data);
                }
                Ok(Message::Close(_)) => {
                    println!("WebSocket client disconnected: {}", addr);
                    break;
                }
                Err(e) => {
                    eprintln!("WebSocket error from {}: {}", addr, e);
                    break;
                }
                _ => {}
            }
        }

        // Cleanup
        {
            let mut clients_guard = clients
                .lock()
                .map_err(|_| anyhow::anyhow!("clients lock poisoned"))?;
            clients_guard.remove(&addr);
        }

        sender_task.abort();

        Ok(())
    }

    /// Handle a client message (subscribe/unsubscribe)
    async fn handle_client_message(
        msg: ClientMessage,
        addr: SocketAddr,
        clients: &Arc<Mutex<HashMap<SocketAddr, ClientSession>>>,
        runtime: &Arc<Mutex<NodeRuntime>>,
        text_tx: &tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        match msg {
            ClientMessage::Subscribe { subscription } => {
                let mut clients_guard = clients
                    .lock()
                    .map_err(|_| anyhow::anyhow!("clients lock poisoned"))?;
                if let Some(session) = clients_guard.get_mut(&addr) {
                    session.add_subscription(subscription.clone());

                    // Send confirmation
                    let confirmation = WsMessage::Subscribed {
                        subscription: subscription.clone(),
                    };
                    let json = serde_json::to_string(&confirmation).unwrap_or_default();
                    let _ = text_tx.send(json);

                    // If subscribing to address, send current state
                    if let SubscriptionType::Address(ref address) = subscription {
                        if let Ok(rt) = runtime.lock() {
                            let balance = rt.utxo_balance(address);
                            let current_state = WsMessage::Notification {
                                subscription: subscription.clone(),
                                data: json!({
                                    "address": address,
                                    "balance_flowers": balance,
                                    "timestamp": chrono::Utc::now().to_rfc3339(),
                                }),
                            };
                            let json = serde_json::to_string(&current_state).unwrap_or_default();
                            let _ = text_tx.send(json);
                        }
                    }
                }
            }
            ClientMessage::Unsubscribe { subscription } => {
                let mut clients_guard = clients
                    .lock()
                    .map_err(|_| anyhow::anyhow!("clients lock poisoned"))?;
                if let Some(session) = clients_guard.get_mut(&addr) {
                    session.remove_subscription(&subscription);

                    let confirmation = WsMessage::Unsubscribed { subscription };
                    let json = serde_json::to_string(&confirmation).unwrap_or_default();
                    let _ = text_tx.send(json);
                }
            }
            ClientMessage::Ping => {
                let json = serde_json::to_string(&WsMessage::Pong).unwrap_or_default();
                let _ = text_tx.send(json);
            }
            ClientMessage::Pong => {
                // Client pong, no action needed
            }
        }
        Ok(())
    }

    /// Broadcast a message to all subscribed clients
    pub fn broadcast(&self, subscription: &SubscriptionType, data: serde_json::Value) {
        let clients_guard = match self.clients.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };

        for (_addr, session) in clients_guard.iter() {
            if session.has_subscription(subscription) {
                let msg = WsMessage::Notification {
                    subscription: subscription.clone(),
                    data: data.clone(),
                };
                // Send asynchronously, ignore errors
                let _ = session.send(msg);
            }
        }
    }

    /// Notify all subscribers when a new block is accepted
    pub fn notify_new_block(&self, block: &crate::AcceptedBlock) {
        let data = json!({
            "height": block.height,
            "hash": block.hash_hex,
            "timestamp": block.timestamp,
            "transaction_count": block.transactions.len(),
            "miner_address": block.miner_address,
            "reward": block.miner_reward_zion,
        });

        self.broadcast(&SubscriptionType::NewBlocks, data);
    }

    /// Notify all subscribers when a new transaction enters mempool
    pub fn notify_pending_transaction(&self, tx: &crate::RuntimeTransaction) {
        let tx_id = tx.tx_id();
        let model = match tx {
            crate::RuntimeTransaction::Account(_) => "account",
            crate::RuntimeTransaction::Utxo(_) => "utxo",
        };
        let data = json!({
            "tx_id": tx_id,
            "model": model,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        self.broadcast(&SubscriptionType::PendingTransactions, data.clone());

        // Also notify address-specific subscribers if we can extract addresses
        if let Some(account_tx) = tx.as_account() {
            let from_data = json!({
                "tx_id": tx_id,
                "model": "account",
                "from": account_tx.from,
                "to": account_tx.to,
                "amount_zion": account_tx.amount_zion.to_string(),
                "fee_zion": account_tx.fee_zion,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });

            // Notify from address subscribers
            let from_subscription = SubscriptionType::Address(account_tx.from.clone());
            self.broadcast(&from_subscription, from_data.clone());

            // Notify to address subscribers
            let to_subscription = SubscriptionType::Address(account_tx.to.clone());
            self.broadcast(&to_subscription, from_data);
        }
    }

    /// Notify address-specific subscribers when their balance changes
    pub fn notify_address_update(&self, address: &str, balance_flowers: u64) {
        let data = json!({
            "address": address,
            "balance_flowers": balance_flowers,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        self.broadcast(&SubscriptionType::Address(address.to_string()), data);
    }

    /// Notify all subscribers when network status changes
    pub fn notify_network_status(&self, height: u64, peer_count: usize, mempool_size: usize) {
        let data = json!({
            "height": height,
            "peer_count": peer_count,
            "mempool_size": mempool_size,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        self.broadcast(&SubscriptionType::NetworkStatus, data);
    }
}
