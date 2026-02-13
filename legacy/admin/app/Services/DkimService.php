<?php

namespace App\Services;

use Exception;
use Illuminate\Support\Facades\Log;

class DkimService
{
    /**
     * Generate DKIM key pair for a domain
     *
     * @param string $domain The domain name
     * @param string $selector DKIM selector (default: 'mail')
     * @return array Array with 'private_key' and 'public_key'
     * @throws Exception
     */
    public function generateKeys(string $domain, string $selector = 'mail'): array
    {
        // Create temporary directory for key generation
        $tempDir = sys_get_temp_dir() . '/dkim_' . uniqid();
        
        try {
            if (!mkdir($tempDir, 0700, true)) {
                throw new Exception("Failed to create temporary directory");
            }

            // Generate RSA key pair using openssl
            $privateKeyPath = $tempDir . '/' . $selector . '.private';

            // Generate private key (2048-bit RSA)
            $generateCmd = sprintf(
                'openssl genrsa -out %s 2048 2>&1',
                escapeshellarg($privateKeyPath)
            );
            
            exec($generateCmd, $output, $returnCode);
            
            if ($returnCode !== 0 || !file_exists($privateKeyPath)) {
                throw new Exception("Failed to generate private key: " . implode("\n", $output));
            }

            // Extract public key
            $publicKeyCmd = sprintf(
                'openssl rsa -in %s -pubout -outform PEM 2>&1',
                escapeshellarg($privateKeyPath)
            );
            
            exec($publicKeyCmd, $publicKeyOutput, $publicKeyReturnCode);
            
            if ($publicKeyReturnCode !== 0) {
                throw new Exception("Failed to extract public key: " . implode("\n", $publicKeyOutput));
            }

            $publicKeyPem = implode("\n", $publicKeyOutput);
            
            // Convert public key to DKIM DNS format
            $dnsRecord = $this->convertToDkimDnsFormat($publicKeyPem, $selector, $domain);

            // Read private key
            $privateKey = file_get_contents($privateKeyPath);

            if (!$privateKey) {
                throw new Exception("Failed to read private key");
            }

            return [
                'private_key' => $privateKey,
                'public_key' => $dnsRecord,
                'selector' => $selector,
                'dns_record_name' => $selector . '._domainkey.' . $domain,
            ];

        } finally {
            // Clean up temporary files
            if (isset($privateKeyPath) && file_exists($privateKeyPath)) {
                @unlink($privateKeyPath);
            }
            if (is_dir($tempDir)) {
                @rmdir($tempDir);
            }
        }
    }

    /**
     * Convert PEM public key to DKIM DNS TXT record format
     *
     * @param string $publicKeyPem Public key in PEM format
     * @param string $selector DKIM selector
     * @param string $domain Domain name
     * @return string DKIM DNS record value
     */
    private function convertToDkimDnsFormat(string $publicKeyPem, string $selector, string $domain): string
    {
        // Remove PEM headers and footers, and whitespace
        $publicKeyBase64 = preg_replace(
            '/-----BEGIN PUBLIC KEY-----|-----END PUBLIC KEY-----|\s+/',
            '',
            $publicKeyPem
        );

        // Format as DKIM TXT record
        return sprintf('v=DKIM1; k=rsa; p=%s', $publicKeyBase64);
    }

    /**
     * Write DKIM keys to OpenDKIM directory structure
     *
     * @param string $domain Domain name
     * @param string $selector DKIM selector
     * @param string $privateKey Private key content
     * @return bool Success status
     */
    public function writeKeysToOpendkim(string $domain, string $selector, string $privateKey): bool
    {
        try {
            $keyDir = "/var/www/html/storage/app/mail-config/opendkim/keys/{$domain}";
            
            // Create directory if it doesn't exist
            if (!is_dir($keyDir)) {
                if (!mkdir($keyDir, 0755, true)) {
                    Log::error("Failed to create DKIM key directory: {$keyDir}");
                    return false;
                }
            }

            $privateKeyFile = "{$keyDir}/{$selector}.private";
            
            // Write private key
            if (file_put_contents($privateKeyFile, $privateKey) === false) {
                Log::error("Failed to write private key file: {$privateKeyFile}");
                return false;
            }

            // Set proper permissions
            $chmodResult = chmod($privateKeyFile, 0600);
            if (!$chmodResult) {
                Log::error("Failed to set permissions on private key file: {$privateKeyFile}");
                return false;
            }

            // Update KeyTable
            $this->updateKeyTable($domain, $selector);
            
            // Update SigningTable
            $this->updateSigningTable($domain, $selector);

            return true;

        } catch (Exception $e) {
            Log::error("Error writing DKIM keys: " . $e->getMessage());
            return false;
        }
    }

    /**
     * Update OpenDKIM KeyTable file
     */
    private function updateKeyTable(string $domain, string $selector): void
    {
        $keyTablePath = '/var/www/html/storage/app/mail-config/opendkim/KeyTable';
        $entry = "{$selector}._domainkey.{$domain}  {$domain}:{$selector}:/etc/opendkim/keys/{$domain}/{$selector}.private\n";
        
        // Ensure directory exists
        $dir = dirname($keyTablePath);
        if (!is_dir($dir)) {
            mkdir($dir, 0755, true);
        }
        
        // Read existing content
        $content = file_exists($keyTablePath) ? file_get_contents($keyTablePath) : '';
        
        // Check if entry already exists
        if (strpos($content, "{$selector}._domainkey.{$domain}") === false) {
            file_put_contents($keyTablePath, $content . $entry);
        }
    }

    /**
     * Update OpenDKIM SigningTable file
     */
    private function updateSigningTable(string $domain, string $selector): void
    {
        $signingTablePath = '/var/www/html/storage/app/mail-config/opendkim/SigningTable';
        $entry = "*@{$domain}       {$selector}._domainkey.{$domain}\n";
        
        // Ensure directory exists
        $dir = dirname($signingTablePath);
        if (!is_dir($dir)) {
            mkdir($dir, 0755, true);
        }
        
        // Read existing content
        $content = file_exists($signingTablePath) ? file_get_contents($signingTablePath) : '';
        
        // Check if entry already exists
        if (strpos($content, "@{$domain}") === false) {
            file_put_contents($signingTablePath, $content . $entry);
        }
    }

    /**
     * Get DNS instructions for DKIM setup
     *
     * @param string $domain Domain name
     * @param string $selector DKIM selector
     * @param string $publicKey Public key DNS record
     * @return string Formatted DNS instructions
     */
    public function getDnsInstructions(string $domain, string $selector, string $publicKey): string
    {
        return sprintf(
            "Add the following TXT record to your DNS:\n\n" .
            "Name: %s._domainkey.%s\n" .
            "Type: TXT\n" .
            "Value: %s\n\n" .
            "Note: Some DNS providers require you to remove quotes from the value.",
            $selector,
            $domain,
            $publicKey
        );
    }
}
