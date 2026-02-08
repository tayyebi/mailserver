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
}
