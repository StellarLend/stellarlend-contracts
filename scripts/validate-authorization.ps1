# Authorization and Validation Testing Script (PowerShell)
# Runs comprehensive validation checks for authorization boundaries

$ErrorActionPreference = "Continue"

Write-Host "==================================" -ForegroundColor Cyan
Write-Host "Authorization Boundary Validation" -ForegroundColor Cyan
Write-Host "==================================" -ForegroundColor Cyan
Write-Host ""

# Test counters
$global:TotalTests = 0
$global:PassedTests = 0
$global:FailedTests = 0

function Run-Test {
    param(
        [string]$TestName,
        [scriptblock]$TestCommand
    )
    
    Write-Host "Running: $TestName" -ForegroundColor Yellow
    $global:TotalTests++
    
    try {
        & $TestCommand
        if ($LASTEXITCODE -eq 0) {
            Write-Host "✓ $TestName passed" -ForegroundColor Green
            $global:PassedTests++
        } else {
            Write-Host "✗ $TestName failed" -ForegroundColor Red
            $global:FailedTests++
        }
    } catch {
        Write-Host "✗ $TestName failed with exception: $_" -ForegroundColor Red
        $global:FailedTests++
    }
    Write-Host ""
}

# Change to project root
$scriptPath = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location "$scriptPath\.."

Write-Host "Step 1: Contract Authorization Tests" -ForegroundColor Cyan
Write-Host "------------------------------------" -ForegroundColor Cyan

Run-Test "Authorization module tests" {
    Set-Location "stellar-lend\contracts\lending"
    cargo test authorization:: --lib -- --test-threads=1
    Set-Location "..\..\.."
}

Run-Test "Validation module tests" {
    Set-Location "stellar-lend\contracts\lending"
    cargo test validation:: --lib -- --test-threads=1
    Set-Location "..\..\.."
}

Run-Test "Adversarial scenario tests" {
    Set-Location "stellar-lend\contracts\lending"
    cargo test adversarial_scenarios_test:: -- --test-threads=1
    Set-Location "..\..\.."
}

Run-Test "Event schema versioning tests" {
    Set-Location "stellar-lend\contracts\lending"
    cargo test event_schema_versioning_test:: -- --test-threads=1
    Set-Location "..\..\.."
}

Write-Host ""
Write-Host "Step 2: API Boundary Validation Tests" -ForegroundColor Cyan
Write-Host "--------------------------------------" -ForegroundColor Cyan

Run-Test "API authorization middleware tests" {
    Set-Location "api"
    npm test -- "src/__tests__/auth.test.ts" --passWithNoTests
    Set-Location ".."
}

Run-Test "API boundary validation tests" {
    Set-Location "api"
    npm test -- "src/__tests__/boundaryValidation.test.ts"
    Set-Location ".."
}

Write-Host ""
Write-Host "Step 3: Integration Tests" -ForegroundColor Cyan
Write-Host "-------------------------" -ForegroundColor Cyan

Run-Test "Full integration test suite" {
    Set-Location "stellar-lend\contracts\lending"
    cargo test integration -- --test-threads=1
    Set-Location "..\..\.."
}

Write-Host ""
Write-Host "Step 4: Security Checks" -ForegroundColor Cyan
Write-Host "-----------------------" -ForegroundColor Cyan

Run-Test "Replay attack prevention" {
    Set-Location "stellar-lend\contracts\lending"
    cargo test test_replay -- --test-threads=1
    Set-Location "..\..\.."
}

Run-Test "Tampering prevention" {
    Set-Location "stellar-lend\contracts\lending"
    cargo test test_cannot -- --test-threads=1
    Set-Location "..\..\.."
}

Run-Test "Network validation" {
    Set-Location "stellar-lend\contracts\lending"
    cargo test test_network -- --test-threads=1
    Set-Location "..\..\.."
}

Run-Test "Rate limiting" {
    Set-Location "stellar-lend\contracts\lending"
    cargo test test_rate_limit -- --test-threads=1
    Set-Location "..\..\.."
}

Write-Host ""
Write-Host "Step 5: Static Analysis" -ForegroundColor Cyan
Write-Host "-----------------------" -ForegroundColor Cyan

Run-Test "Clippy lints" {
    Set-Location "stellar-lend\contracts\lending"
    cargo clippy --all-targets --all-features -- -D warnings
    Set-Location "..\..\.."
}

Run-Test "Format check" {
    Set-Location "stellar-lend\contracts\lending"
    cargo fmt -- --check
    Set-Location "..\..\.."
}

Write-Host ""
Write-Host "Step 6: Documentation Validation" -ForegroundColor Cyan
Write-Host "---------------------------------" -ForegroundColor Cyan

Run-Test "Documentation builds" {
    Set-Location "stellar-lend\contracts\lending"
    cargo doc --no-deps --document-private-items
    Set-Location "..\..\.."
}

Run-Test "Example compilation" {
    Set-Location "stellar-lend\contracts\lending"
    cargo build --examples
    Set-Location "..\..\.."
}

Write-Host ""
Write-Host "====================================" -ForegroundColor Cyan
Write-Host "Validation Summary" -ForegroundColor Cyan
Write-Host "====================================" -ForegroundColor Cyan
Write-Host "Total Tests:  $global:TotalTests"
Write-Host "Passed:       $global:PassedTests" -ForegroundColor Green
Write-Host "Failed:       $global:FailedTests" -ForegroundColor Red
Write-Host ""

if ($global:FailedTests -eq 0) {
    Write-Host "✓ All authorization boundary validations passed!" -ForegroundColor Green
    exit 0
} else {
    Write-Host "✗ Some tests failed. Please review the output above." -ForegroundColor Red
    exit 1
}
