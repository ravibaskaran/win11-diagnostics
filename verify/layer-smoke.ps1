[CmdletBinding()]
param(
    [ValidateSet('L0', 'L1', 'L2', 'L3', 'L4', 'All')]
    [string]$Layer = 'All'
)

$ErrorActionPreference = 'Stop'

function Invoke-Layer([string]$Name) {
    if ($Name -in @('L1', 'L3') -and -not $IsWindows) {
        throw "layer-gating: $Name requires a Windows runner"
    }

    switch ($Name) {
        'L0' { cargo test --workspace --lib }
        'L1' { cargo test --workspace --tests }
        'L2' { cargo test -p sidebar-app --test ui_snapshots }
        'L3' { cargo bench -p sidebar-app --bench layer_smoke -- --nocapture }
        'L4' { Write-Output 'L4 is manual; run the release smoke checklist on a Win11 host.' }
    }
}

if ($Layer -eq 'All') {
    foreach ($name in @('L0', 'L1', 'L2', 'L3', 'L4')) { Invoke-Layer $name }
} else {
    Invoke-Layer $Layer
}
