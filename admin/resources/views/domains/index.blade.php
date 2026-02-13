@extends('layouts.app')

@section('title', 'Domains')

@section('content')
<div class="card">
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem;">
        <h2>Domains</h2>
        <a href="{{ route('domains.create') }}" class="btn">+ Add Domain</a>
    </div>

    @if($domains->isEmpty())
        <p>No domains found. <a href="{{ route('domains.create') }}">Create your first domain</a></p>
    @else
        <table>
            <thead>
                <tr>
                    <th>Domain</th>
                    <th>Description</th>
                    <th>Status</th>
                    <th>DKIM</th>
                    <th>Accounts</th>
                    <th>Aliases</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                @foreach($domains as $domain)
                <tr>
                    <td><strong>{{ $domain->domain }}</strong></td>
                    <td>{{ $domain->description ?? '-' }}</td>
                    <td>
                        <span class="badge {{ $domain->active ? 'badge-success' : 'badge-danger' }}">
                            {{ $domain->active ? 'Active' : 'Inactive' }}
                        </span>
                    </td>
                    <td>
                        @if($domain->dkim_public_key)
                            <a href="{{ route('domains.show-dkim', $domain) }}" style="color: #10b981;">✓ Configured</a>
                        @else
                            <a href="{{ route('domains.show-dkim', $domain) }}" style="color: #f59e0b;">⚠ Not Set</a>
                        @endif
                    </td>
                    <td>{{ $domain->emailAccounts->count() }}</td>
                    <td>{{ $domain->aliases->count() }}</td>
                    <td class="actions">
                        <a href="{{ route('domains.edit', $domain) }}" class="btn btn-sm">Edit</a>
                        <form action="{{ route('domains.destroy', $domain) }}" method="POST" style="display: inline;">
                            @csrf
                            @method('DELETE')
                            <button type="submit" class="btn btn-sm btn-danger" onclick="return confirm('Are you sure?')">Delete</button>
                        </form>
                    </td>
                </tr>
                @endforeach
            </tbody>
        </table>
    @endif
</div>
@endsection
