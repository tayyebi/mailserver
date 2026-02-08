<?php

use Illuminate\Http\Request;
use Illuminate\Support\Facades\Route;
use App\Http\Controllers\DomainController;
use App\Http\Controllers\EmailAccountController;
use App\Http\Controllers\AliasController;
use App\Http\Controllers\DashboardController;
use App\Http\Controllers\AuthController;

// Public routes
Route::post('/login', [AuthController::class, 'login']);

// Protected routes
Route::middleware('auth:sanctum')->group(function () {
    // Auth routes
    Route::post('/logout', [AuthController::class, 'logout']);
    Route::get('/me', [AuthController::class, 'me']);
    
    // Dashboard
    Route::get('/dashboard', [DashboardController::class, 'index']);
    
    // Resource routes
    Route::apiResource('domains', DomainController::class);
    Route::apiResource('email-accounts', EmailAccountController::class);
    Route::apiResource('aliases', AliasController::class);
});
