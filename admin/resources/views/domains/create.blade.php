@extends('layouts.app')

@section('title', 'Create Domain')

@section('content')
<div class="card">
    <h2>Create New Domain</h2>
    
    <form action="{{ route('domains.store') }}" method="POST">
        @csrf
        
        <div class="form-group">
            <label for="domain">Domain Name *</label>
            <input type="text" id="domain" name="domain" required placeholder="example.com" value="{{ old('domain') }}">
            @error('domain')
                <small style="color: red;">{{ $message }}</small>
            @enderror
        </div>

        <div class="form-group">
            <label for="description">Description</label>
            <textarea id="description" name="description" rows="3" placeholder="Optional description">{{ old('description') }}</textarea>
        </div>

        <div class="form-group">
            <label>
                <input type="checkbox" name="active" value="1" checked>
                Active
            </label>
        </div>

        <h3 style="margin-top: 2rem;">DKIM Configuration (Optional)</h3>
        <p style="color: #666; margin-bottom: 1rem;">Configure DKIM signing for this domain.</p>

        <div class="form-group">
            <label>
                <input type="checkbox" name="generate_dkim" value="1" id="generate_dkim" onchange="toggleDkimFields()">
                Auto-generate DKIM keys
            </label>
            <small style="color: #666; display: block; margin-top: 0.25rem;">
                Automatically create DKIM signing keys for this domain
            </small>
        </div>

        <div id="dkim_manual_fields">
            <div class="form-group">
                <label for="dkim_selector">DKIM Selector</label>
                <input type="text" id="dkim_selector" name="dkim_selector" placeholder="mail" value="{{ old('dkim_selector', 'mail') }}">
                <small style="color: #666;">Example: mail, default, dkim</small>
            </div>

            <div class="form-group">
                <label for="dkim_private_key">DKIM Private Key (leave empty for auto-generation)</label>
                <textarea id="dkim_private_key" name="dkim_private_key" rows="6" placeholder="-----BEGIN RSA PRIVATE KEY-----&#10;...&#10;-----END RSA PRIVATE KEY-----">{{ old('dkim_private_key') }}</textarea>
                <small style="color: #666;">PEM format private key for signing</small>
            </div>

            <div class="form-group">
                <label for="dkim_public_key">DKIM Public Key (leave empty for auto-generation)</label>
                <textarea id="dkim_public_key" name="dkim_public_key" rows="3" placeholder="v=DKIM1; k=rsa; p=...">{{ old('dkim_public_key') }}</textarea>
                <small style="color: #666;">Public key for DNS TXT record</small>
            </div>
        </div>

        <script>
            function toggleDkimFields() {
                const checkbox = document.getElementById('generate_dkim');
                const manualFields = document.getElementById('dkim_manual_fields');
                const privateKeyField = document.getElementById('dkim_private_key');
                const publicKeyField = document.getElementById('dkim_public_key');
                
                if (checkbox.checked) {
                    privateKeyField.disabled = true;
                    publicKeyField.disabled = true;
                    privateKeyField.style.opacity = '0.5';
                    publicKeyField.style.opacity = '0.5';
                } else {
                    privateKeyField.disabled = false;
                    publicKeyField.disabled = false;
                    privateKeyField.style.opacity = '1';
                    publicKeyField.style.opacity = '1';
                }
            }
        </script>

        <div class="actions">
            <button type="submit" class="btn btn-success">Create Domain</button>
            <a href="{{ route('domains.index') }}" class="btn">Cancel</a>
        </div>
    </form>
</div>
@endsection
