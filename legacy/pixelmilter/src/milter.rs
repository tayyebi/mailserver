/*
 * Milter Protocol Server Implementation
 * 
 * This module implements the server side of the Sendmail Milter protocol.
 * It handles the low-level communication with the MTA (Mail Transfer Agent, e.g., Postfix).
 * 
 * Key responsibilities:
 * - Defining Milter protocol constants (commands, responses, flags).
 * - Implementing the `MilterServer` to accept connections (TCP or Unix socket).
 * - Managing the connection state machine (Connect -> Helo -> Mail -> Rcpt -> Header -> Body -> EOM).
 * - Parsing Milter commands and dispatching them to the `MilterCallbacks` trait implementation.
 * - Sending appropriate responses back to the MTA.
 */

use anyhow::{Context, Result};
use std::path::Path;
use tokio::net::{UnixListener, TcpListener};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::{error, info, warn, debug};
use anyhow::bail;

// Milter Protocol Commands (SMFIC_ defines)
pub const SMFIC_OPTNEG: u8 = b'O';
pub const SMFIC_CONNECT: u8 = b'C';
pub const SMFIC_HELO: u8 = b'H';
pub const SMFIC_MAIL: u8 = b'M';
pub const SMFIC_RCPT: u8 = b'R';
pub const SMFIC_HEADER: u8 = b'L';
pub const SMFIC_EOH: u8 = b'N';
pub const SMFIC_BODY: u8 = b'B';
pub const SMFIC_EOM: u8 = b'E';
pub const SMFIC_ABORT: u8 = b'A';
pub const SMFIC_QUIT: u8 = b'Q';
pub const SMFIC_MACRO: u8 = b'D';

// Milter Protocol Responses (SMFIR_ defines)
pub const SMFIR_CONTINUE: u8 = b'c';
pub const SMFIR_ACCEPT: u8 = b'a';
pub const SMFIR_REJECT: u8 = b'r';
pub const SMFIR_TEMPFAIL: u8 = b't';
pub const SMFIR_DISCARD: u8 = b'd';
pub const SMFIR_REPLACEBODY: u8 = b'b';

// Milter Protocol Version
pub const SMFI_VERSION: u32 = 6;

// Milter Action Flags (SMFIF_ defines)
pub const SMFIF_ADDHDRS: u32 = 0x01;       // Add headers
pub const SMFIF_CHGHDRS: u32 = 0x02;       // Change headers
pub const SMFIF_ADDRCPT: u32 = 0x04;       // Add recipient
pub const SMFIF_DELRCPT: u32 = 0x08;       // Delete recipient
pub const SMFIF_CHGBODY: u32 = 0x10;       // Change body
pub const SMFIF_QUARANTINE: u32 = 0x20;    // Quarantine

// Milter Step Flags (SMFIP_ defines)
#[allow(dead_code)]
pub const SMFIP_NOCONNECT: u32 = 0x01;     // Skip connect
#[allow(dead_code)]
pub const SMFIP_NOHELO: u32 = 0x02;        // Skip helo
#[allow(dead_code)]
pub const SMFIP_NOMAIL: u32 = 0x04;        // Skip mail from
#[allow(dead_code)]
pub const SMFIP_NORCPT: u32 = 0x08;        // Skip rcpt to
#[allow(dead_code)]
pub const SMFIP_NOBODY: u32 = 0x10;        // Skip body
#[allow(dead_code)]
pub const SMFIP_NOHEADERS: u32 = 0x20;     // Skip headers
#[allow(dead_code)]
pub const SMFIP_NOEOH: u32 = 0x40;         // Skip end of headers
#[allow(dead_code)]
pub const SMFIP_NOURI: u32 = 0x80;         // Skip URI
#[allow(dead_code)]
pub const SMFIP_SKIP: u32 = 0x100;         // Can skip messages
#[allow(dead_code)]
pub const SMFIP_REC_ONLY: u32 = 0x200;     // Recipient list only

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

