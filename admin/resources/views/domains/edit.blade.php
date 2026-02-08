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

        <div class="actions">
            <button type="submit" class="btn btn-success">Update Domain</button>
            <a href="{{ route('domains.index') }}" class="btn">Cancel</a>
        </div>
    </form>
</div>
@endsection
