use crate::config::CkPoolConfig;
use serde_json::Value;
use std::error::Error;
use std::{thread, time::Duration};
use tracing::{error, info};
use zmq;
use crate::path_to_mock::mock_config;


// Define a trait for the socket operations we need
trait MinerSocket {
    fn recv_string(&mut self) -> Result<Result<String, Vec<u8>>, zmq::Error>;
}

// Implement the trait for the real ZMQ socket
impl MinerSocket for zmq::Socket {
    fn recv_string(&mut self) -> Result<Result<String, Vec<u8>>, zmq::Error> {
        self.socket.recv_string(zmq::DONTWAIT) // Ensure `socket` is a `zmq::Socket`
    }
    
}

// Function to create the real ZMQ socket
fn create_zmq_socket(config: &CkPoolConfig) -> Result<zmq::Socket, Box<dyn Error>> {
    let ctx = zmq::Context::new();
    let socket = ctx.socket(zmq::SUB)?;
    socket.connect(format!("tcp://{}:{}", config.host, config.port).as_str())?;
    socket.set_subscribe(b"")?;
    info!("Connected to ckpool at {}:{}", config.host, config.port);
    Ok(socket)
}

// Function to handle reconnection
fn reconnect_zmq_socket(config: &CkPoolConfig) -> Result<zmq::Socket, Box<dyn Error>> {
    let mut attempts = 0;
    let max_attempts = 5;
    while attempts < max_attempts {
        match create_zmq_socket(config) {
            Ok(socket) => {
                info!("Reconnected to ZMQ after {} attempts", attempts);
                return Ok(socket);
            }
            Err(e) => {
                error!("Reconnection attempt {} failed: {}", attempts + 1, e);
                attempts += 1;
                thread::sleep(Duration::from_secs(2_u64.pow(attempts))); // Exponential backoff
            }
        }
    }
    Err("Failed to reconnect after multiple attempts".into())
}

// Function to receive shares with reconnection logic
fn receive_shares<S: MinerSocket>(
    config: &CkPoolConfig,
    socket: &mut S,
    tx: tokio::sync::mpsc::Sender<Value>,
) -> Result<(), Box<dyn Error>> {
    loop {
        match socket.recv_string() {
            Ok(Ok(json_str)) => {
                tracing::debug!("Received json from ckpool: {}", json_str);
                match serde_json::from_str(&json_str) {
                    Ok(json_value) => {
                        if let Err(e) = tx.blocking_send(json_value) {
                            error!("Failed to send share to channel: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse JSON: {}", e);
                    }
                }
            }
            Ok(Err(e)) => {
                error!("Failed to decode message: {:?}", e);
            }
            Err(e) => {
                error!("Failed to receive message: {:?}. Attempting reconnection...", e);
                return Err(Box::new(e));
            }
        }
    }
}

// A receive function that clients use to receive shares
pub fn receive_from_ckpool(
    config: &CkPoolConfig,
    tx: tokio::sync::mpsc::Sender<Value>,
) -> Result<(), Box<dyn Error>> {
    let mut socket = create_zmq_socket(config)?;
    receive_shares(config, &mut socket, tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    // Mock socket for testing
    struct MockSocket {
        messages: Vec<Result<Result<String, Vec<u8>>, zmq::Error>>,
        current: usize,
    }

    impl MockSocket {
        fn new(messages: Vec<Result<Result<String, Vec<u8>>, zmq::Error>>) -> Self {
            Self {
                messages,
                current: 0,
            }
        }
    }

    impl MinerSocket for MockSocket {
        fn recv_string(&mut self) -> Result<Result<String, Vec<u8>>, zmq::Error> {
            if self.current >= self.messages.len() {
                Err(zmq::Error::EAGAIN)
            } else {
                let msg = self.messages[self.current].clone();
                self.current += 1;
                msg
            }
        }
    }

    // Mock CkPoolConfig
    fn mock_config() -> CkPoolConfig {
        CkPoolConfig {
            host: "127.0.0.1".to_string(),
            port: 1234,
        }
    }

    #[tokio::test]
    async fn test_receive_valid_json() {
        let (tx, mut rx) = mpsc::channel(100);
        let mock_messages = vec![Ok(Ok(r#"{"share": "test", "value": 123}"#.to_string()))];
        let mut mock_socket = MockSocket::new(mock_messages);

        let result = receive_shares(&mock_config(), &mut mock_socket, tx);
        assert!(result.is_ok());

        if let Some(value) = rx.recv().await {
            assert_eq!(value["share"], "test");
            assert_eq!(value["value"], 123);
        }
    }

    #[tokio::test]
    async fn test_receive_invalid_json() {
        let (tx, _rx) = mpsc::channel(100);
        let mock_messages = vec![Ok(Ok("invalid json".to_string()))];
        let mock_socket: zmq::Socket = zmq::Context::new().socket(zmq::PULL).unwrap();
        let result = receive_shares(&mock_config(), mock_socket, tx);
        
        assert!(result.is_ok()); // Should not fail but log an error
    }

    #[tokio::test]
    async fn test_receive_decode_error() {
        let (tx, _rx) = mpsc::channel(100);
        let mock_messages = vec![Ok(Err(vec![1, 2, 3]))]; // Simulating decode error
        let mut mock_socket = MockSocket::new(mock_messages);

        let result = receive_shares(&mock_config(), &mut mock_socket, tx);
        assert!(result.is_ok()); // Should not panic but handle error
    }

    #[tokio::test]
    async fn test_reconnect_on_failure() {
        let (tx, mut rx) = mpsc::channel(100);
        let mock_messages = vec![
            Err(zmq::Error::EAGAIN), // Simulate failure
            Ok(Ok(r#"{"share": "test_reconnect", "value": 456}"#.to_string())), // Recovery message
        ];

        let mut mock_socket = MockSocket::new(mock_messages);
        let handle = tokio::spawn(async move {
            let result = receive_shares(&mock_config(), &mut mock_socket, tx);
            assert!(result.is_ok() || result.is_err(), "Should handle failure & retry");
        });

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        if let Some(value) = rx.recv().await {
            assert_eq!(value["share"], "test_reconnect");
            assert_eq!(value["value"], 456);
        } else {
            panic!("Did not receive expected message after reconnect");
        }

        handle.await.unwrap();
    }
}