#[derive(Debug, Clone)]
pub struct MilterOptions {
    pub protocol_version: u32,
    pub action_flags: u32,
    pub step_flags: u32,
}

impl Default for MilterOptions {
    fn default() -> Self {
        Self {
            protocol_version: SMFI_VERSION, // SMFI_VERSION - Current protocol version
            // For now, accept all actions by default
            action_flags: SMFIF_ADDHDRS | SMFIF_CHGHDRS | SMFIF_ADDRCPT | SMFIF_DELRCPT | SMFIF_CHGBODY | SMFIF_QUARANTINE,
            // Process all steps - don't skip any email processing steps
            // Set to 0 to allow all protocol steps (CONNECT, HELO, MAIL, RCPT, HEADERS, BODY, EOM)
            step_flags: 0,
        }
    }
}

#[async_trait::async_trait]
pub trait MilterCallbacks: Send + Sync + Clone {
    fn get_milter_options(&self) -> &MilterOptions;
    async fn connect(&self, ctx_id: &str, hostname: &str, addr: &str) -> MilterResult;
    async fn helo(&self, ctx_id: &str, helo_name: &str) -> MilterResult {
        let _ = (ctx_id, helo_name);
        MilterResult::Continue
    }
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
    async fn macro_data(&self, ctx_id: &str, macro_name: &str, macro_value: &str) -> MilterResult {
        let _ = (ctx_id, macro_name, macro_value);
        MilterResult::Continue
    }
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
        eprintln!("DEBUG: Attempting to bind to TCP address: {}", address);
        
        let listener = TcpListener::bind(address)
            .await
            .with_context(|| format!("Failed to bind to TCP address: {}. Check if the port is available.", address))?;

        info!("Milter server listening on TCP: {}", address);
        eprintln!("DEBUG: Milter server listening on TCP: {}", address);

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
    stream: tokio::net::UnixStream,
    callbacks: &T,
) -> Result<()> {
    handle_connection_impl(stream, callbacks).await
}

