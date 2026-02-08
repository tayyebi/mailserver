@extends('layouts.app')

@section('title', 'Dashboard')

@section('content')
<div class="card">
    <h2>Dashboard</h2>
    <p>Welcome to the Mail Server Administration Panel</p>
</div>

<div class="stats">
    <div class="stat-box">
        <h3>{{ $stats['total_domains'] }}</h3>
        <p>Total Domains</p>
        <small>({{ $stats['active_domains'] }} active)</small>
    </div>
    <div class="stat-box">
        <h3>{{ $stats['total_email_accounts'] }}</h3>
        <p>Email Accounts</p>
        <small>({{ $stats['active_email_accounts'] }} active)</small>
    </div>
    <div class="stat-box">
        <h3>{{ $stats['total_aliases'] }}</h3>
        <p>Email Aliases</p>
        <small>({{ $stats['active_aliases'] }} active)</small>
    </div>
</div>

<div class="card">
    <h3>Quick Actions</h3>
    <div class="actions">
        <a href="{{ route('domains.create') }}" class="btn">+ Add Domain</a>
        <a href="{{ route('email-accounts.create') }}" class="btn">+ Add Email Account</a>
        <a href="{{ route('aliases.create') }}" class="btn">+ Add Alias</a>
    </div>
</div>
@endsection
