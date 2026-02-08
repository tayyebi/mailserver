@extends('layouts.app')

@section('title', 'Edit Email Account')

@section('content')
<div class="card">
    <h2>Edit Email Account: {{ $emailAccount->email }}</h2>
    
    <form action="{{ route('email-accounts.update', $emailAccount) }}" method="POST">
        @csrf
        @method('PUT')
        
        <div class="form-group">
            <label for="domain_id">Domain *</label>
            <select id="domain_id" name="domain_id" required>
                @foreach($domains as $domain)
                    <option value="{{ $domain->id }}" {{ old('domain_id', $emailAccount->domain_id) == $domain->id ? 'selected' : '' }}>
                        {{ $domain->domain }}
                    </option>
                @endforeach
            </select>
        </div>

        <div class="form-group">
            <label for="username">Username *</label>
            <input type="text" id="username" name="username" required value="{{ old('username', $emailAccount->username) }}">
        </div>

        <div class="form-group">
            <label for="email">Email *</label>
            <input type="email" id="email" name="email" required value="{{ old('email', $emailAccount->email) }}">
        </div>

        <div class="form-group">
            <label for="password">Password (leave blank to keep current)</label>
            <input type="password" id="password" name="password" minlength="8">
        </div>

        <div class="form-group">
            <label for="name">Full Name</label>
            <input type="text" id="name" name="name" value="{{ old('name', $emailAccount->name) }}">
        </div>

        <div class="form-group">
            <label for="quota">Quota (MB, 0 = unlimited)</label>
            <input type="number" id="quota" name="quota" value="{{ old('quota', $emailAccount->quota) }}" min="0" step="100">
        </div>

        <div class="form-group">
            <label>
                <input type="checkbox" name="active" value="1" {{ old('active', $emailAccount->active) ? 'checked' : '' }}>
                Active
            </label>
        </div>

        <div class="actions">
            <button type="submit" class="btn btn-success">Update Account</button>
            <a href="{{ route('email-accounts.index') }}" class="btn">Cancel</a>
        </div>
    </form>
</div>
@endsection
