<?php

use Illuminate\Support\Facades\Route;
use App\Http\Controllers\DashboardController;
use App\Http\Controllers\DomainController;
use App\Http\Controllers\EmailAccountController;
use App\Http\Controllers\AliasController;

Route::get('/', [DashboardController::class, 'index'])->name('dashboard');

Route::resource('domains', DomainController::class)->except(['show']);
Route::resource('email-accounts', EmailAccountController::class)->except(['show']);
Route::resource('aliases', AliasController::class)->except(['show']);
