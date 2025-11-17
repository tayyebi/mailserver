use anyhow::{Context, Result};
use std::path::Path;
use tokio::net::UnixListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{error, info, warn, debug};

#[derive(Debug, Clone)]
pub enum MilterResult {
    Continue,
    Accept,
    Reject,
    Discard,
    TempFail,
    ReplaceBody(Vec<u8>),
    AddHeader(String, String),
    ChangeHeader(String, String),
}

#[async_trait::async_trait]
pub trait MilterCallbacks: Send + Sync + Clone {
    async fn connect(&self, ctx_id: &str, hostname: &str, addr: &str) -> MilterResult;
    async fn mail_from(&self, ctx_id: &str, sender: &str) -> MilterResult;
    async fn rcpt_to(&self, ctx_id: &str, recipient: &str) -> MilterResult {
        let _ = (ctx_id, recipient);
        MilterResult::Continue
    }
    async fn header(&self, ctx_id: &str, name: &str, value: &str) -> MilterResult;
    async fn end_of_headers(&self, ctx_id: &str) -> MilterResult;
    async fn body(&self, ctx_id: &str, chunk: &[u8]) -> MilterResult;
    async fn end_of_message(&self, ctx_id: &str) -> MilterResult;
    async fn close(&self, ctx_id: &str) -> MilterResult;
}

pub struct MilterServer<T: MilterCallbacks + 'static> {
    callbacks: T,
}

