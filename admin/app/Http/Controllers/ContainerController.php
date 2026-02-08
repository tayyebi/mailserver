<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use Illuminate\View\View;
use Illuminate\Http\RedirectResponse;
use Illuminate\Support\Facades\Log;

class ContainerController extends Controller
{
    /**
     * Display list of mailserver containers
     */
    public function index(): View
    {
        $containers = $this->getContainers();
        return view('containers.index', compact('containers'));
    }

    /**
     * Restart a container
     */
    public function restart(Request $request): RedirectResponse
    {
        $validated = $request->validate([
            'container' => 'required|string',
        ]);

        $container = $validated['container'];
        
        // Security: Only allow mailserver containers
        if (!$this->isMailserverContainer($container)) {
            return back()->with('error', 'Invalid container');
        }

        try {
            exec(sprintf('docker restart %s 2>&1', escapeshellarg($container)), $output, $returnCode);
            
            if ($returnCode === 0) {
                Log::info("Container restarted: {$container}");
                return back()->with('success', "Container {$container} restarted successfully");
            } else {
                Log::error("Failed to restart container {$container}: " . implode("\n", $output));
                return back()->with('error', 'Failed to restart container: ' . implode("\n", $output));
            }
        } catch (\Exception $e) {
            Log::error("Error restarting container {$container}: " . $e->getMessage());
            return back()->with('error', 'Error restarting container: ' . $e->getMessage());
        }
    }

    /**
     * Stop a container
     */
    public function stop(Request $request): RedirectResponse
    {
        $validated = $request->validate([
            'container' => 'required|string',
        ]);

        $container = $validated['container'];
        
        // Security: Only allow mailserver containers (exclude admin itself)
        if (!$this->isMailserverContainer($container) || $container === 'mailserver_admin') {
            return back()->with('error', 'Invalid container or cannot stop admin panel');
        }

        try {
            exec(sprintf('docker stop %s 2>&1', escapeshellarg($container)), $output, $returnCode);
            
            if ($returnCode === 0) {
                Log::info("Container stopped: {$container}");
                return back()->with('success', "Container {$container} stopped successfully");
            } else {
                Log::error("Failed to stop container {$container}: " . implode("\n", $output));
                return back()->with('error', 'Failed to stop container: ' . implode("\n", $output));
            }
        } catch (\Exception $e) {
            Log::error("Error stopping container {$container}: " . $e->getMessage());
            return back()->with('error', 'Error stopping container: ' . $e->getMessage());
        }
    }

    /**
     * Start a container
     */
    public function start(Request $request): RedirectResponse
    {
        $validated = $request->validate([
            'container' => 'required|string',
        ]);

        $container = $validated['container'];
        
        // Security: Only allow mailserver containers
        if (!$this->isMailserverContainer($container)) {
            return back()->with('error', 'Invalid container');
        }

        try {
            exec(sprintf('docker start %s 2>&1', escapeshellarg($container)), $output, $returnCode);
            
            if ($returnCode === 0) {
                Log::info("Container started: {$container}");
                return back()->with('success', "Container {$container} started successfully");
            } else {
                Log::error("Failed to start container {$container}: " . implode("\n", $output));
                return back()->with('error', 'Failed to start container: ' . implode("\n", $output));
            }
        } catch (\Exception $e) {
            Log::error("Error starting container {$container}: " . $e->getMessage());
            return back()->with('error', 'Error starting container: ' . $e->getMessage());
        }
    }

    /**
     * View container logs
     */
    public function logs(Request $request, string $container): View
    {
        // Security: Only allow mailserver containers
        if (!$this->isMailserverContainer($container)) {
            abort(404);
        }

        // Validate lines parameter
        $validated = $request->validate([
            'lines' => 'nullable|integer|min:1|max:10000',
        ]);

        $lines = $validated['lines'] ?? 100;
        $logs = $this->getContainerLogs($container, $lines);
        
        return view('containers.logs', compact('container', 'logs', 'lines'));
    }

    /**
     * Get list of mailserver containers
     */
    private function getContainers(): array
    {
        try {
            // Get containers with mailserver role label
            $cmd = 'docker ps -a --filter "label=com.mailserver.role" --format "{{.Names}}|{{.Status}}|{{.State}}|{{json .Labels}}" 2>&1';
            exec($cmd, $output, $returnCode);
            
            if ($returnCode !== 0) {
                Log::error("Failed to get containers: " . implode("\n", $output));
                return [];
            }

            $containers = [];
            foreach ($output as $line) {
                $parts = explode('|', $line, 4);
                if (count($parts) === 4) {
                    $labels = json_decode($parts[3], true);
                    $containers[] = [
                        'name' => $parts[0],
                        'status' => $parts[1],
                        'state' => $parts[2],
                        'role' => $labels['com.mailserver.role'] ?? 'unknown',
                    ];
                }
            }

            return $containers;
        } catch (\Exception $e) {
            Log::error("Error getting containers: " . $e->getMessage());
            return [];
        }
    }

    /**
     * Get container logs
     */
    private function getContainerLogs(string $container, int $lines = 100): string
    {
        try {
            // Validate lines is within bounds
            $lines = max(1, min(10000, (int)$lines));
            
            $cmd = sprintf('docker logs --tail %d %s 2>&1', $lines, escapeshellarg($container));
            exec($cmd, $output, $returnCode);
            
            if ($returnCode !== 0) {
                return "Failed to retrieve logs";
            }

            return implode("\n", $output);
        } catch (\Exception $e) {
            Log::error("Error getting logs for {$container}: " . $e->getMessage());
            return "Error retrieving logs";
        }
    }

    /**
     * Check if container is a mailserver container
     */
    private function isMailserverContainer(string $containerName): bool
    {
        $allowedContainers = [
            'mailserver_admin',
            'dovecot',
            'postfix',
            'opendkim',
            'pixelmilter',
            'pixelserver',
            'nginx_proxy',
        ];

        return in_array($containerName, $allowedContainers);
    }
}
