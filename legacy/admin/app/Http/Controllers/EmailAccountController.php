<?php

namespace App\Http\Controllers;

use App\Models\Alias;
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
            'set_as_catch_all' => 'sometimes|boolean',
        ]);

        // Convert quota from MB to bytes
        if (isset($validated['quota'])) {
            $validated['quota'] = $validated['quota'] * 1048576;
        }

        $validated['active'] = $request->has('active');
        $account = EmailAccount::create($validated);

        if ($request->boolean('set_as_catch_all')) {
            $this->setCatchAllForAccount($account);
        }

        return redirect()->route('email-accounts.index')->with('success', 'Email account created successfully');
    }

    public function edit(EmailAccount $emailAccount): View
    {
        $domains = Domain::where('active', true)->get();
        // Ensure domain relation is loaded
        $emailAccount->load('domain');

        // Convert quota from bytes to MB for display
        $emailAccount->quota = $emailAccount->quota ? round($emailAccount->quota / 1048576) : 0;

        $currentCatchAll = null;
        if ($emailAccount->domain) {
            $currentCatchAll = Alias::where('domain_id', $emailAccount->domain_id)
                ->where('source', '@' . $emailAccount->domain->domain)
                ->first();
        }

        $isCatchAllForThisAccount = $currentCatchAll && $currentCatchAll->destination === $emailAccount->email;

        return view('email-accounts.edit', compact('emailAccount', 'domains', 'isCatchAllForThisAccount'));
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
            'set_as_catch_all' => 'sometimes|boolean',
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

        if ($request->boolean('set_as_catch_all')) {
            $this->setCatchAllForAccount($emailAccount);
        }

        return redirect()->route('email-accounts.index')->with('success', 'Email account updated successfully');
    }

    /**
     * Point the domain-level catch-all alias to the given account.
     */
    protected function setCatchAllForAccount(EmailAccount $account): void
    {
        $account->loadMissing('domain');

        if (!$account->domain) {
            return;
        }

        $source = '@' . $account->domain->domain;

        $alias = Alias::where('domain_id', $account->domain_id)
            ->where('source', $source)
            ->first();

        if ($alias) {
            $alias->update([
                'destination' => $account->email,
                'active' => true,
            ]);
        } else {
            Alias::create([
                'domain_id' => $account->domain_id,
                'source' => $source,
                'destination' => $account->email,
                'active' => true,
            ]);
        }
    }

    public function destroy(EmailAccount $emailAccount): RedirectResponse
    {
        $emailAccount->delete();
        return redirect()->route('email-accounts.index')->with('success', 'Email account deleted successfully');
    }

    public function setup(EmailAccount $emailAccount): View
    {
        $emailAccount->load('domain');

        $connection = [
            'host'  => config('mailserver.host'),
            'ports' => config('mailserver.ports'),
            'email' => $emailAccount->email,
            'name'  => $emailAccount->name,
        ];

        return view('email-accounts.setup', compact('emailAccount', 'connection'));
    }
}