impl<T: MilterCallbacks + 'static> MilterServer<T> {
    pub fn new(callbacks: T) -> Self {
        Self { callbacks }
    }

    pub async fn run(&self, socket_path: &Path) -> Result<()> {
        let listener = UnixListener::bind(socket_path)
            .with_context(|| format!("Failed to bind to socket: {:?}", socket_path))?;

        info!("Milter server listening on: {:?}", socket_path);

        let callbacks = std::sync::Arc::new(self.callbacks.clone());
        
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let callbacks = callbacks.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, &*callbacks).await {
                            error!("Error handling connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }
}

async fn handle_connection<T: MilterCallbacks>(
    mut stream: tokio::net::UnixStream,
    callbacks: &T,
) -> Result<()> {
    let ctx_id = uuid::Uuid::new_v4().to_string();
    debug!(ctx_id = %ctx_id, "New milter connection");

    let mut buffer = vec![0u8; 4096];
    let mut message_buffer = Vec::new();
    let mut current_state = MilterState::WaitingForConnect;

    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => {
                debug!(ctx_id = %ctx_id, "Connection closed");
                let _ = callbacks.close(&ctx_id).await;
                break;
            }
            Ok(n) => {
                message_buffer.extend_from_slice(&buffer[..n]);
                
                while let Some((command, data)) = parse_milter_message(&mut message_buffer)? {
                    match process_milter_command(&ctx_id, command, data, callbacks, &mut current_state).await {
                        Ok(Some(response)) => {
                            if let Err(e) = stream.write_all(&response).await {
                                error!(ctx_id = %ctx_id, error = %e, "Failed to write response");
                                break;
                            }
                        }
                        Ok(None) => {
                            // No response needed
                        }
                        Err(e) => {
                            error!(ctx_id = %ctx_id, error = %e, "Error processing command");
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                error!(ctx_id = %ctx_id, error = %e, "Failed to read from stream");
                break;
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
enum MilterState {
    WaitingForConnect,
    Connected,
    InMessage,
}

fn parse_milter_message(buffer: &mut Vec<u8>) -> Result<Option<(u8, Vec<u8>)>> {
    if buffer.len() < 5 {
        return Ok(None);
    }

    // Read message length (4 bytes, big-endian)
    let length = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    
    if buffer.len() < 4 + length {
        return Ok(None);
    }

    // Extract command and data
    let command = buffer[4];
    let data = buffer[5..4 + length].to_vec();
    
    // Remove processed message from buffer
    buffer.drain(..4 + length);
    
    Ok(Some((command, data)))
}

async fn process_milter_command<T: MilterCallbacks>(
    ctx_id: &str,
    command: u8,
    data: Vec<u8>,
    callbacks: &T,
    state: &mut MilterState,
) -> Result<Option<Vec<u8>>> {
    match command {
        b'O' => {
            // SMFIC_OPTNEG - Option negotiation
            debug!(ctx_id = %ctx_id, "Option negotiation");
            *state = MilterState::Connected;
            Ok(Some(create_response(b'O', &[0, 0, 0, 0, 0, 0])))
        }
        b'C' => {
            // SMFIC_CONNECT
            let (hostname, addr) = parse_connect_data(&data)?;
            debug!(ctx_id = %ctx_id, hostname = %hostname, addr = %addr, "Connect");
            
            match callbacks.connect(ctx_id, &hostname, &addr).await {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'M' => {
            // SMFIC_MAIL - MAIL FROM
            let sender = parse_string_data(&data)?;
            debug!(ctx_id = %ctx_id, sender = %sender, "Mail from");
            *state = MilterState::InMessage;
            
            match callbacks.mail_from(ctx_id, &sender).await {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'R' => {
            // SMFIC_RCPT - RCPT TO
            let recipient = parse_string_data(&data)?;
            debug!(ctx_id = %ctx_id, recipient = %recipient, "Rcpt to");
            
            match callbacks.rcpt_to(ctx_id, &recipient).await {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'L' => {
            // SMFIC_HEADER
            let (name, value) = parse_header_data(&data)?;
            debug!(ctx_id = %ctx_id, name = %name, value = %value, "Header");
            
            match callbacks.header(ctx_id, &name, &value).await {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'N' => {
            // SMFIC_EOH - End of headers
            debug!(ctx_id = %ctx_id, "End of headers");
            
            match callbacks.end_of_headers(ctx_id).await {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'B' => {
            // SMFIC_BODY
            debug!(ctx_id = %ctx_id, size = data.len(), "Body chunk");
            
            match callbacks.body(ctx_id, &data).await {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'E' => {
            // SMFIC_BODYEOB - End of message
            debug!(ctx_id = %ctx_id, "End of message");
            
            match callbacks.end_of_message(ctx_id).await {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                MilterResult::Discard => Ok(Some(create_response(b'd', &[]))),
                MilterResult::ReplaceBody(body) => {
                    // Send replace body response
                    Ok(Some(create_response(b'b', &body)))
                }
                _ => Ok(Some(create_response(b'a', &[]))),
            }
        }
        b'Q' => {
            // SMFIC_QUIT
            debug!(ctx_id = %ctx_id, "Quit");
            let _ = callbacks.close(ctx_id).await;
            Ok(None)
        }
        _ => {
            warn!(ctx_id = %ctx_id, command = command, "Unknown milter command");
            Ok(Some(create_response(b'c', &[])))
        }
    }
}

fn create_response(command: u8, data: &[u8]) -> Vec<u8> {
    let length = (data.len() + 1) as u32;
    let mut response = Vec::with_capacity(4 + data.len() + 1);
    
    // Add length (big-endian)
    response.extend_from_slice(&length.to_be_bytes());
    // Add command
    response.push(command);
    // Add data
    response.extend_from_slice(data);
    
    response
}

fn parse_connect_data(data: &[u8]) -> Result<(String, String)> {
    let data_str = String::from_utf8_lossy(data);
    let parts: Vec<&str> = data_str.split('\0').collect();
    
    let hostname = parts.get(0).unwrap_or(&"unknown").to_string();
    let addr = parts.get(1).unwrap_or(&"unknown").to_string();
    
    Ok((hostname, addr))
}

fn parse_string_data(data: &[u8]) -> Result<String> {
    let data_str = String::from_utf8_lossy(data);
    Ok(data_str.trim_end_matches('\0').to_string())
}

fn parse_header_data(data: &[u8]) -> Result<(String, String)> {
    let data_str = String::from_utf8_lossy(data);
    let parts: Vec<&str> = data_str.split('\0').collect();
    
    let name = parts.get(0).unwrap_or(&"").to_string();
    let value = parts.get(1).unwrap_or(&"").to_string();
    
    Ok((name, value))
}
