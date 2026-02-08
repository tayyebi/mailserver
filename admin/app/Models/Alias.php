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
}
