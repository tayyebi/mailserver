<?php

namespace App\Http\Controllers;

use App\Models\Alias;
use Illuminate\Http\Request;
use Illuminate\Http\JsonResponse;

class AliasController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index(): JsonResponse
    {
        $aliases = Alias::with('domain')->get();
        return response()->json($aliases);
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'domain_id' => 'required|exists:domains,id',
            'source' => 'required|string',
            'destination' => 'required|string',
            'active' => 'boolean',
        ]);

        $alias = Alias::create($validated);
        return response()->json($alias, 201);
    }

    /**
     * Display the specified resource.
     */
    public function show(Alias $alias): JsonResponse
    {
        $alias->load('domain');
        return response()->json($alias);
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, Alias $alias): JsonResponse
    {
        $validated = $request->validate([
            'domain_id' => 'exists:domains,id',
            'source' => 'string',
            'destination' => 'string',
            'active' => 'boolean',
        ]);

        $alias->update($validated);
        return response()->json($alias);
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy(Alias $alias): JsonResponse
    {
        $alias->delete();
        return response()->json(['message' => 'Alias deleted successfully']);
    }
}
