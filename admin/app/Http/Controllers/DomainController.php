<?php

namespace App\Http\Controllers;

use App\Models\Domain;
use App\Services\DkimService;
use Illuminate\Http\Request;
use Illuminate\View\View;
use Illuminate\Http\RedirectResponse;
use Illuminate\Support\Facades\Log;

class DomainController extends Controller
{
    protected $dkimService;

    public function __construct(DkimService $dkimService)
    {
        $this->dkimService = $dkimService;
    }
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
            'generate_dkim' => 'boolean',
            'dkim_selector' => 'nullable|string|max:255',
            'dkim_private_key' => 'nullable|string',
            'dkim_public_key' => 'nullable|string',
        ]);

        $validated['active'] = $request->has('active');
        
        // Auto-generate DKIM keys if requested
        if ($request->has('generate_dkim') && $request->generate_dkim) {
            $selector = $validated['dkim_selector'] ?: 'mail';
            $dkimResult = $this->generateAndStoreDkimKeys($validated['domain'], $selector);
            
            if ($dkimResult['success']) {
                $validated['dkim_selector'] = $selector;
                $validated['dkim_private_key'] = $dkimResult['private_key'];
                $validated['dkim_public_key'] = $dkimResult['public_key'];
                
                $domain = Domain::create($validated);
                
                return redirect()->route('domains.show-dkim', $domain)
                    ->with('success', 'Domain created successfully with DKIM keys');
            } else {
                return back()->withInput()
                    ->with('error', 'Failed to generate DKIM keys: ' . $dkimResult['error']);
            }
        }
        
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
            'dkim_selector' => 'nullable|string|max:255',
            'dkim_private_key' => 'nullable|string',
            'dkim_public_key' => 'nullable|string',
        ]);

        $validated['active'] = $request->has('active');
        
        // If DKIM private key is empty, keep the existing one
        if (empty($validated['dkim_private_key'])) {
            unset($validated['dkim_private_key']);
        }
        
        $domain->update($validated);
        
        return redirect()->route('domains.index')->with('success', 'Domain updated successfully');
    }

    public function destroy(Domain $domain): RedirectResponse
    {
        $domain->delete();
        return redirect()->route('domains.index')->with('success', 'Domain deleted successfully');
    }

    /**
     * Show DKIM information for a domain
     */
    public function showDkim(Domain $domain): View
    {
        return view('domains.show-dkim', compact('domain'));
    }

    /**
     * Generate or regenerate DKIM keys for a domain
     */
    public function generateDkim(Request $request, Domain $domain): RedirectResponse
    {
        $validated = $request->validate([
            'dkim_selector' => 'nullable|string|max:255',
        ]);

        $selector = $validated['dkim_selector'] ?? $domain->dkim_selector ?? 'mail';
        $dkimResult = $this->generateAndStoreDkimKeys($domain->domain, $selector);
        
        if ($dkimResult['success']) {
            $domain->update([
                'dkim_selector' => $selector,
                'dkim_private_key' => $dkimResult['private_key'],
                'dkim_public_key' => $dkimResult['public_key'],
            ]);

            return redirect()->route('domains.show-dkim', $domain)
                ->with('success', 'DKIM keys generated successfully');
        } else {
            return back()->with('error', 'Failed to generate DKIM keys: ' . $dkimResult['error']);
        }
    }

    /**
     * Generate DKIM keys and write to OpenDKIM directory
     * 
     * @param string $domainName
     * @param string $selector
     * @return array ['success' => bool, 'private_key' => string, 'public_key' => string, 'error' => string]
     */
    private function generateAndStoreDkimKeys(string $domainName, string $selector): array
    {
        try {
            $keys = $this->dkimService->generateKeys($domainName, $selector);
            
            // Write keys to OpenDKIM directory structure
            $this->dkimService->writeKeysToOpendkim(
                $domainName,
                $selector,
                $keys['private_key']
            );
            
            return [
                'success' => true,
                'private_key' => $keys['private_key'],
                'public_key' => $keys['public_key'],
            ];
        } catch (\Exception $e) {
            Log::error("Failed to generate DKIM keys for {$domainName}: " . $e->getMessage());
            return [
                'success' => false,
                'error' => $e->getMessage(),
            ];
        }
    }
}
