Stop-Process -Id 20360 -Force -ErrorAction SilentlyContinue
Stop-Process -Id 30444 -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2
Get-Process | Where-Object { $_.Name -match 'AT-Switch|setup|makensis' } | Format-Table Name, Id -AutoSize