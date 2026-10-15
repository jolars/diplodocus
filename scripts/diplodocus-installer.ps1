$ErrorActionPreference = 'Stop'

# Fetch the complete release installer before executing it.
$source = Invoke-RestMethod -Uri 'https://github.com/jolars/diplodocus/releases/latest/download/diplodocus-installer.ps1'
$installer = [scriptblock]::Create($source)
& $installer @args
