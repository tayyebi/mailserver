# Nginx Reverse Proxy

Dedicated reverse proxy for the mail server stack, providing unified access to admin panel and pixel server services.

## Features

- **Standard Ports**: HTTP (80) and HTTPS (443)
- **Automatic HTTPS Redirect**: All HTTP traffic redirected to HTTPS
- **Self-Signed SSL**: Certificates generated automatically during build
- **Path-Based Routing**: Routes requests to appropriate backend services
- **Security Headers**: X-Frame-Options, X-Content-Type-Options, X-XSS-Protection

## Routes

### Admin Panel
- **Path**: `/admin`
- **Backend**: `mailserver_admin` container (port 80)
- **Access**: `https://your-server/admin`

### Reports API
- **Path**: `/admin/reports`
- **Backend**: `pixelserver` container (port 8444)
- **Access**: `https://your-server/admin/reports`

### Pixel Tracking
- **Path**: `/pixel`
- **Backend**: `pixelserver` container (port 8443)
- **Access**: `https://your-server/pixel?id=...`

### Health Check
- **Path**: `/health`
- **Response**: `OK`
- **Access**: `https://your-server/health`

## SSL Configuration

Self-signed certificates are automatically generated during container build:
- **Certificate**: `/etc/nginx/ssl/nginx-selfsigned.crt`
- **Private Key**: `/etc/nginx/ssl/nginx-selfsigned.key`
- **Validity**: 365 days
- **Key Size**: 2048 bits RSA

### Using Custom Certificates

To use your own SSL certificates:

1. Mount your certificates as volumes in `docker-compose.yml`:
```yaml
volumes:
  - ./path/to/your/cert.crt:/etc/nginx/ssl/nginx-selfsigned.crt:ro
  - ./path/to/your/key.key:/etc/nginx/ssl/nginx-selfsigned.key:ro
```

2. Restart the proxy:
```bash
docker-compose restart nginx-proxy
```

## Configuration Files

- **nginx.conf**: Main Nginx configuration
- **default.conf**: Server blocks and routing rules
- **Dockerfile**: Container build instructions

## Security

### SSL/TLS
- TLS 1.2 and 1.3 enabled
- Strong cipher suites only
- Server cipher preference enabled

### Headers
- `X-Frame-Options: SAMEORIGIN`
- `X-Content-Type-Options: nosniff`
- `X-XSS-Protection: 1; mode=block`

### Backend Communication
- Admin panel: HTTP (internal network)
- Pixel server tracking: HTTPS with SSL verification disabled (self-signed)
- Pixel server reports: HTTP (internal network)

## Troubleshooting

### Check if proxy is running
```bash
docker-compose ps nginx-proxy
```

### View logs
```bash
docker-compose logs nginx-proxy
```

### Test health endpoint
```bash
curl -k https://your-server/health
```

### Verify SSL certificate
```bash
openssl s_client -connect your-server:443 -showcerts
```

## Development

To modify routing rules:

1. Edit `default.conf`
2. Rebuild the container:
```bash
docker-compose build nginx-proxy
docker-compose restart nginx-proxy
```

## Network

- **Container Name**: `nginx_proxy`
- **Network**: `mailnet` (internal Docker network)
- **IP Address**: Configured via `PROXY_IP` environment variable
- **Exposed Ports**: 80 (HTTP), 443 (HTTPS)
