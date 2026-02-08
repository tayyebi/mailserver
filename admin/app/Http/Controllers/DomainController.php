<?php

namespace App\Http\Controllers;

use App\Models\Domain;
use Illuminate\Http\Request;
use Illuminate\View\View;
use Illuminate\Http\RedirectResponse;

class DomainController extends Controller
{
    public function index(): View
    {
        $domains = Domain::with('emailAccounts', 'aliases')->get();
        return view('domains.index', compact('domains'));
    }

    public function create(): View
    {
        return view('domains.create');
    }

    public function store(Request $request): RedirectResponse
    {
        $validated = $request->validate([
            'domain' => 'required|string|unique:domains',
            'description' => 'nullable|string',
            'active' => 'boolean',
        ]);

        $validated['active'] = $request->has('active');
        Domain::create($validated);
        
        return redirect()->route('domains.index')->with('success', 'Domain created successfully');
    }

    public function show(Domain $domain): View
    {
        $domain->load('emailAccounts', 'aliases');
        return view('domains.show', compact('domain'));
    }

    public function edit(Domain $domain): View
    {
        return view('domains.edit', compact('domain'));
    }

    public function update(Request $request, Domain $domain): RedirectResponse
    {
        $validated = $request->validate([
            'domain' => 'string|unique:domains,domain,' . $domain->id,
            'description' => 'nullable|string',
        ]);

        $validated['active'] = $request->has('active');
        $domain->update($validated);
        
        return redirect()->route('domains.index')->with('success', 'Domain updated successfully');
    }

    public function destroy(Domain $domain): RedirectResponse
    {
        $domain->delete();
        return redirect()->route('domains.index')->with('success', 'Domain deleted successfully');
    }
}
