<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * Run the migrations.
     */
    public function up(): void
    {
        Schema::create('email_accounts', function (Blueprint $table) {
            $table->id();
            $table->foreignId('domain_id')->constrained('domains')->onDelete('cascade');
            $table->string('username'); // local part before @
            $table->string('email')->unique(); // full email address
            $table->string('password'); // hashed password for authentication
            $table->string('name')->nullable(); // Full name
            $table->boolean('active')->default(true);
            $table->bigInteger('quota')->default(0); // 0 = unlimited, in bytes
            $table->timestamps();
            
            $table->index(['email']);
            $table->index(['domain_id']);
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('email_accounts');
    }
};
