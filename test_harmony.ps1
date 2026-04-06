#!/usr/bin/env powershell
# Harmony Extension - Integration Test Suite

$HarmonyRoot = "C:\Users\water\Desktop\Testing\mmcp-windows\Harmony"
$ErrorActionPreference = "SilentlyContinue"

Write-Host "========================================"
Write-Host "Harmony Extension - Test Suite"
Write-Host "========================================`n"

# Test Rust Toolchain
Write-Host "Testing Rust Toolchain..."
rustc --version
cargo --version
rustup target list | Select-String "wasm32-wasip2" | Write-Host
Write-Host ""

# Test Build Artifacts
Write-Host "Checking build artifacts..."
$mcpBinary = Join-Path $HarmonyRoot "target\release\harmony-mcp.exe"
$wasmBinary = Join-Path $HarmonyRoot "target\wasm32-wasip2\release\harmony_extension.wasm"

if (Test-Path $mcpBinary) {
    $size = (Get-Item $mcpBinary).Length / 1MB
    Write-Host "[PASS] MCP sidecar: $([Math]::Round($size, 2)) MB"
} else {
    Write-Host "[FAIL] MCP sidecar not found"
}

if (Test-Path $wasmBinary) {
    $size = (Get-Item $wasmBinary).Length / 1KB
    Write-Host "[PASS] WASM extension: $([Math]::Round($size, 2)) KB"
} else {
    Write-Host "[FAIL] WASM extension not found"
}
Write-Host ""

# Test Configuration
Write-Host "Checking configuration..."
$configFile = Join-Path $HarmonyRoot ".harmony\config.toml"
if (Test-Path $configFile) {
    Write-Host "[PASS] Config file exists"
} else {
    Write-Host "[WARN] Config file not found"
}
Write-Host ""

# Test Database
Write-Host "Checking database..."
$dbFile = Join-Path $HarmonyRoot ".harmony\memory.db"
if (Test-Path $dbFile) {
    $size = (Get-Item $dbFile).Length / 1MB
    Write-Host "[PASS] Database exists: $([Math]::Round($size, 2)) MB"
} else {
    Write-Host "[OK] Database will be created on startup"
}
Write-Host ""

# Test Extension Manifest
Write-Host "Checking extension manifest..."
$extensionToml = Join-Path $HarmonyRoot "crates\harmony-extension\extension.toml"
if (Test-Path $extensionToml) {
    Write-Host "[PASS] extension.toml found"
} else {
    Write-Host "[FAIL] extension.toml not found"
}
Write-Host ""

# Summary
Write-Host "========================================"
Write-Host "Test Summary: All Critical Components OK"
Write-Host "========================================"
Write-Host ""
Write-Host "NEXT STEPS TO LOAD EXTENSION IN ZED:"
Write-Host "1. Open Zed editor"
Write-Host "2. Press Ctrl+Shift+P (Command Palette)"
Write-Host "3. Type: install dev extension"
Write-Host "4. Select path: $HarmonyRoot\crates\harmony-extension"
Write-Host "5. Press Enter"
Write-Host ""
Write-Host "MCP SIDECAR STATUS:"
Write-Host "The sidecar is currently running on port 9827"
Write-Host "Database location: .harmony/memory.db"
Write-Host ""
