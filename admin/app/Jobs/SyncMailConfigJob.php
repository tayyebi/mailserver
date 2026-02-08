<?php

namespace App\Jobs;

use Illuminate\Bus\Queueable;
use Illuminate\Contracts\Queue\ShouldQueue;
use Illuminate\Foundation\Bus\Dispatchable;
use Illuminate\Queue\InteractsWithQueue;
use Illuminate\Queue\SerializesModels;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;

class SyncMailConfigJob implements ShouldQueue
{
    use Dispatchable, InteractsWithQueue, Queueable, SerializesModels;

    /**
     * Execute the job with lock to prevent overlapping syncs.
     */
    public function handle(): void
    {
        // Use cache lock to prevent overlapping sync processes
        $lock = Cache::lock('mail-config-sync', 10);

        if (!$lock->get()) {
            // Another sync is already running, skip this one
            Log::info('Skipping mail config sync - already in progress');
            return;
        }

        try {
            $this->syncConfiguration();
        } finally {
            $lock->release();
        }
    }

    /**
     * Sync the configuration from SQLite to config files.
     */
    protected function syncConfiguration(): void
    {
        $dbPath = database_path('database.sqlite');
        
        if (!file_exists($dbPath)) {
            Log::warning('Database file not found for sync');
            return;
        }

        // Generate config data
        $domains = DB::table('domains')
            ->where('active', true)
            ->pluck('domain')
            ->toArray();

        $mailboxes = DB::table('email_accounts')
            ->where('active', true)
            ->get()
            ->map(fn($account) => $account->email . ' ' . $account->username . '/')
            ->toArray();

        $aliases = DB::table('aliases')
            ->where('active', true)
            ->get()
            ->map(fn($alias) => $alias->source . ' ' . $alias->destination)
            ->toArray();

        $passwords = DB::table('email_accounts')
            ->where('active', true)
            ->get()
            ->map(fn($account) => $account->email . ':{BLF-CRYPT}' . $account->password)
            ->toArray();

        // Write to shared config directory that's mounted in Postfix/Dovecot containers
        $configDir = storage_path('app/mail-config');
        
        // Ensure directory exists
        if (!is_dir($configDir)) {
            if (!mkdir($configDir, 0755, true)) {
                Log::error('Failed to create mail config directory: ' . $configDir);
                throw new \RuntimeException('Failed to create mail config directory');
            }
        }

        // Write config files atomically (write to temp, then rename)
        $this->writeConfigFileAtomic($configDir . '/virtual_domains', $domains, 0644);
        $this->writeConfigFileAtomic($configDir . '/vmailbox', $mailboxes, 0644);
        $this->writeConfigFileAtomic($configDir . '/virtual_aliases', $aliases, 0644);
        $this->writeConfigFileAtomic($configDir . '/dovecot_passwd', $passwords, 0600);

        Log::info('Mail configuration synced successfully', [
            'domains' => count($domains),
            'mailboxes' => count($mailboxes),
            'aliases' => count($aliases),
            'passwords' => count($passwords),
        ]);
    }

    /**
     * Write content to a file atomically.
     */
    protected function writeConfigFileAtomic(string $path, array $lines, int $permissions): void
    {
        $tempFile = $path . '.tmp';
        
        try {
            // Write to temp file
            if (file_put_contents($tempFile, implode("\n", $lines) . "\n") === false) {
                throw new \RuntimeException("Failed to write to temp file: $tempFile");
            }
            
            // Set permissions
            if (!chmod($tempFile, $permissions)) {
                throw new \RuntimeException("Failed to set permissions on: $tempFile");
            }
            
            // Atomic rename
            if (!rename($tempFile, $path)) {
                throw new \RuntimeException("Failed to rename $tempFile to $path");
            }
        } catch (\Exception $e) {
            // Clean up temp file on failure
            @unlink($tempFile);
            Log::error('Failed to write config file', [
                'path' => $path,
                'error' => $e->getMessage()
            ]);
            throw $e;
        }
    }
}
