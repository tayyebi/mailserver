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

        <div class="actions">
            <button type="submit" class="btn btn-success">Create Domain</button>
            <a href="{{ route('domains.index') }}" class="btn">Cancel</a>
        </div>
    </form>
</div>
@endsection
