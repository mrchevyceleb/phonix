$path = "src/app.rs"
$c = Get-Content -Path $path -Raw

# Core UI text normalization
$c = $c -replace 'Ready — hold key to dictate', 'Ready - hold key to dictate'
$c = $c -replace 'Recording…', 'Recording...'
$c = $c -replace 'Transcribing…', 'Transcribing...'
$c = $c -replace 'speech → text', 'speech -> text'
$c = $c -replace 'text → polished text', 'text -> polished text'
$c = $c -replace 'Advanced — override URL / model', 'Advanced - override URL / model'
$c = $c -replace 'Text accumulates here — copy when done\.', 'Text accumulates here - copy when done.'

# Remove decorative icon prefixes from labels/buttons
$c = $c -replace '"[^"]*  Stop"', '"Stop"'
$c = $c -replace '"[^"]*  Start"', '"Start"'
$c = $c -replace '"[^"]*  Copy All"', '"Copy All"'
$c = $c -replace '"[^"]*  Recording"', '"Recording"'
$c = $c -replace '"[^"]* Copied"', '"Copied"'
$c = $c -replace '"[^"]* Live"', '"Live"'

# Empty-state icon label
$c = $c -replace 'RichText::new\("[^"]*"\)\s*\r?\n\s*\.size\(40\.0\)', 'RichText::new("MIC")`r`n                        .size(40.0)'

Set-Content -Path $path -Value $c -Encoding UTF8
