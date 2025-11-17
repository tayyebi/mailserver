use anyhow::{Context, Result};
use std::path::Path;
use tokio::net::{UnixListener, TcpListener};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{error, info, warn, debug};
use uuid::Uuid;

#[derive(Debug, Clone)]
#[allow(dead_code)] // These variants are part of the milter protocol and may be used in the future
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

    pub async fn run_unix(&self, socket_path: &Path) -> Result<()> {
        info!("Attempting to bind to Unix socket: {:?}", socket_path);
        
        let listener = UnixListener::bind(socket_path)
            .with_context(|| format!("Failed to bind to socket: {:?}. Check permissions and ensure the directory exists.", socket_path))?;

        info!("Milter server listening on Unix socket: {:?}", socket_path);

        let callbacks = std::sync::Arc::new(self.callbacks.clone());
        
        // This loop should never exit - it runs forever accepting connections
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let callbacks = callbacks.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_unix_connection(stream, &*callbacks).await {
                            error!("Error handling connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    // Continue the loop - don't exit on connection errors
                }
            }
        }
    }

    pub async fn run_inet(&self, address: &str) -> Result<()> {
        info!("Attempting to bind to TCP address: {}", address);
        
        let listener = TcpListener::bind(address)
            .await
            .with_context(|| format!("Failed to bind to TCP address: {}. Check if the port is available.", address))?;

        info!("Milter server listening on TCP: {}", address);

        let callbacks = std::sync::Arc::new(self.callbacks.clone());
        
        // This loop should never exit - it runs forever accepting connections
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("New TCP connection from: {}", addr);
                    let callbacks = callbacks.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_tcp_connection(stream, &*callbacks).await {
                            error!("Error handling connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    // Continue the loop - don't exit on connection errors
                }
            }
        }
    }
}

async fn handle_unix_connection<T: MilterCallbacks>(
    mut stream: tokio::net::UnixStream,
    callbacks: &T,
) -> Result<()> {
    handle_connection_impl(stream, callbacks).await
}

async fn handle_tcp_connection<T: MilterCallbacks>(
    mut stream: tokio::net::TcpStream,
    callbacks: &T,
) -> Result<()> {
    handle_connection_impl(stream, callbacks).await
}

