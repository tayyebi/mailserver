<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Mail Server Admin - @yield('title')</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { font-family: Arial, sans-serif; line-height: 1.6; color: #333; background: #f4f4f4; }
        .container { max-width: 1200px; margin: 0 auto; padding: 20px; }
        header { background: #2c3e50; color: white; padding: 1rem 0; margin-bottom: 2rem; }
        header .container { display: flex; justify-content: space-between; align-items: center; }
        header h1 { font-size: 1.5rem; }
        nav a { color: white; text-decoration: none; margin-left: 1rem; padding: 0.5rem 1rem; border-radius: 4px; }
        nav a:hover { background: #34495e; }
        .card { background: white; border-radius: 8px; padding: 1.5rem; margin-bottom: 1.5rem; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 1rem; }
        .stat-box { background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 1.5rem; border-radius: 8px; text-align: center; }
        .stat-box h3 { font-size: 2rem; margin-bottom: 0.5rem; }
        .stat-box p { opacity: 0.9; }
        table { width: 100%; border-collapse: collapse; }
        th, td { padding: 0.75rem; text-align: left; border-bottom: 1px solid #ddd; }
        th { background: #f8f9fa; font-weight: 600; }
        tr:hover { background: #f8f9fa; }
        .btn { display: inline-block; padding: 0.5rem 1rem; background: #667eea; color: white; text-decoration: none; border-radius: 4px; border: none; cursor: pointer; }
        .btn:hover { background: #5568d3; }
        .btn-sm { padding: 0.25rem 0.75rem; font-size: 0.875rem; }
        .btn-danger { background: #e74c3c; }
        .btn-danger:hover { background: #c0392b; }
        .btn-success { background: #27ae60; }
        .btn-success:hover { background: #229954; }
        .form-group { margin-bottom: 1rem; }
        .form-group label { display: block; margin-bottom: 0.25rem; font-weight: 600; }
        .form-group input, .form-group select, .form-group textarea { width: 100%; padding: 0.5rem; border: 1px solid #ddd; border-radius: 4px; }
        .form-group input[type="checkbox"] { width: auto; margin-right: 0.5rem; }
        .alert { padding: 1rem; margin-bottom: 1rem; border-radius: 4px; }
        .alert-success { background: #d4edda; color: #155724; border: 1px solid #c3e6cb; }
        .badge { display: inline-block; padding: 0.25rem 0.5rem; border-radius: 4px; font-size: 0.75rem; }
        .badge-success { background: #d4edda; color: #155724; }
        .badge-danger { background: #f8d7da; color: #721c24; }
        .actions { display: flex; gap: 0.5rem; }
    </style>
</head>
<body>
    <header>
        <div class="container">
            <h1>📧 Mail Server Admin</h1>
            <nav>
                <a href="{{ route('dashboard') }}">Dashboard</a>
                <a href="{{ route('domains.index') }}">Domains</a>
                <a href="{{ route('email-accounts.index') }}">Email Accounts</a>
                <a href="{{ route('aliases.index') }}">Aliases</a>
                <a href="{{ route('footer.edit') }}">Footer</a>
                <a href="{{ route('containers.index') }}">Containers</a>
            </nav>
        </div>
    </header>

    <div class="container">
        @if(session('success'))
            <div class="alert alert-success">{{ session('success') }}</div>
        @endif
        
        @if(session('error'))
            <div class="alert" style="background: #f8d7da; color: #721c24; border: 1px solid #f5c6cb;">{{ session('error') }}</div>
        @endif

        @yield('content')
    </div>
</body>
</html>