async fn handle_tcp_connection<T: MilterCallbacks>(
    stream: tokio::net::TcpStream,
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

    let milter_options = callbacks.get_milter_options();

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
                
                loop {
                    let (command, data) = match parse_milter_message(&mut message_buffer) {
                        Ok(Some((cmd, dta))) => (cmd, dta),
                        Ok(None) => {
                            // Incomplete message, wait for more data
                            break;
                        }
                        Err(e) => {
                            error!(
                                ctx_id = %ctx_id,
                                error = %e,
                                "Failed to parse milter message - sending tempfail and closing connection"
                            );
                            // Send a temporary failure response before closing
                            let _ = stream.write_all(&create_response(SMFIR_TEMPFAIL, &[])).await;
                            return Err(e);
                        }
                    };
                    
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
                    
                    match process_milter_command(&ctx_id, command, data, callbacks, &mut current_state, milter_options).await {
                        Ok((Some(response), should_continue)) => {
                            let response_command = if response.len() > 4 { response[4] as char } else { '?' };
                            debug!(
                                ctx_id = %ctx_id,
                                response_size = response.len(),
                                response_command = %response_command,
                                "Sending response to client"
                            );
                            if response_command == 'b' {
                                info!(
                                    ctx_id = %ctx_id,
                                    response_size = response.len(),
                                    "Sending ReplaceBody response"
                                );
                            }
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
                            if !should_continue {
                                info!(ctx_id = %ctx_id, "Breaking connection handler loop after command");
                                break;
                            }
                        }
                        Ok((None, should_continue)) => {
                            debug!(ctx_id = %ctx_id, "No response needed for command");
                            if !should_continue {
                                info!(ctx_id = %ctx_id, "Breaking connection handler loop after command");
                                break;
                            }
                        }
                        Err(e) => {
                            error!(
                                ctx_id = %ctx_id,
                                error = %e,
                                command = %command_char,
                                total_commands = command_count,
                                "Error processing command - sending tempfail and closing connection"
                            );
                            // Send a temporary failure response before closing
                            let _ = stream.write_all(&create_response(SMFIR_TEMPFAIL, &[])).await;
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

/// Represents the current state of the milter connection for a given message.
/// The milter protocol is stateful, and commands are expected in a specific order.
#[derive(Debug)]
enum MilterState {
    /// Initial state, waiting for the SMFIC_OPTNEG command from the MTA.
    WaitingForConnect,
    /// State after successful option negotiation, waiting for SMFIC_CONNECT, SMFIC_HELO, or SMFIC_MAIL.
    Connected,
    /// State after receiving SMFIC_MAIL, indicating a message is currently being processed.
    /// In this state, SMFIC_RCPT, SMFIC_HEADER, SMFIC_EOH, SMFIC_BODY, SMFIC_EOM are expected.
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

/// Processes a single milter command received from the MTA.
/// This function acts as the core of the milter's state machine,
/// handling commands, invoking callbacks, and generating responses.
/// It also enforces state transitions to ensure protocol adherence.
async fn process_milter_command<T: MilterCallbacks>(
    ctx_id: &str,
    command: u8,
    data: Vec<u8>,
    callbacks: &T,
    state: &mut MilterState,
    options: &MilterOptions,
) -> Result<(Option<Vec<u8>>, bool)> {
    let previous_state = format!("{:?}", state);
    match command {
        SMFIC_OPTNEG => {
            // SMFIC_OPTNEG - Option negotiation. This is the first command sent by the MTA.
            // It allows the MTA and milter to negotiate capabilities (protocol version, actions, steps).
            if !matches!(state, MilterState::WaitingForConnect) {
                warn!(
                    ctx_id = %ctx_id,
                    command_char = %format!("{}", command as char),
                    current_state = %previous_state,
                    "Received OPTNEG in unexpected state"
                );
                // If OPTNEG is received in an unexpected state, send TempFail and continue.
                // This might indicate a protocol error, but we try to recover.
                return Ok((Some(create_response(SMFIR_TEMPFAIL, &[])), true));
            }
            // Respond with the milter's protocol version, action flags, and step flags
            let mut response_data = Vec::with_capacity(12);
            response_data.extend_from_slice(&options.protocol_version.to_be_bytes());
            response_data.extend_from_slice(&options.action_flags.to_be_bytes());
            response_data.extend_from_slice(&options.step_flags.to_be_bytes());
            
            *state = MilterState::Connected;
            info!(
                ctx_id = %ctx_id,
                milter_protocol_version = options.protocol_version,
                milter_action_flags = options.action_flags,
                milter_step_flags = options.step_flags,
                new_state = "Connected",
                "Option negotiation completed"
            );
            Ok((Some(create_response(SMFIC_OPTNEG, &response_data)), true))
        }
        SMFIC_CONNECT => {
            // SMFIC_CONNECT - Connection information. Sent after OPTNEG (if not skipped).
            // Contains hostname and IP address of the connecting client.
            if !matches!(state, MilterState::Connected) {
                warn!(
                    ctx_id = %ctx_id,
                    command_char = %format!("{}", command as char),
                    current_state = %previous_state,
                    "Received CONNECT in unexpected state"
                );
                return Ok((Some(create_response(SMFIR_TEMPFAIL, &[])), true));
            }
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
            
            Ok((milter_response_from_result(result), true))
        }
        SMFIC_HELO => {
            // SMFIC_HELO - HELO or EHLO command from the client.
            if !matches!(state, MilterState::Connected) {
                warn!(
                    ctx_id = %ctx_id,
                    command_char = %format!("{}", command as char),
                    current_state = %previous_state,
                    "Received HELO in unexpected state"
                );
                return Ok((Some(create_response(SMFIR_TEMPFAIL, &[])), true));
            }
            debug!(ctx_id = %ctx_id, data_size = data.len(), "Parsing helo data");
            let helo_name = parse_string_data(&data)?;
            debug!(
                ctx_id = %ctx_id,
                helo_name = %helo_name,
                "Processing helo command"
            );
            
            let result = callbacks.helo(ctx_id, &helo_name).await;
            let result_str = format!("{:?}", result);
            debug!(
                ctx_id = %ctx_id,
                result = %result_str,
                "Helo callback completed"
            );
            
            Ok((milter_response_from_result(result), true))
        }
        SMFIC_MAIL => {
            // SMFIC_MAIL - MAIL FROM command. Initiates a new message.
            if !matches!(state, MilterState::Connected) {
                warn!(
                    ctx_id = %ctx_id,
                    command_char = %format!("{}", command as char),
                    current_state = %previous_state,
                    "Received MAIL FROM in unexpected state"
                );
                return Ok((Some(create_response(SMFIR_TEMPFAIL, &[])), true));
            }
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
            
            Ok((milter_response_from_result(result), true))
        }
        SMFIC_RCPT => {
            // SMFIC_RCPT - RCPT TO command. Specifies a recipient for the current message.
            if !matches!(state, MilterState::InMessage) {
                warn!(
                    ctx_id = %ctx_id,
                    command_char = %format!("{}", command as char),
                    current_state = %previous_state,
                    "Received RCPT TO in unexpected state"
                );
                return Ok((Some(create_response(SMFIR_TEMPFAIL, &[])), true));
            }
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
            
            Ok((milter_response_from_result(result), true))
        }
        SMFIC_HEADER => {
            // SMFIC_HEADER - A message header line. Sent repeatedly for each header.
            if !matches!(state, MilterState::InMessage) {
                warn!(
                    ctx_id = %ctx_id,
                    command_char = %format!("{}", command as char),
                    current_state = %previous_state,
                    "Received HEADER in unexpected state"
                );
                return Ok((Some(create_response(SMFIR_TEMPFAIL, &[])), true));
            }
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
            
            Ok((milter_response_from_result(result), true))
        }
        SMFIC_EOH => {
            // SMFIC_EOH - End of message headers.
            if !matches!(state, MilterState::InMessage) {
                warn!(
                    ctx_id = %ctx_id,
                    command_char = %format!("{}", command as char),
                    current_state = %previous_state,
                    "Received EOH in unexpected state"
                );
                return Ok((Some(create_response(SMFIR_TEMPFAIL, &[])), true));
            }
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
            
            Ok((milter_response_from_result(result), true))
        }
        SMFIC_BODY => {
            // SMFIC_BODY - A chunk of the message body. Sent repeatedly until EOM.
            if !matches!(state, MilterState::InMessage) {
                warn!(
                    ctx_id = %ctx_id,
                    command_char = %format!("{}", command as char),
                    current_state = %previous_state,
                    "Received BODY in unexpected state"
                );
                return Ok((Some(create_response(SMFIR_TEMPFAIL, &[])), true));
            }
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
            
            Ok((milter_response_from_result(result), true))
        }
        SMFIC_EOM => {
            // SMFIC_EOM - End of message. Marks the end of the current message.
            if !matches!(state, MilterState::InMessage) {
                warn!(
                    ctx_id = %ctx_id,
                    command_char = %format!("{}", command as char),
                    current_state = %previous_state,
                    "Received EOM in unexpected state"
                );
                return Ok((Some(create_response(SMFIR_TEMPFAIL, &[])), true));
            }
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
            
            // Reset state to Connected so the connection can handle subsequent messages
            *state = MilterState::Connected;
            Ok((milter_response_from_result(result), true))
        }
        SMFIC_MACRO => {
            // SMFIC_MACRO - Macro definitions. Contains name-value pairs for macros.
            // Macros can be sent at various stages of the protocol.
            debug!(ctx_id = %ctx_id, data_size = data.len(), "Parsing macro data");
            let (macro_name, macro_value) = parse_macro_data(&data)?;
            debug!(
                ctx_id = %ctx_id,
                macro_name = %macro_name,
                macro_value = %macro_value,
                "Processing macro command"
            );

            let result = callbacks.macro_data(ctx_id, &macro_name, &macro_value).await;
            let result_str = format!("{:?}", result);
            debug!(
                ctx_id = %ctx_id,
                result = %result_str,
                "Macro data callback completed"
            );

            Ok((milter_response_from_result(result), true))
        }
        SMFIC_ABORT => {
            // SMFIC_ABORT - Abort current message. The MTA is cancelling the current message.
            // Per milter protocol, reset message state but keep connection open.
            // No response is sent. The MTA may send a new MAIL FROM after ABORT.
            info!(ctx_id = %ctx_id, "Processing abort command - resetting message state");
            *state = MilterState::Connected;
            Ok((None, true))
        }
        SMFIC_QUIT => {
            // SMFIC_QUIT - Quit command. The MTA is closing the connection.
            // No response is sent, and the connection handler loop is terminated.
            info!(ctx_id = %ctx_id, "Processing quit command");
            let _ = callbacks.close(ctx_id).await;
            Ok((None, false))
        }
        _ => {
            warn!(
                ctx_id = %ctx_id,
                command = command,
                command_char = %format!("{}", command as char),
                data_size = data.len(),
                "Unknown milter command received - sending continue"
            );
            // For unknown commands, send a continue response to avoid breaking the connection.
            Ok((Some(create_response(SMFIR_CONTINUE, &[])), true))
        }
    }
}

/// Creates a milter response message.
/// The response format is: 4 bytes length (big-endian) + 1 byte command + data.
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

    let hostname = parts.get(0)
        .map(|s| s.to_string())
        .context("Missing hostname in connect data")?;
    let addr = parts.get(1)
        .map(|s| s.to_string())
        .context("Missing address in connect data")?;
    
    Ok((hostname, addr))
}

fn parse_string_data(data: &[u8]) -> Result<String> {
    let data_str = String::from_utf8_lossy(data);
    if data_str.is_empty() {
        bail!("Empty string data received");
    }
    Ok(data_str.trim_end_matches('\0').to_string())
}

fn parse_header_data(data: &[u8]) -> Result<(String, String)> {
    let data_str = String::from_utf8_lossy(data);
    let parts: Vec<&str> = data_str.splitn(2, '\0').collect();

    let name = parts.get(0)
        .map(|s| s.to_string())
        .context("Missing header name in header data")?;
    let value = parts.get(1)
        .map(|s| s.trim_end_matches('\0').to_string())
        .context("Missing header value in header data")?;
    
    Ok((name, value))
}

fn parse_macro_data(data: &[u8]) -> Result<(String, String)> {
    let data_str = String::from_utf8_lossy(data);
    let parts: Vec<&str> = data_str.splitn(2, '\0').collect();

    let macro_name = parts.get(0)
        .map(|s| s.to_string())
        .context("Missing macro name in macro data")?;
    let macro_value = parts.get(1).unwrap_or(&"").to_string();
    
    Ok((macro_name, macro_value))
}

fn milter_response_from_result(result: MilterResult) -> Option<Vec<u8>> {
    match result {
        MilterResult::Continue => Some(create_response(SMFIR_CONTINUE, &[])),
        MilterResult::Accept => Some(create_response(SMFIR_ACCEPT, &[])),
        MilterResult::Reject => Some(create_response(SMFIR_REJECT, &[])),
        MilterResult::TempFail => Some(create_response(SMFIR_TEMPFAIL, &[])),
        MilterResult::Discard => Some(create_response(SMFIR_DISCARD, &[])),
        MilterResult::ReplaceBody(body) => {
            info!("Sending ReplaceBody response with body size: {}", body.len());
            // Per milter protocol, ReplaceBody must be followed by a final action response.
            // Concatenate the REPLACEBODY and ACCEPT responses so both are sent.
            let mut combined = create_response(SMFIR_REPLACEBODY, &body);
            combined.extend_from_slice(&create_response(SMFIR_ACCEPT, &[]));
            Some(combined)
        },
        // TODO: Handle AddHeader, ChangeHeader, etc. if implemented
        _ => Some(create_response(SMFIR_ACCEPT, &[])), // Default to accept for unhandled results
    }
}
