@extends('layouts.app')

@section('title', 'Client Setup – ' . $emailAccount->email)

@section('content')
<div class="card">
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem;">
        <h2>Client Setup: {{ $emailAccount->email }}</h2>
        <a href="{{ route('email-accounts.index') }}" class="btn">← Back</a>
    </div>

    @if($emailAccount->name)
        <p style="margin-bottom: 1.5rem; color: #555;">Display name: <strong>{{ $emailAccount->name }}</strong></p>
    @endif

    {{-- Connection details table --}}
    <h3 style="margin-bottom: 0.75rem;">Connection Details</h3>
    <table>
        <thead>
            <tr>
                <th>Protocol</th>
                <th>Server</th>
                <th>Port</th>
                <th>Security</th>
                <th>Authentication</th>
            </tr>
        </thead>
        <tbody>
            <tr>
                <td><strong>IMAP</strong></td>
                <td>{{ $connection['host'] }}</td>
                <td>{{ $connection['ports']['imaps'] }}</td>
                <td>SSL/TLS</td>
                <td>{{ $connection['email'] }}</td>
            </tr>
            <tr style="color: #888;">
                <td>IMAP (plain)</td>
                <td>{{ $connection['host'] }}</td>
                <td>{{ $connection['ports']['imap'] }}</td>
                <td>STARTTLS</td>
                <td>{{ $connection['email'] }}</td>
            </tr>
            <tr>
                <td><strong>POP3</strong></td>
                <td>{{ $connection['host'] }}</td>
                <td>{{ $connection['ports']['pop3s'] }}</td>
                <td>SSL/TLS</td>
                <td>{{ $connection['email'] }}</td>
            </tr>
            <tr style="color: #888;">
                <td>POP3 (plain)</td>
                <td>{{ $connection['host'] }}</td>
                <td>{{ $connection['ports']['pop3'] }}</td>
                <td>STARTTLS</td>
                <td>{{ $connection['email'] }}</td>
            </tr>
            <tr>
                <td><strong>SMTP (sending)</strong></td>
                <td>{{ $connection['host'] }}</td>
                <td>{{ $connection['ports']['submission'] }}</td>
                <td>STARTTLS</td>
                <td>{{ $connection['email'] }}</td>
            </tr>
            <tr>
                <td><strong>SMTPS (sending)</strong></td>
                <td>{{ $connection['host'] }}</td>
                <td>{{ $connection['ports']['smtps'] }}</td>
                <td>SSL/TLS</td>
                <td>{{ $connection['email'] }}</td>
            </tr>
        </tbody>
    </table>

    <p style="margin-top: 0.75rem; color: #666; font-size: 0.875rem;">
        <strong>Username:</strong> {{ $connection['email'] }} &nbsp;|&nbsp;
        <strong>Password:</strong> the password set for this account
    </p>

    {{-- Client-specific guides --}}
    <h3 style="margin-top: 2rem; margin-bottom: 0.75rem;">Quick Setup Guides</h3>

    <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 1rem;">
        {{-- Thunderbird --}}
        <div class="card" style="margin-bottom: 0;">
            <h4>🦊 Thunderbird</h4>
            <ol style="margin-left: 1.25rem; margin-top: 0.5rem; line-height: 1.8;">
                <li>Menu → <em>Account Settings</em> → <em>Account Actions</em> → <em>Add Mail Account</em></li>
                <li>Name: <strong>{{ $connection['name'] ?? $connection['email'] }}</strong></li>
                <li>Email: <strong>{{ $connection['email'] }}</strong></li>
                <li>Click <em>Configure manually</em></li>
                <li>Incoming: IMAP · <strong>{{ $connection['host'] }}</strong> · Port <strong>{{ $connection['ports']['imaps'] }}</strong> · SSL/TLS</li>
                <li>Outgoing: SMTP · <strong>{{ $connection['host'] }}</strong> · Port <strong>{{ $connection['ports']['submission'] }}</strong> · STARTTLS</li>
                <li>Username (both): <strong>{{ $connection['email'] }}</strong></li>
            </ol>
        </div>

        {{-- Outlook --}}
        <div class="card" style="margin-bottom: 0;">
            <h4>📧 Outlook / Windows Mail</h4>
            <ol style="margin-left: 1.25rem; margin-top: 0.5rem; line-height: 1.8;">
                <li>File → <em>Add Account</em> → choose <em>Advanced setup</em> → <em>IMAP</em></li>
                <li>Email: <strong>{{ $connection['email'] }}</strong></li>
                <li>Incoming server: <strong>{{ $connection['host'] }}</strong> · Port <strong>{{ $connection['ports']['imaps'] }}</strong> · SSL/TLS</li>
                <li>Outgoing server: <strong>{{ $connection['host'] }}</strong> · Port <strong>{{ $connection['ports']['submission'] }}</strong> · STARTTLS</li>
                <li>Username: <strong>{{ $connection['email'] }}</strong></li>
            </ol>
        </div>

        {{-- Apple Mail --}}
        <div class="card" style="margin-bottom: 0;">
            <h4>🍎 Apple Mail (macOS / iOS)</h4>
            <ol style="margin-left: 1.25rem; margin-top: 0.5rem; line-height: 1.8;">
                <li>Settings → <em>Mail</em> → <em>Accounts</em> → <em>Add Account</em> → <em>Other</em></li>
                <li>Name: <strong>{{ $connection['name'] ?? $connection['email'] }}</strong></li>
                <li>Email: <strong>{{ $connection['email'] }}</strong></li>
                <li>Incoming: IMAP · <strong>{{ $connection['host'] }}</strong> · Port <strong>{{ $connection['ports']['imaps'] }}</strong> · SSL</li>
                <li>Outgoing: <strong>{{ $connection['host'] }}</strong> · Port <strong>{{ $connection['ports']['submission'] }}</strong> · STARTTLS</li>
                <li>Username: <strong>{{ $connection['email'] }}</strong></li>
            </ol>
        </div>

        {{-- Android / Gmail app --}}
        <div class="card" style="margin-bottom: 0;">
            <h4>🤖 Android (Gmail app)</h4>
            <ol style="margin-left: 1.25rem; margin-top: 0.5rem; line-height: 1.8;">
                <li>Settings → <em>Add account</em> → <em>Other</em></li>
                <li>Email: <strong>{{ $connection['email'] }}</strong></li>
                <li>Choose <strong>IMAP</strong></li>
                <li>Incoming: <strong>{{ $connection['host'] }}</strong> · Port <strong>{{ $connection['ports']['imaps'] }}</strong> · Security: SSL/TLS</li>
                <li>Outgoing: <strong>{{ $connection['host'] }}</strong> · Port <strong>{{ $connection['ports']['submission'] }}</strong> · Security: STARTTLS</li>
                <li>Username: <strong>{{ $connection['email'] }}</strong></li>
            </ol>
        </div>
    </div>
</div>
@endsection
