<?php

namespace App\Http\Controllers;

use App\Models\Domain;
use Illuminate\Http\Request;
use Illuminate\Http\JsonResponse;

class DomainController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index(): JsonResponse
    {
        $domains = Domain::with('emailAccounts', 'aliases')->get();
        return response()->json($domains);
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'domain' => 'required|string|unique:domains',
            'description' => 'nullable|string',
            'active' => 'boolean',
            'dkim_selector' => 'string',
        ]);

        $domain = Domain::create($validated);
        return response()->json($domain, 201);
    }

    /**
     * Display the specified resource.
     */
    public function show(Domain $domain): JsonResponse
    {
        $domain->load('emailAccounts', 'aliases');
        return response()->json($domain);
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, Domain $domain): JsonResponse
    {
        $validated = $request->validate([
            'domain' => 'string|unique:domains,domain,' . $domain->id,
            'description' => 'nullable|string',
            'active' => 'boolean',
            'dkim_selector' => 'string',
            'dkim_private_key' => 'nullable|string',
            'dkim_public_key' => 'nullable|string',
        ]);

        $domain->update($validated);
        return response()->json($domain);
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy(Domain $domain): JsonResponse
    {
        $domain->delete();
        return response()->json(['message' => 'Domain deleted successfully']);
    }
}
