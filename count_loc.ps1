# Lines of Code Counter for PythonCompiler project
$root = "c:\Users\AG\CascadeProjects\PythonCompiler"
$extensions = @("*.rs", "*.toml", "*.spy", "*.spyc", "*.md")

$files = Get-ChildItem -Path $root -Recurse -File -Include $extensions |
    Where-Object { $_.FullName -notmatch '\\(target|node_modules|\.git|\.claude)\\' }

$results = @()

foreach ($f in $files) {
    $content = Get-Content $f.FullName -ErrorAction SilentlyContinue
    $total = ($content | Measure-Object -Line).Lines
    $blank = ($content | Where-Object { $_ -match '^\s*$' } | Measure-Object).Count
    $comment = ($content | Where-Object { $_ -match '^\s*(//|#)' } | Measure-Object).Count
    $code = $total - $blank - $comment

    $results += [PSCustomObject]@{
        File      = $f.FullName.Replace("$root\", "")
        Extension = $f.Extension
        Total     = $total
        Blank     = $blank
        Comment   = $comment
        Code      = $code
    }
}

Write-Host ""
Write-Host "=== Per-Language Summary ==="
Write-Host ""

$langMap = @{
    ".rs"   = "Rust"
    ".md"   = "Markdown"
    ".spy"  = "StrictPy"
    ".spyc" = "StrictPy Bytecode"
    ".toml" = "TOML (Config)"
}

$grouped = $results | Group-Object Extension | Sort-Object { ($_.Group | Measure-Object -Property Code -Sum).Sum } -Descending

$grandFiles = 0
$grandTotal = 0
$grandBlank = 0
$grandComment = 0
$grandCode = 0

foreach ($g in $grouped) {
    $t = ($g.Group | Measure-Object -Property Total -Sum).Sum
    $b = ($g.Group | Measure-Object -Property Blank -Sum).Sum
    $cm = ($g.Group | Measure-Object -Property Comment -Sum).Sum
    $cd = ($g.Group | Measure-Object -Property Code -Sum).Sum
    $name = if ($langMap.ContainsKey($g.Name)) { $langMap[$g.Name] } else { $g.Name }

    $line = "{0,-22} {1,5} files  |  {2,6} total  |  {3,5} blank  |  {4,5} comment  |  {5,6} code" -f $name, $g.Count, $t, $b, $cm, $cd
    Write-Host $line
    $grandFiles += $g.Count
    $grandTotal += $t
    $grandBlank += $b
    $grandComment += $cm
    $grandCode += $cd
}

$sep = "-" * 85
Write-Host $sep
$totLine = "{0,-22} {1,5} files  |  {2,6} total  |  {3,5} blank  |  {4,5} comment  |  {5,6} code" -f "TOTAL", $grandFiles, $grandTotal, $grandBlank, $grandComment, $grandCode
Write-Host $totLine

Write-Host ""
Write-Host "=== Top 10 Largest Files (by code lines) ==="
Write-Host ""

$top = $results | Sort-Object Code -Descending | Select-Object -First 10
foreach ($item in $top) {
    $fline = "{0,6} lines  {1}" -f $item.Code, $item.File
    Write-Host $fline
}
Write-Host ""
