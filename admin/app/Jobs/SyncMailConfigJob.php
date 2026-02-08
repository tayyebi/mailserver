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

        // Write to temporary files first (atomic writes)
        $this->writeConfigFile('/tmp/virtual_domains', $domains);
        $this->writeConfigFile('/tmp/vmailbox', $mailboxes);
        $this->writeConfigFile('/tmp/virtual_aliases', $aliases);
        $this->writeConfigFile('/tmp/dovecot_passwd', $passwords);

        // Move to final locations if they exist
        $this->moveIfTargetExists('/tmp/virtual_domains', '/etc/postfix/virtual_domains', 0644);
        $this->moveIfTargetExists('/tmp/vmailbox', '/etc/postfix/vmailbox', 0644);
        $this->moveIfTargetExists('/tmp/virtual_aliases', '/etc/postfix/virtual_aliases', 0644);
        $this->moveIfTargetExists('/tmp/dovecot_passwd', '/etc/dovecot/passwd', 0600);

        // Generate postmap databases
        $this->runCommand('postmap /etc/postfix/vmailbox');
        $this->runCommand('postmap /etc/postfix/virtual_aliases');

        // Reload services
        $this->runCommand('postfix reload');
        $this->runCommand('doveadm reload');

        Log::info('Mail configuration synced successfully');
    }

    /**
     * Write content to a file.
     */
    protected function writeConfigFile(string $path, array $lines): void
    {
        file_put_contents($path, implode("\n", $lines) . "\n");
    }

    /**
     * Move file to target if target directory exists.
     */
    protected function moveIfTargetExists(string $source, string $target, int $permissions): void
    {
        $targetDir = dirname($target);
        
        if (is_dir($targetDir)) {
            rename($source, $target);
            chmod($target, $permissions);
        } else {
            // Clean up temp file if target doesn't exist
            @unlink($source);
        }
    }

    /**
     * Run a system command and log any errors.
     */
    protected function runCommand(string $command): void
    {
        exec($command . ' 2>&1', $output, $return);
        
        if ($return !== 0) {
            Log::warning("Command failed: $command", ['output' => $output]);
        }
    }
}
