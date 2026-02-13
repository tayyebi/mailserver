<?php

namespace App\Models;

use App\Jobs\SyncMailConfigJob;
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
        // Dispatch sync job with debouncing via job queue
        SyncMailConfigJob::dispatch()->onQueue('mail-sync');
    }
}
