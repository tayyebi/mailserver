<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Support\Facades\Hash;

class EmailAccount extends Model
{
    protected $fillable = [
        'domain_id',
        'username',
        'email',
        'password',
        'name',
        'active',
        'quota',
    ];

    protected $hidden = [
        'password',
    ];

    protected $casts = [
        'active' => 'boolean',
        'quota' => 'integer',
    ];

    public function domain(): BelongsTo
    {
        return $this->belongsTo(Domain::class);
    }

    public function setPasswordAttribute($value)
    {
        $this->attributes['password'] = Hash::make($value);
    }
}
