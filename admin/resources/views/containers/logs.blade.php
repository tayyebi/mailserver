@extends('layouts.app')

@section('title', 'Container Logs - ' . $container)

@section('content')
<div class="card">
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem;">
        <h2>Logs: {{ $container }}</h2>
        <a href="{{ route('containers.index') }}" class="btn">← Back to Containers</a>
    </div>

    <div style="margin-bottom: 1rem;">
        <form method="GET" style="display: flex; gap: 0.5rem; align-items: center;">
            <label for="lines">Lines to display:</label>
            <select name="lines" id="lines" onchange="this.form.submit()" style="width: auto;">
                <option value="50" {{ $lines == 50 ? 'selected' : '' }}>50</option>
                <option value="100" {{ $lines == 100 ? 'selected' : '' }}>100</option>
                <option value="200" {{ $lines == 200 ? 'selected' : '' }}>200</option>
                <option value="500" {{ $lines == 500 ? 'selected' : '' }}>500</option>
                <option value="1000" {{ $lines == 1000 ? 'selected' : '' }}>1000</option>
            </select>
            <button type="submit" class="btn btn-sm">Refresh</button>
        </form>
    </div>

    <div style="
        background: #1e293b;
        color: #e2e8f0;
        padding: 1rem;
        border-radius: 4px;
        font-family: 'Courier New', monospace;
        font-size: 0.875rem;
        overflow-x: auto;
        max-height: 600px;
        overflow-y: auto;
    ">
        <pre style="margin: 0; white-space: pre-wrap; word-wrap: break-word;">{{ $logs }}</pre>
    </div>

    <div style="background: #f0f9ff; padding: 1rem; border-radius: 4px; margin-top: 1rem; border-left: 4px solid #0ea5e9;">
        <strong>ℹ️ Log Tips:</strong>
        <ul style="margin: 0.5rem 0 0 1.5rem;">
            <li>Logs are displayed in chronological order (most recent at the bottom)</li>
            <li>Use the dropdown to adjust the number of lines displayed</li>
            <li>Click Refresh to get the latest logs</li>
        </ul>
    </div>
</div>
@endsection