async fn handle_connection_impl<T: MilterCallbacks, S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    callbacks: &T,
) -> Result<()> {
    let ctx_id = uuid::Uuid::new_v4().to_string();
    info!(ctx_id = %ctx_id, "New milter connection handler started");

    let mut buffer = vec![0u8; 4096];
    let mut message_buffer = Vec::new();
    let mut current_state = MilterState::WaitingForConnect;
    let mut command_count = 0u64;

    loop {
        debug!(ctx_id = %ctx_id, "Waiting for data from stream");
        match stream.read(&mut buffer).await {
            Ok(0) => {
                info!(
                    ctx_id = %ctx_id,
                    total_commands = command_count,
                    "Connection closed by peer (EOF)"
                );
                let _ = callbacks.close(&ctx_id).await;
                break;
            }
            Ok(n) => {
                debug!(
                    ctx_id = %ctx_id,
                    bytes_read = n,
                    buffer_size = message_buffer.len(),
                    "Read data from stream"
                );
                message_buffer.extend_from_slice(&buffer[..n]);
                
                while let Some((command, data)) = parse_milter_message(&mut message_buffer)? {
                    command_count += 1;
                    let command_char = format!("{}", command as char);
                    debug!(
                        ctx_id = %ctx_id,
                        command = %command_char,
                        command_byte = command,
                        data_size = data.len(),
                        command_number = command_count,
                        "Processing milter command"
                    );
                    
                    match process_milter_command(&ctx_id, command, data, callbacks, &mut current_state).await {
                        Ok(Some(response)) => {
                            debug!(
                                ctx_id = %ctx_id,
                                response_size = response.len(),
                                "Sending response to client"
                            );
                            if let Err(e) = stream.write_all(&response).await {
                                error!(
                                    ctx_id = %ctx_id,
                                    error = %e,
                                    total_commands = command_count,
                                    "Failed to write response"
                                );
                                break;
                            }
                            debug!(ctx_id = %ctx_id, "Response sent successfully");
                        }
                        Ok(None) => {
                            debug!(ctx_id = %ctx_id, "No response needed for command");
                        }
                        Err(e) => {
                            error!(
                                ctx_id = %ctx_id,
                                error = %e,
                                command = %command_char,
                                total_commands = command_count,
                                "Error processing command"
                            );
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                error!(
                    ctx_id = %ctx_id,
                    error = %e,
                    total_commands = command_count,
                    "Failed to read from stream"
                );
                break;
            }
        }
    }

    info!(
        ctx_id = %ctx_id,
        total_commands = command_count,
        "Connection handler finished"
    );
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
        debug!(
            buffer_size = buffer.len(),
            "Buffer too small for milter message header"
        );
        return Ok(None);
    }

    // Read message length (4 bytes, big-endian)
    let length = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    debug!(
        buffer_size = buffer.len(),
        message_length = length,
        "Parsing milter message"
    );
    
    if buffer.len() < 4 + length {
        debug!(
            buffer_size = buffer.len(),
            required_size = 4 + length,
            "Incomplete milter message in buffer"
        );
        return Ok(None);
    }

    // Extract command and data
    let command = buffer[4];
    let data = buffer[5..4 + length].to_vec();
    let original_buffer_size = buffer.len();
    
    // Remove processed message from buffer
    buffer.drain(..4 + length);
    
    debug!(
        command = command,
        command_char = %format!("{}", command as char),
        data_size = data.len(),
        original_buffer_size = original_buffer_size,
        remaining_buffer_size = buffer.len(),
        "Milter message parsed successfully"
    );
    
    Ok(Some((command, data)))
}

async fn process_milter_command<T: MilterCallbacks>(
    ctx_id: &str,
    command: u8,
    data: Vec<u8>,
    callbacks: &T,
    state: &mut MilterState,
) -> Result<Option<Vec<u8>>> {
    let previous_state = format!("{:?}", state);
    match command {
        b'O' => {
            // SMFIC_OPTNEG - Option negotiation
            // The request contains: protocol version (4 bytes), action flags (4 bytes), step flags (4 bytes)
            // The response should contain: protocol version (4 bytes), action flags (4 bytes), step flags (4 bytes)
            debug!(
                ctx_id = %ctx_id,
                data_size = data.len(),
                previous_state = %previous_state,
                "Processing option negotiation"
            );
            
            // Parse the request to see what Postfix is requesting
            if data.len() >= 12 {
                let protocol_version = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                let action_flags = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                let step_flags = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
                debug!(
                    ctx_id = %ctx_id,
                    protocol_version = protocol_version,
                    requested_action_flags = action_flags,
                    requested_step_flags = step_flags,
                    "Parsed option negotiation request"
                );
            }
            
            // Respond with protocol version 6, and accept all actions/steps
            // Protocol version: 6 (0x00000006)
            // Action flags: 0x1F (accept all: add header, change header, add recipient, delete recipient, change body, quarantine)
            // Step flags: 0x1F (accept all steps: connect, helo, mail, rcpt, header, eoh, body, eom)
            let mut response_data = Vec::with_capacity(12);
            response_data.extend_from_slice(&6u32.to_be_bytes()); // Protocol version 6
            response_data.extend_from_slice(&0x1Fu32.to_be_bytes()); // Action flags - accept all
            response_data.extend_from_slice(&0x1Fu32.to_be_bytes()); // Step flags - accept all
            
            *state = MilterState::Connected;
            info!(
                ctx_id = %ctx_id,
                new_state = "Connected",
                "Option negotiation completed"
            );
            Ok(Some(create_response(b'O', &response_data)))
        }
        b'C' => {
            // SMFIC_CONNECT
            debug!(ctx_id = %ctx_id, data_size = data.len(), "Parsing connect data");
            let (hostname, addr) = parse_connect_data(&data)?;
            debug!(
                ctx_id = %ctx_id,
                hostname = %hostname,
                addr = %addr,
                previous_state = %previous_state,
                "Processing connect command"
            );
            
            let result = callbacks.connect(ctx_id, &hostname, &addr).await;
            let result_str = format!("{:?}", result);
            debug!(
                ctx_id = %ctx_id,
                result = %result_str,
                "Connect callback completed"
            );
            
            match result {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'M' => {
            // SMFIC_MAIL - MAIL FROM
            debug!(ctx_id = %ctx_id, data_size = data.len(), "Parsing mail from data");
            let sender = parse_string_data(&data)?;
            debug!(
                ctx_id = %ctx_id,
                sender = %sender,
                previous_state = %previous_state,
                "Processing mail from command"
            );
            *state = MilterState::InMessage;
            
            let result = callbacks.mail_from(ctx_id, &sender).await;
            let result_str = format!("{:?}", result);
            debug!(
                ctx_id = %ctx_id,
                result = %result_str,
                new_state = "InMessage",
                "Mail from callback completed"
            );
            
            match result {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'R' => {
            // SMFIC_RCPT - RCPT TO
            debug!(ctx_id = %ctx_id, data_size = data.len(), "Parsing rcpt to data");
            let recipient = parse_string_data(&data)?;
            debug!(
                ctx_id = %ctx_id,
                recipient = %recipient,
                "Processing rcpt to command"
            );
            
            let result = callbacks.rcpt_to(ctx_id, &recipient).await;
            let result_str = format!("{:?}", result);
            debug!(
                ctx_id = %ctx_id,
                result = %result_str,
                "Rcpt to callback completed"
            );
            
            match result {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'L' => {
            // SMFIC_HEADER
            debug!(ctx_id = %ctx_id, data_size = data.len(), "Parsing header data");
            let (name, value) = parse_header_data(&data)?;
            debug!(
                ctx_id = %ctx_id,
                name = %name,
                value_len = value.len(),
                "Processing header command"
            );
            
            let result = callbacks.header(ctx_id, &name, &value).await;
            let result_str = format!("{:?}", result);
            debug!(
                ctx_id = %ctx_id,
                result = %result_str,
                "Header callback completed"
            );
            
            match result {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'N' => {
            // SMFIC_EOH - End of headers
            debug!(
                ctx_id = %ctx_id,
                previous_state = %previous_state,
                "Processing end of headers command"
            );
            
            let result = callbacks.end_of_headers(ctx_id).await;
            let result_str = format!("{:?}", result);
            debug!(
                ctx_id = %ctx_id,
                result = %result_str,
                "End of headers callback completed"
            );
            
            match result {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'B' => {
            // SMFIC_BODY
            debug!(
                ctx_id = %ctx_id,
                chunk_size = data.len(),
                "Processing body chunk command"
            );
            
            let result = callbacks.body(ctx_id, &data).await;
            let result_str = format!("{:?}", result);
            debug!(
                ctx_id = %ctx_id,
                result = %result_str,
                "Body callback completed"
            );
            
            match result {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                _ => Ok(Some(create_response(b'c', &[]))),
            }
        }
        b'E' => {
            // SMFIC_BODYEOB - End of message
            debug!(
                ctx_id = %ctx_id,
                previous_state = %previous_state,
                "Processing end of message command"
            );
            
            let result = callbacks.end_of_message(ctx_id).await;
            let result_str = format!("{:?}", result);
            debug!(
                ctx_id = %ctx_id,
                result = %result_str,
                "End of message callback completed"
            );
            
            match result {
                MilterResult::Continue => Ok(Some(create_response(b'c', &[]))),
                MilterResult::Accept => Ok(Some(create_response(b'a', &[]))),
                MilterResult::Reject => Ok(Some(create_response(b'r', &[]))),
                MilterResult::TempFail => Ok(Some(create_response(b't', &[]))),
                MilterResult::Discard => Ok(Some(create_response(b'd', &[]))),
                MilterResult::ReplaceBody(body) => {
                    info!(
                        ctx_id = %ctx_id,
                        replacement_body_size = body.len(),
                        "Replacing message body"
                    );
                    Ok(Some(create_response(b'b', &body)))
                }
                _ => Ok(Some(create_response(b'a', &[]))),
            }
        }
        b'Q' => {
            // SMFIC_QUIT
            info!(ctx_id = %ctx_id, "Processing quit command");
            let _ = callbacks.close(ctx_id).await;
            Ok(None)
        }
        _ => {
            warn!(
                ctx_id = %ctx_id,
                command = command,
                command_char = %format!("{}", command as char),
                data_size = data.len(),
                "Unknown milter command received"
            );
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
