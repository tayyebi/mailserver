use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use anyhow::Result;
use log::info;

pub async fn handle_fake_service<S>(socket: &mut S, service_type: &str) -> Result<()> 
where S: AsyncRead + AsyncWrite + Unpin
{
    match service_type {
        "smtp" | "smtps" => fake_smtp(socket).await,
        "http" | "https" => fake_http(socket).await,
        "ssh" => fake_ssh(socket).await,
        _ => fake_generic(socket).await,
    }
}

async fn fake_smtp<S>(socket: &mut S) -> Result<()> 
where S: AsyncRead + AsyncWrite + Unpin
{
    // Fake SMTP Banner
    socket.write_all(b"220 mail.example.com ESMTP Postfix\r\n").await?;
    let mut buf = [0; 1024];
    loop {
        let n = socket.read(&mut buf).await?;
        if n == 0 { break; }
        let input = String::from_utf8_lossy(&buf[..n]);
        info!("SMTP received: {:?}", input.trim());
        
        if input.to_uppercase().starts_with("HELO") || input.to_uppercase().starts_with("EHLO") {
            socket.write_all(b"250 Hello\r\n").await?;
        } else if input.to_uppercase().starts_with("MAIL FROM") {
            socket.write_all(b"250 Ok\r\n").await?;
        } else if input.to_uppercase().starts_with("RCPT TO") {
            socket.write_all(b"250 Ok\r\n").await?;
        } else if input.to_uppercase().starts_with("DATA") {
            socket.write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n").await?;
        } else if input.to_uppercase().starts_with("QUIT") {
            socket.write_all(b"221 Bye\r\n").await?;
            break;
        } else {
            socket.write_all(b"500 Command not recognized\r\n").await?;
        }
    }
    Ok(())
}

async fn fake_http<S>(socket: &mut S) -> Result<()> 
where S: AsyncRead + AsyncWrite + Unpin
{
    let mut buf = [0; 1024];
    let n = socket.read(&mut buf).await?;
    if n > 0 {
        let input = String::from_utf8_lossy(&buf[..n]);
        info!("HTTP received: {:?}", input.trim());
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n<html><body><h1>It works!</h1></body></html>";
        socket.write_all(response.as_bytes()).await?;
    }
    Ok(())
}

async fn fake_ssh<S>(socket: &mut S) -> Result<()> 
where S: AsyncRead + AsyncWrite + Unpin
{
    // Fake SSH Banner
    socket.write_all(b"SSH-2.0-OpenSSH_8.2p1 Ubuntu-4ubuntu0.5\r\n").await?;
    let mut buf = [0; 1024];
    let n = socket.read(&mut buf).await?;
    if n > 0 {
        let input = String::from_utf8_lossy(&buf[..n]);
        info!("SSH received: {:?}", input.trim());
        // SSH handshake is complex, we just log the initial packet and close or hang
    }
    Ok(())
}

async fn fake_generic<S>(socket: &mut S) -> Result<()> 
where S: AsyncRead + AsyncWrite + Unpin
{
    socket.write_all(b"Hello\r\n").await?;
    let mut buf = [0; 1024];
    loop {
        let n = socket.read(&mut buf).await?;
        if n == 0 { break; }
        let input = String::from_utf8_lossy(&buf[..n]);
        info!("Generic received: {:?}", input.trim());
        socket.write_all(b"Echo: ").await?;
        socket.write_all(&buf[..n]).await?;
    }
    Ok(())
}
