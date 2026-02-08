<?php

namespace App\Models;

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
        // Trigger config sync in background
        if (file_exists(base_path('sync-config.sh'))) {
            exec('bash ' . base_path('sync-config.sh') . ' > /dev/null 2>&1 &');
        }
    }
}
