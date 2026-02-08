<?php

namespace App\Http\Controllers;

use App\Models\Domain;
use App\Models\EmailAccount;
use App\Models\Alias;
use Illuminate\Http\JsonResponse;

class DashboardController extends Controller
{
    /**
     * Display dashboard statistics.
     */
    public function index(): JsonResponse
    {
        $stats = [
            'total_domains' => Domain::count(),
            'active_domains' => Domain::where('active', true)->count(),
            'total_email_accounts' => EmailAccount::count(),
            'active_email_accounts' => EmailAccount::where('active', true)->count(),
            'total_aliases' => Alias::count(),
            'active_aliases' => Alias::where('active', true)->count(),
            'recent_domains' => Domain::latest()->take(5)->get(),
            'recent_accounts' => EmailAccount::with('domain')->latest()->take(5)->get(),
        ];

        return response()->json($stats);
    }
}
