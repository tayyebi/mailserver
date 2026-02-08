<?php

namespace App\Http\Controllers;

use App\Models\EmailAccount;
use Illuminate\Http\Request;
use Illuminate\Http\JsonResponse;

class EmailAccountController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index(): JsonResponse
    {
        $accounts = EmailAccount::with('domain')->get();
        return response()->json($accounts);
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'domain_id' => 'required|exists:domains,id',
            'username' => 'required|string',
            'email' => 'required|email|unique:email_accounts',
            'password' => 'required|string|min:8',
            'name' => 'nullable|string',
            'active' => 'boolean',
            'quota' => 'integer|min:0',
        ]);

        $account = EmailAccount::create($validated);
        return response()->json($account, 201);
    }

    /**
     * Display the specified resource.
     */
    public function show(EmailAccount $emailAccount): JsonResponse
    {
        $emailAccount->load('domain');
        return response()->json($emailAccount);
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, EmailAccount $emailAccount): JsonResponse
    {
        $validated = $request->validate([
            'domain_id' => 'exists:domains,id',
            'username' => 'string',
            'email' => 'email|unique:email_accounts,email,' . $emailAccount->id,
            'password' => 'nullable|string|min:8',
            'name' => 'nullable|string',
            'active' => 'boolean',
            'quota' => 'integer|min:0',
        ]);

        // Only update password if provided
        if (!isset($validated['password']) || empty($validated['password'])) {
            unset($validated['password']);
        }

        $emailAccount->update($validated);
        return response()->json($emailAccount);
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy(EmailAccount $emailAccount): JsonResponse
    {
        $emailAccount->delete();
        return response()->json(['message' => 'Email account deleted successfully']);
    }
}
