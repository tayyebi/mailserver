@extends('layouts.app')

@section('title', 'Container Management')

@section('content')
<div class="card">
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem;">
        <h2>Container Management</h2>
    </div>

    <p style="color: #666; margin-bottom: 1.5rem;">
        Manage mail server containers. Monitor status, restart services, and view logs.
    </p>

    @if(empty($containers))
        <div style="background: #fff3cd; padding: 1rem; border-radius: 4px; margin-bottom: 1rem;">
            <strong>⚠ Unable to retrieve container information</strong>
            <p style="margin-top: 0.5rem;">Make sure Docker socket is mounted and accessible.</p>
        </div>
    @else
        <table>
            <thead>
                <tr>
                    <th>Container</th>
                    <th>Role</th>
                    <th>Status</th>
                    <th>State</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody>
                @foreach($containers as $container)
                <tr>
                    <td><strong>{{ $container['name'] }}</strong></td>
                    <td>
                        <span style="
                            display: inline-block;
                            padding: 0.25rem 0.5rem;
                            border-radius: 3px;
                            font-size: 0.875rem;
                            background: #e0e7ff;
                            color: #3730a3;
                        ">
                            {{ ucfirst($container['role']) }}
                        </span>
                    </td>
                    <td>{{ $container['status'] }}</td>
                    <td>
                        @if($container['state'] === 'running')
                            <span class="badge badge-success">Running</span>
                        @elseif($container['state'] === 'exited')
                            <span class="badge badge-danger">Stopped</span>
                        @else
                            <span class="badge">{{ ucfirst($container['state']) }}</span>
                        @endif
                    </td>
                    <td class="actions">
                        <a href="{{ route('containers.logs', $container['name']) }}" class="btn btn-sm" style="background: #6366f1; color: white;">
                            View Logs
                        </a>
                        
                        @if($container['state'] === 'running')
                            <form action="{{ route('containers.restart') }}" method="POST" style="display: inline;">
                                @csrf
                                <input type="hidden" name="container" value="{{ $container['name'] }}">
                                <button type="submit" class="btn btn-sm" style="background: #f59e0b; color: white;" 
                                    onclick="return confirm('Restart {{ $container['name'] }}?')">
                                    Restart
                                </button>
                            </form>
                            
                            @if($container['name'] !== 'mailserver_admin')
                                <form action="{{ route('containers.stop') }}" method="POST" style="display: inline;">
                                    @csrf
                                    <input type="hidden" name="container" value="{{ $container['name'] }}">
                                    <button type="submit" class="btn btn-sm btn-danger" 
                                        onclick="return confirm('Stop {{ $container['name'] }}?')">
                                        Stop
                                    </button>
                                </form>
                            @endif
                        @else
                            <form action="{{ route('containers.start') }}" method="POST" style="display: inline;">
                                @csrf
                                <input type="hidden" name="container" value="{{ $container['name'] }}">
                                <button type="submit" class="btn btn-sm btn-success" 
                                    onclick="return confirm('Start {{ $container['name'] }}?')">
                                    Start
                                </button>
                            </form>
                        @endif
                    </td>
                </tr>
                @endforeach
            </tbody>
        </table>

        <div style="background: #f0f9ff; padding: 1rem; border-radius: 4px; margin-top: 1.5rem; border-left: 4px solid #0ea5e9;">
            <strong>ℹ️ Container Management Notes:</strong>
            <ul style="margin: 0.5rem 0 0 1.5rem;">
                <li>The admin panel cannot stop itself</li>
                <li>Restarting containers may cause temporary service interruption</li>
                <li>Check logs if a container is frequently restarting</li>
            </ul>
        </div>
    @endif
</div>
@endsection
