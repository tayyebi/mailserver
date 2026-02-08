<?php

namespace App\Http\Controllers;

use App\Models\EmailAccount;
use App\Models\Domain;
use Illuminate\Http\Request;
use Illuminate\View\View;
use Illuminate\Http\RedirectResponse;

class EmailAccountController extends Controller
{
    public function index(): View
    {
        $accounts = EmailAccount::with('domain')->get();
        return view('email-accounts.index', compact('accounts'));
    }

    public function create(): View
    {
        $domains = Domain::where('active', true)->get();
        return view('email-accounts.create', compact('domains'));
    }

    public function store(Request $request): RedirectResponse
    {
        $validated = $request->validate([
            'domain_id' => 'required|exists:domains,id',
            'username' => 'required|string',
            'email' => 'required|email|unique:email_accounts',
            'password' => 'required|string|min:8',
            'name' => 'nullable|string',
            'quota' => 'integer|min:0',
        ]);

        // Convert quota from MB to bytes
        if (isset($validated['quota'])) {
            $validated['quota'] = $validated['quota'] * 1048576;
        }

        $validated['active'] = $request->has('active');
        EmailAccount::create($validated);
        
        return redirect()->route('email-accounts.index')->with('success', 'Email account created successfully');
    }

    public function edit(EmailAccount $emailAccount): View
    {
        $domains = Domain::where('active', true)->get();
        // Convert quota from bytes to MB for display
        $emailAccount->quota = $emailAccount->quota ? round($emailAccount->quota / 1048576) : 0;
        return view('email-accounts.edit', compact('emailAccount', 'domains'));
    }

    public function update(Request $request, EmailAccount $emailAccount): RedirectResponse
    {
        $validated = $request->validate([
            'domain_id' => 'exists:domains,id',
            'username' => 'string',
            'email' => 'email|unique:email_accounts,email,' . $emailAccount->id,
            'password' => 'nullable|string|min:8',
            'name' => 'nullable|string',
            'quota' => 'integer|min:0',
        ]);

        if (!isset($validated['password']) || empty($validated['password'])) {
            unset($validated['password']);
        }

        // Convert quota from MB to bytes
        if (isset($validated['quota'])) {
            $validated['quota'] = $validated['quota'] * 1048576;
        }

        $validated['active'] = $request->has('active');
        $emailAccount->update($validated);
        
        return redirect()->route('email-accounts.index')->with('success', 'Email account updated successfully');
    }

    public function destroy(EmailAccount $emailAccount): RedirectResponse
    {
        $emailAccount->delete();
        return redirect()->route('email-accounts.index')->with('success', 'Email account deleted successfully');
    }
}
