<?php

namespace App\Models;

use App\Jobs\SyncMailConfigJob;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;

class Domain extends Model
{
    protected $fillable = [
        'domain',
        'description',
        'active',
        'dkim_selector',
        'dkim_private_key',
        'dkim_public_key',
    ];

    protected $casts = [
        'active' => 'boolean',
        'dkim_private_key' => 'encrypted',
    ];

    public function emailAccounts(): HasMany
    {
        return $this->hasMany(EmailAccount::class);
    }

    public function aliases(): HasMany
    {
        return $this->hasMany(Alias::class);
    }

    protected static function booted(): void
    {
        static::saved(function () {
            self::syncConfig();
        });

        static::deleted(function () {
            self::syncConfig();
        });
    }

    protected static function syncConfig(): void
    {
        // Dispatch sync job with debouncing via job queue
        SyncMailConfigJob::dispatch()->onQueue('mail-sync');
    }
}
