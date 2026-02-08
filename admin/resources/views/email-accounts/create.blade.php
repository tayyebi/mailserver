@extends('layouts.app')

@section('title', 'Create Email Account')

@section('content')
<div class="card">
    <h2>Create New Email Account</h2>
    
    <form action="{{ route('email-accounts.store') }}" method="POST">
        @csrf
        
        <div class="form-group">
            <label for="domain_id">Domain *</label>
            <select id="domain_id" name="domain_id" required>
                <option value="">Select Domain</option>
                @foreach($domains as $domain)
                    <option value="{{ $domain->id }}" {{ old('domain_id') == $domain->id ? 'selected' : '' }}>
                        {{ $domain->domain }}
                    </option>
                @endforeach
            </select>
            @error('domain_id')
                <small style="color: red;">{{ $message }}</small>
            @enderror
        </div>

        <div class="form-group">
            <label for="username">Username (local part) *</label>
            <input type="text" id="username" name="username" required placeholder="user" value="{{ old('username') }}">
            @error('username')
                <small style="color: red;">{{ $message }}</small>
            @enderror
        </div>

        <div class="form-group">
            <label for="email">Full Email Address *</label>
            <input type="email" id="email" name="email" required placeholder="user@example.com" value="{{ old('email') }}">
            @error('email')
                <small style="color: red;">{{ $message }}</small>
            @enderror
        </div>

        <div class="form-group">
            <label for="password">Password *</label>
            <input type="password" id="password" name="password" required minlength="8">
            @error('password')
                <small style="color: red;">{{ $message }}</small>
            @enderror
        </div>

        <div class="form-group">
            <label for="name">Full Name</label>
            <input type="text" id="name" name="name" placeholder="John Doe" value="{{ old('name') }}">
        </div>

        <div class="form-group">
            <label for="quota">Quota (MB, 0 = unlimited)</label>
            <input type="number" id="quota" name="quota" value="{{ old('quota', 0) }}" min="0" step="100">
        </div>

        <div class="form-group">
            <label>
                <input type="checkbox" name="active" value="1" checked>
                Active
            </label>
        </div>

        <div class="actions">
            <button type="submit" class="btn btn-success">Create Account</button>
            <a href="{{ route('email-accounts.index') }}" class="btn">Cancel</a>
        </div>
    </form>
</div>
@endsection
