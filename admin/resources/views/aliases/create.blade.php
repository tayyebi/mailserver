@extends('layouts.app')

@section('title', 'Create Alias')

@section('content')
<div class="card">
    <h2>Create New Alias</h2>
    
    <form action="{{ route('aliases.store') }}" method="POST">
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
            <label for="source">Source Email *</label>
            <input type="text" id="source" name="source" required placeholder="alias@example.com" value="{{ old('source') }}">
            <small>Can be a full email or catch-all like @example.com</small>
            @error('source')
                <small style="color: red;">{{ $message }}</small>
            @enderror
        </div>

        <div class="form-group">
            <label for="destination">Destination Email *</label>
            <input type="text" id="destination" name="destination" required placeholder="user@example.com" value="{{ old('destination') }}">
            @error('destination')
                <small style="color: red;">{{ $message }}</small>
            @enderror
        </div>

        <div class="form-group">
            <label>
                <input type="checkbox" name="active" value="1" checked>
                Active
            </label>
        </div>

        <div class="actions">
            <button type="submit" class="btn btn-success">Create Alias</button>
            <a href="{{ route('aliases.index') }}" class="btn">Cancel</a>
        </div>
    </form>
</div>
@endsection
