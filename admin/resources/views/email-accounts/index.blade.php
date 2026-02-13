@extends('layouts.app')

@section('title', 'Email Accounts')

@section('content')
<div class="card">
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem;">
        <h2>Email Accounts</h2>
        <a href="{{ route('email-accounts.create') }}" class="btn">+ Add Email Account</a>
    </div>

    @if($accounts->isEmpty())
        <p>No email accounts found. <a href="{{ route('email-accounts.create') }}">Create your first account</a></p>
    @else
        <table>
            <thead>
                <tr>
                    <th>Email</th>
                    <th>Name</th>
                    <th>Domain</th>
                    <th>Quota</th>
                    <th>Status</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                @foreach($accounts as $account)
                <tr>
                    <td><strong>{{ $account->email }}</strong></td>
                    <td>{{ $account->name ?? '-' }}</td>
                    <td>{{ $account->domain->domain }}</td>
                    <td>{{ $account->quota == 0 ? 'Unlimited' : number_format($account->quota / 1048576, 0) . ' MB' }}</td>
                    <td>
                        <span class="badge {{ $account->active ? 'badge-success' : 'badge-danger' }}">
                            {{ $account->active ? 'Active' : 'Inactive' }}
                        </span>
                    </td>
                    <td class="actions">
                        <a href="{{ route('email-accounts.setup', $account) }}" class="btn btn-sm btn-success">Setup</a>
                        <a href="{{ route('email-accounts.edit', $account) }}" class="btn btn-sm">Edit</a>
                        <form action="{{ route('email-accounts.destroy', $account) }}" method="POST" style="display: inline;">
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
