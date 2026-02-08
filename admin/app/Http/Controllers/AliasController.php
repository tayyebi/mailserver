<?php

namespace App\Http\Controllers;

use App\Models\Alias;
use App\Models\Domain;
use Illuminate\Http\Request;
use Illuminate\View\View;
use Illuminate\Http\RedirectResponse;

class AliasController extends Controller
{
    public function index(): View
    {
        $aliases = Alias::with('domain')->get();
        return view('aliases.index', compact('aliases'));
    }

    public function create(): View
    {
        $domains = Domain::where('active', true)->get();
        return view('aliases.create', compact('domains'));
    }

    public function store(Request $request): RedirectResponse
    {
        $validated = $request->validate([
            'domain_id' => 'required|exists:domains,id',
            'source' => 'required|string',
            'destination' => 'required|string',
        ]);

        $validated['active'] = $request->has('active');
        Alias::create($validated);
        
        return redirect()->route('aliases.index')->with('success', 'Alias created successfully');
    }

    public function show(Alias $alias): View
    {
        $alias->load('domain');
        return view('aliases.show', compact('alias'));
    }

    public function edit(Alias $alias): View
    {
        $domains = Domain::where('active', true)->get();
        return view('aliases.edit', compact('alias', 'domains'));
    }

    public function update(Request $request, Alias $alias): RedirectResponse
    {
        $validated = $request->validate([
            'domain_id' => 'exists:domains,id',
            'source' => 'string',
            'destination' => 'string',
        ]);

        $validated['active'] = $request->has('active');
        $alias->update($validated);
        
        return redirect()->route('aliases.index')->with('success', 'Alias updated successfully');
    }

    public function destroy(Alias $alias): RedirectResponse
    {
        $alias->delete();
        return redirect()->route('aliases.index')->with('success', 'Alias deleted successfully');
    }
}
