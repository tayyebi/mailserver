<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use Illuminate\View\View;
use Illuminate\Http\RedirectResponse;
use Illuminate\Support\Facades\Log;

class FooterController extends Controller
{
    /**
     * Path to the domain-wide footer HTML file inside the container.
     * This maps to data/mail-config/domain-wide-footer.html on the host
     * via the bind mount in docker-compose.yml.
     */
    protected function footerPath(): string
    {
        return storage_path('app/mail-config/domain-wide-footer.html');
    }

    /**
     * Show the footer editor form.
     */
    public function edit(): View
    {
        $footerHtml = '';
        $path = $this->footerPath();

        if (file_exists($path)) {
            $footerHtml = file_get_contents($path);
        }

        return view('footer.edit', compact('footerHtml'));
    }

    /**
     * Save the updated footer HTML.
     */
    public function update(Request $request): RedirectResponse
    {
        $validated = $request->validate([
            'footer_html' => 'nullable|string|max:65535',
        ]);

        $path = $this->footerPath();
        $dir = dirname($path);

        // Ensure directory exists
        if (!is_dir($dir)) {
            if (!mkdir($dir, 0755, true)) {
                Log::error("Failed to create mail-config directory: {$dir}");
                return back()->with('error', 'Failed to create configuration directory.');
            }
        }

        $html = $validated['footer_html'] ?? '';

        // Write atomically: temp file then rename
        $tempFile = $path . '.tmp';
        try {
            if (file_put_contents($tempFile, $html) === false) {
                throw new \RuntimeException("Failed to write temp file: {$tempFile}");
            }
            if (!chmod($tempFile, 0644)) {
                throw new \RuntimeException("Failed to set permissions on: {$tempFile}");
            }
            if (!rename($tempFile, $path)) {
                throw new \RuntimeException("Failed to rename {$tempFile} to {$path}");
            }

            Log::info('Domain-wide footer updated', ['size' => strlen($html)]);

            return redirect()->route('footer.edit')
                ->with('success', 'Footer updated successfully. Restart pixelmilter for changes to take effect.');
        } catch (\Exception $e) {
            @unlink($tempFile);
            Log::error('Failed to save footer', ['error' => $e->getMessage()]);
            return back()->withInput()
                ->with('error', 'Failed to save footer: ' . $e->getMessage());
        }
    }
}
