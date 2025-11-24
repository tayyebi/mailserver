mod config;
mod service;
mod tls;

use anyhow::Result;
use config::Config;
use log::{info, warn, error};
use std::env;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let config_path = env::args().nth(1).unwrap_or_else(|| "honeypot.json".to_string());
    let config = config::load_config(&config_path).await?;

    info!("Starting honeypot with config: {:?}", config);

    let mut handles = vec![];

    for (port, service_type) in config.services {
        let whitelist = config.whitelist.clone();
        let service_type = service_type.clone();
        
        let handle = tokio::spawn(async move {
            if let Err(e) = start_listener(port, service_type, whitelist).await {
                error!("Error on port {}: {}", port, e);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}

async fn start_listener(port: u16, service_type: String, whitelist: Vec<String>) -> Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!("Listening on port {} for service {}", port, service_type);

    loop {
        let (socket, remote_addr) = listener.accept().await?;
        let whitelist = whitelist.clone();
        let service_type = service_type.clone();

        tokio::spawn(async move {
            handle_connection(socket, remote_addr, service_type, whitelist).await;
        });
    }
}

async fn handle_connection(mut socket: TcpStream, remote_addr: SocketAddr, service_type: String, whitelist: Vec<String>) {
    let ip = remote_addr.ip().to_string();
    
    if whitelist.contains(&ip) {
        info!("Allowed connection from {} on port {}", ip, remote_addr.port());
        return;
    }

    warn!("TRAPPED connection from {} on port {} (service: {})", ip, remote_addr.port(), service_type);
    
    if service_type == "https" || service_type == "smtps" {
        match tls::create_tls_acceptor() {
            Ok(acceptor) => {
                match acceptor.accept(socket).await {
                    Ok(mut stream) => {
                        if let Err(e) = service::handle_fake_service(&mut stream, &service_type).await {
                            error!("Error handling TLS connection from {}: {}", ip, e);
                        }
                    }
                    Err(e) => error!("TLS handshake failed from {}: {}", ip, e),
                }
            }
            Err(e) => error!("Failed to create TLS acceptor: {}", e),
        }
    } else {
        if let Err(e) = service::handle_fake_service(&mut socket, &service_type).await {
            error!("Error handling connection from {}: {}", ip, e);
        }
    }
}
