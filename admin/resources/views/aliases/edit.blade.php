@extends('layouts.app')

@section('title', 'Edit Alias')

@section('content')
<div class="card">
    <h2>Edit Alias</h2>
    
    <form action="{{ route('aliases.update', $alias) }}" method="POST">
        @csrf
        @method('PUT')
        
        <div class="form-group">
            <label for="domain_id">Domain *</label>
            <select id="domain_id" name="domain_id" required>
                @foreach($domains as $domain)
                    <option value="{{ $domain->id }}" {{ old('domain_id', $alias->domain_id) == $domain->id ? 'selected' : '' }}>
                        {{ $domain->domain }}
                    </option>
                @endforeach
            </select>
        </div>

        <div class="form-group">
            <label for="source">Source Email *</label>
            <input type="text" id="source" name="source" required value="{{ old('source', $alias->source) }}">
        </div>

        <div class="form-group">
            <label for="destination">Destination Email *</label>
            <input type="text" id="destination" name="destination" required value="{{ old('destination', $alias->destination) }}">
        </div>

        <div class="form-group">
            <label>
                <input type="checkbox" name="active" value="1" {{ old('active', $alias->active) ? 'checked' : '' }}>
                Active
            </label>
        </div>

        <div class="actions">
            <button type="submit" class="btn btn-success">Update Alias</button>
            <a href="{{ route('aliases.index') }}" class="btn">Cancel</a>
        </div>
    </form>
</div>
@endsection
