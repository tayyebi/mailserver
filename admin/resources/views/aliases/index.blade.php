@extends('layouts.app')

@section('title', 'Email Aliases')

@section('content')
<div class="card">
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem;">
        <h2>Email Aliases</h2>
        <a href="{{ route('aliases.create') }}" class="btn">+ Add Alias</a>
    </div>

    @if($aliases->isEmpty())
        <p>No aliases found. <a href="{{ route('aliases.create') }}">Create your first alias</a></p>
    @else
        <table>
            <thead>
                <tr>
                    <th>Source</th>
                    <th>Destination</th>
                    <th>Domain</th>
                    <th>Status</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                @foreach($aliases as $alias)
                <tr>
                    <td><strong>{{ $alias->source }}</strong></td>
                    <td>{{ $alias->destination }}</td>
                    <td>{{ $alias->domain->domain }}</td>
                    <td>
                        <span class="badge {{ $alias->active ? 'badge-success' : 'badge-danger' }}">
                            {{ $alias->active ? 'Active' : 'Inactive' }}
                        </span>
                        @if(substr($alias->source, 0, 1) === '@')
                            <span class="badge badge-info" title="Domain-wide catch-all alias">
                                Catch-all
                            </span>
                        @endif
                    </td>
                    <td class="actions">
                        <a href="{{ route('aliases.edit', $alias) }}" class="btn btn-sm">Edit</a>
                        <form action="{{ route('aliases.destroy', $alias) }}" method="POST" style="display: inline;">
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
