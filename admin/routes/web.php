<?php

use Illuminate\Support\Facades\Route;
use App\Http\Controllers\DashboardController;
use App\Http\Controllers\DomainController;
use App\Http\Controllers\EmailAccountController;
use App\Http\Controllers\AliasController;

Route::get('/', [DashboardController::class, 'index'])->name('dashboard');

Route::resource('domains', DomainController::class);
Route::resource('email-accounts', EmailAccountController::class);
Route::resource('aliases', AliasController::class);
