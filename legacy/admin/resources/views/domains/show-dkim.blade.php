@extends('layouts.app')

@section('title', 'DKIM Configuration - ' . $domain->domain)

@section('content')
<div class="card">
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem;">
        <h2>DKIM Configuration for {{ $domain->domain }}</h2>
        <a href="{{ route('domains.index') }}" class="btn">← Back to Domains</a>
    </div>

    @if($domain->dkim_public_key)
        <div style="background: #e8f5e9; padding: 1rem; border-radius: 4px; margin-bottom: 1.5rem;">
            <strong>✓ DKIM is configured for this domain</strong>
        </div>

        <h3>DNS Configuration</h3>
        <p>Add the following TXT record to your DNS provider:</p>
        
        <div style="background: #f5f5f5; padding: 1rem; border-radius: 4px; margin: 1rem 0; font-family: monospace;">
            <div style="margin-bottom: 0.5rem;">
                <strong>Name:</strong> {{ $domain->dkim_selector ?? 'mail' }}._domainkey.{{ $domain->domain }}
            </div>
            <div style="margin-bottom: 0.5rem;">
                <strong>Type:</strong> TXT
            </div>
            <div>
                <strong>Value:</strong><br>
                <textarea readonly style="width: 100%; min-height: 80px; font-family: monospace; font-size: 12px; margin-top: 0.5rem;">{{ $domain->dkim_public_key }}</textarea>
            </div>
        </div>

        <div style="background: #fff3cd; padding: 1rem; border-radius: 4px; margin: 1rem 0;">
            <strong>Note:</strong> Some DNS providers require you to remove quotes from the TXT record value.
        </div>

        <h3 style="margin-top: 2rem;">DKIM Selector</h3>
        <p><strong>Current Selector:</strong> {{ $domain->dkim_selector ?? 'mail' }}</p>

        <h3 style="margin-top: 2rem;">Regenerate DKIM Keys</h3>
        <p style="color: #666;">Warning: Regenerating keys will invalidate the current keys. Update your DNS records after regeneration.</p>
        
        <form action="{{ route('domains.generate-dkim', $domain) }}" method="POST" style="margin-top: 1rem;">
            @csrf
            <div class="form-group">
                <label for="dkim_selector">DKIM Selector</label>
                <input type="text" id="dkim_selector" name="dkim_selector" value="{{ $domain->dkim_selector ?? 'mail' }}" placeholder="mail">
                <small style="color: #666;">Common values: mail, default, dkim</small>
            </div>
            
            <button type="submit" class="btn btn-warning" onclick="return confirm('Are you sure you want to regenerate DKIM keys? This will invalidate the current keys.')">
                Regenerate DKIM Keys
            </button>
        </form>

    @else
        <div style="background: #fff3cd; padding: 1rem; border-radius: 4px; margin-bottom: 1.5rem;">
            <strong>⚠ DKIM is not configured for this domain</strong>
        </div>

        <h3>Generate DKIM Keys</h3>
        <p>Generate DKIM keys to enable email signing for this domain.</p>
        
        <form action="{{ route('domains.generate-dkim', $domain) }}" method="POST" style="margin-top: 1rem;">
            @csrf
            <div class="form-group">
                <label for="dkim_selector">DKIM Selector *</label>
                <input type="text" id="dkim_selector" name="dkim_selector" value="{{ old('dkim_selector', 'mail') }}" placeholder="mail" required>
                <small style="color: #666;">Common values: mail, default, dkim</small>
            </div>
            
            <button type="submit" class="btn btn-success">Generate DKIM Keys</button>
        </form>
    @endif
</div>
@endsection
