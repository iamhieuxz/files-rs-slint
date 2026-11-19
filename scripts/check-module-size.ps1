# Warn when a source module grows too large.
#
# Never fails CI: this is a signal to split a module, not a functional error.
# GitHub Actions turns "::warning file=..." into a PR annotation.

$limit = 800
$roots = @('app/src', 'crates/files-core/src', 'crates/files-fs/src', 'app/ui')
$extensions = @('.rs', '.slint')

$rows = foreach ($root in $roots) {
    if (-not (Test-Path $root)) { continue }
    Get-ChildItem $root -Recurse -File |
        Where-Object { $extensions -contains $_.Extension } |
        ForEach-Object {
            $lines = (Get-Content -LiteralPath $_.FullName | Measure-Object -Line).Lines
            [pscustomobject]@{
                Path = $_.FullName.Replace((Get-Location).Path + '\', '').Replace('\', '/')
                Lines = $lines
            }
        }
}

$over = $rows | Where-Object { $_.Lines -gt $limit } | Sort-Object Lines -Descending
$rows | Sort-Object Lines -Descending | Select-Object -First 15 | Format-Table -AutoSize

if ($over) {
    foreach ($row in $over) {
        Write-Host "::warning file=$($row.Path)::$($row.Path) has $($row.Lines) lines (limit $limit); consider splitting it."
    }
    Write-Host "$($over.Count) file(s) exceed $limit lines."
} else {
    Write-Host "No file exceeds $limit lines."
}

# Always succeed: this is a tracking signal, not a merge blocker.
exit 0
