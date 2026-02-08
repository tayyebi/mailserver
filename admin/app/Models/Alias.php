<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class Alias extends Model
{
    protected $fillable = [
        'domain_id',
        'source',
        'destination',
        'active',
    ];

    protected $casts = [
        'active' => 'boolean',
    ];

    public function domain(): BelongsTo
    {
        return $this->belongsTo(Domain::class);
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
