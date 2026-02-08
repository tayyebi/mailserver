@extends('layouts.app')

@section('title', 'Edit Domain')

@section('content')
<div class="card">
    <h2>Edit Domain: {{ $domain->domain }}</h2>
    
    <form action="{{ route('domains.update', $domain) }}" method="POST">
        @csrf
        @method('PUT')
        
        <div class="form-group">
            <label for="domain">Domain Name *</label>
            <input type="text" id="domain" name="domain" required value="{{ old('domain', $domain->domain) }}">
            @error('domain')
                <small style="color: red;">{{ $message }}</small>
            @enderror
        </div>

        <div class="form-group">
            <label for="description">Description</label>
            <textarea id="description" name="description" rows="3">{{ old('description', $domain->description) }}</textarea>
        </div>

        <div class="form-group">
            <label>
                <input type="checkbox" name="active" value="1" {{ old('active', $domain->active) ? 'checked' : '' }}>
                Active
            </label>
        </div>

        <h3 style="margin-top: 2rem;">DKIM Configuration (Optional)</h3>
        <p style="color: #666; margin-bottom: 1rem;">Configure DKIM signing for this domain. Leave empty to skip DKIM setup.</p>

        <div class="form-group">
            <label for="dkim_selector">DKIM Selector</label>
            <input type="text" id="dkim_selector" name="dkim_selector" placeholder="mail" value="{{ old('dkim_selector', $domain->dkim_selector) }}">
            <small style="color: #666;">Example: mail, default, dkim</small>
        </div>

        <div class="form-group">
            <label for="dkim_private_key">DKIM Private Key</label>
            <textarea id="dkim_private_key" name="dkim_private_key" rows="6" placeholder="-----BEGIN RSA PRIVATE KEY-----&#10;...&#10;-----END RSA PRIVATE KEY-----">{{ old('dkim_private_key', $domain->dkim_private_key) }}</textarea>
            <small style="color: #666;">PEM format private key for signing</small>
        </div>

        <div class="form-group">
            <label for="dkim_public_key">DKIM Public Key</label>
            <textarea id="dkim_public_key" name="dkim_public_key" rows="3" placeholder="v=DKIM1; k=rsa; p=...">{{ old('dkim_public_key', $domain->dkim_public_key) }}</textarea>
            <small style="color: #666;">Public key for DNS TXT record</small>
        </div>

        <div class="actions">
            <button type="submit" class="btn btn-success">Update Domain</button>
            <a href="{{ route('domains.index') }}" class="btn">Cancel</a>
        </div>
    </form>
</div>
@endsection
