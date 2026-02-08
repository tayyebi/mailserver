<?php

namespace App\Http\Controllers;

use App\Models\Domain;
use App\Models\EmailAccount;
use App\Models\Alias;
use Illuminate\View\View;

class DashboardController extends Controller
{
    public function index(): View
    {
        $stats = [
            'total_domains' => Domain::count(),
            'active_domains' => Domain::where('active', true)->count(),
            'total_email_accounts' => EmailAccount::count(),
            'active_email_accounts' => EmailAccount::where('active', true)->count(),
            'total_aliases' => Alias::count(),
            'active_aliases' => Alias::where('active', true)->count(),
        ];

        return view('dashboard', compact('stats'));
    }
}
