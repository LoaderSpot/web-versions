$OutputEncoding = [System.Text.Encoding]::UTF8
$versionsWebUrl = "https://raw.githubusercontent.com/LoaderSpot/web-versions/refs/heads/main/versions_web.json"
$versionsUrl = "https://raw.githubusercontent.com/LoaderSpot/LoaderSpot/refs/heads/main/versions.json"

$versionsWebResponse = Invoke-WebRequest -Uri $versionsWebUrl -ErrorAction Stop
$versionsWeb = $versionsWebResponse.Content | ConvertFrom-Json

$versionsResponse = Invoke-WebRequest -Uri $versionsUrl -ErrorAction Stop
$versions = $versionsResponse.Content | ConvertFrom-Json

$latestVersion = $versions.psobject.Properties.Name | ForEach-Object { [version]$_ } | Sort-Object -Descending | Select-Object -First 1

if (-not $latestVersion) {
    Write-Output "Не удалось определить последнюю версию в versions.json"
    exit
}

$versionsToSort = @()
$versionsWeb.psobject.Properties | ForEach-Object {
    $shortVersionString = $_.Name
    $currentVersion = [version]$shortVersionString
    if ($currentVersion -gt $latestVersion) {
        $versionsToSort += [pscustomobject]@{
            VersionObject = $currentVersion
            FullVersion   = $_.Value.clientVersion
        }
    }
}

if ($versionsToSort.Count -gt 0) {
    $sortedList = $versionsToSort | Sort-Object -Property VersionObject | Select-Object -ExpandProperty FullVersion
    

    $versionString = $sortedList -join ','

    Write-Output "Найдено $($sortedList.Count) новых версий для проверки"
    
    $tempDir = [System.IO.Path]::GetTempPath()
    
    $binaryUrl = "https://github.com/LoaderSpot/LoaderSpot/releases/latest/download/loaderspot-cli-win-x64.exe"
    $exePath = Join-Path -Path $tempDir -ChildPath "loaderspot-cli-win-x64.exe"
    
    Write-Output "Скачивание последней версии loaderspot-cli..."
    try {
        Invoke-WebRequest -Uri $binaryUrl -OutFile $exePath -ErrorAction Stop
        Write-Output "loaderspot-cli успешно скачан в $exePath"
    }
    catch {
        Write-Error "Не удалось скачать loaderspot-cli: $_"
        exit
    }

    Write-Output "Поиск валидных версий..."
    $output = & $exePath --version $versionString --connections 300 --platform win --arch x64
    
    if ($output) {
        $results = $output | ConvertFrom-Json
        
        $foundAnything = $false
        foreach ($result in $results) {
            $linkProperty = $result.psobject.Properties | Where-Object { $_.Name -notin @('version', 'unknown') }
            
            if ($linkProperty) {
                $foundAnything = $true
                $version = $result.version
                $url = $linkProperty.Value
                
                Write-Output "Найдена ссылка для версии ${version}: ${url}"
                Write-Output "Отправка в GitHub Actions..."
                
                $apiUrl = "https://api.github.com/repos/LoaderSpot/LoaderSpot/dispatches"
                
                $payload = @{
                    event_type     = "webhook-event"
                    client_payload = @{
                        v = $version
                        s = "[Spotify Web](open.spotify.com)"
                    }
                } | ConvertTo-Json -Depth 4

                $headers = @{
                    "Accept"        = "application/vnd.github.everest-preview+json"
                    "Authorization" = "Bearer $env:GH_TOKEN"
                    "Content-Type"  = "application/json"
                }

                try {
                    Invoke-RestMethod -Uri $apiUrl -Method Post -Headers $headers -Body $payload -ErrorAction Stop
                    Write-Output "Успешно отправлено для версии ${version}"
                }
                catch {
                    Write-Error "Ошибка при отправке для версии ${version}: $_"
                }
            }
        }

        if (-not $foundAnything) {
            Write-Output "Поиск ссылок не дал результатов"
        }
    }
    else {
        Write-Output "loaderspot-cli не вернул никакого вывода"
    }

}
else {
    Write-Output "Новых версий не найдено"
}