# Installe ou met à jour une instance KRAKEN sous Windows, avec Docker Desktop :
#   irm https://raw.githubusercontent.com/KAYO74/Kraken/main/install.ps1 | iex
# ou depuis un fichier : powershell -ExecutionPolicy Bypass -File install.ps1 [-Domain chat.exemple.org]
# Même comportement que install.sh. Aide : -Help. Montrer le plan sans rien installer : -DryRun.
# Relancer la même commande met à jour l'instance ; .env et les données ne sont jamais touchés.
# Compatible Windows PowerShell 5.1 et PowerShell 7. Fichier en UTF-8 sans BOM (pour irm | iex).
param(
    [switch]$Yes,
    [string]$Domain = '',
    [switch]$Local,
    [string]$Dir = '',
    [string]$Port = '',
    [switch]$DryRun,
    [switch]$Help
)

function Install-Kraken {
    param([hashtable]$Options)
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'

    $RawBase = 'https://raw.githubusercontent.com/KAYO74/Kraken'
    $DefaultImage = 'ghcr.io/kayo74/kraken:latest'
    $InstallUrl = 'https://raw.githubusercontent.com/KAYO74/Kraken/main/install.ps1'

    # --- Messages ------------------------------------------------------------------------------

    # Windows PowerShell 5.1 lit un fichier sans BOM en ANSI : on remet les accents des messages.
    function Repair-Text([string]$Text) {
        if (('é').Length -eq 1) { return $Text }
        return [System.Text.Encoding]::UTF8.GetString([System.Text.Encoding]::Default.GetBytes($Text))
    }
    # Les valeurs ({0}, {1}…) sont ajoutées après la réparation : un chemin accentué reste intact.
    function Format-Text([string]$Format, [object[]]$Values) {
        $text = Repair-Text $Format
        if ($null -ne $Values -and $Values.Count -gt 0) { $text = $text -f $Values }
        return $text
    }
    function Say([string]$Format) { Write-Host (Format-Text $Format $args) }
    function Step([string]$Format) {
        Write-Host ''
        Write-Host ('==> ' + (Format-Text $Format $args)) -ForegroundColor Cyan
    }
    function Warn([string]$Format) {
        Write-Host ''
        Write-Host ('Attention : ' + (Format-Text $Format $args)) -ForegroundColor Yellow
    }
    function Fail([string]$Format) { throw (Format-Text $Format $args) }

    # --- Programmes externes -------------------------------------------------------------------

    # Lance un programme et renvoie son code de sortie ; sa sortie s'affiche (ou rien avec -Quiet).
    function Invoke-Tool([string]$File, [string[]]$ArgumentList = @(), [switch]$Quiet) {
        if (-not (Get-Command $File -ErrorAction SilentlyContinue)) { return 127 }
        $saved = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            if ($Quiet) { & $File @ArgumentList *> $null } else { & $File @ArgumentList | Out-Host }
            return $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $saved
        }
    }
    # Sortie d'un programme en texte, ou $null s'il échoue.
    function Get-ToolOutput([string]$File, [string[]]$ArgumentList = @()) {
        if (-not (Get-Command $File -ErrorAction SilentlyContinue)) { return $null }
        $saved = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            $lines = & $File @ArgumentList 2>$null
            if ($LASTEXITCODE -ne 0) { return $null }
            return ((@($lines) | ForEach-Object { [string]$_ }) -join "`n").Trim()
        } finally {
            $ErrorActionPreference = $saved
        }
    }
    function Test-Command([string]$Name) { return [bool](Get-Command $Name -ErrorAction SilentlyContinue) }

    # --- Réseau --------------------------------------------------------------------------------

    function Test-IPv4([string]$Ip) { return ($Ip -match '^(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}\z') }

    # Adresse joignable depuis Internet (pas privée, pas réservée).
    function Test-PublicIPv4([string]$Ip) {
        if (-not (Test-IPv4 $Ip)) { return $false }
        $b = @($Ip.Split('.') | ForEach-Object { [int]$_ })
        if ($b[0] -eq 0 -or $b[0] -eq 10 -or $b[0] -eq 127 -or $b[0] -ge 224) { return $false }
        if ($b[0] -eq 169 -and $b[1] -eq 254) { return $false }
        if ($b[0] -eq 172 -and $b[1] -ge 16 -and $b[1] -le 31) { return $false }
        if ($b[0] -eq 192 -and $b[1] -eq 168) { return $false }
        if ($b[0] -eq 100 -and $b[1] -ge 64 -and $b[1] -le 127) { return $false }
        if ($b[0] -eq 192 -and $b[1] -eq 0 -and ($b[2] -eq 0 -or $b[2] -eq 2)) { return $false }
        if ($b[0] -eq 198 -and ($b[1] -eq 18 -or $b[1] -eq 19)) { return $false }
        if ($b[0] -eq 198 -and $b[1] -eq 51 -and $b[2] -eq 100) { return $false }
        if ($b[0] -eq 203 -and $b[1] -eq 0 -and $b[2] -eq 113) { return $false }
        return $true
    }

    # Toutes les adresses IPv4 de cette machine.
    function Get-LocalIPv4 {
        $list = New-Object System.Collections.Generic.List[string]
        try {
            foreach ($nic in [System.Net.NetworkInformation.NetworkInterface]::GetAllNetworkInterfaces()) {
                if ($nic.OperationalStatus -ne [System.Net.NetworkInformation.OperationalStatus]::Up) { continue }
                foreach ($unicast in $nic.GetIPProperties().UnicastAddresses) {
                    if ($unicast.Address.AddressFamily -eq [System.Net.Sockets.AddressFamily]::InterNetwork) {
                        $list.Add($unicast.Address.ToString())
                    }
                }
            }
        } catch { }
        return $list.ToArray()
    }

    # Adresse de cette machine sur le réseau local (celle de la route vers Internet ; aucun envoi).
    function Get-LanIPv4 {
        try {
            $socket = New-Object System.Net.Sockets.Socket([System.Net.Sockets.AddressFamily]::InterNetwork,
                [System.Net.Sockets.SocketType]::Dgram, [System.Net.Sockets.ProtocolType]::Udp)
            try {
                $socket.Connect('1.1.1.1', 53)
                $ip = $socket.LocalEndPoint.Address.ToString()
            } finally {
                $socket.Close()
            }
            if ((Test-IPv4 $ip) -and $ip -ne '0.0.0.0') { return $ip }
        } catch { }
        foreach ($ip in (Get-LocalIPv4)) {
            if (-not $ip.StartsWith('127.') -and -not $ip.StartsWith('169.254.')) { return $ip }
        }
        return ''
    }

    function Get-PublicIPv4 {
        foreach ($ip in (Get-LocalIPv4)) { if (Test-PublicIPv4 $ip) { return $ip } }
        return ''
    }

    # Adresse publique vue d'Internet (derrière une box, ce n'est pas celle de la machine).
    function Get-ExternalIPv4 {
        foreach ($url in @('https://api.ipify.org', 'https://ipv4.icanhazip.com')) {
            try {
                $text = ([string](Invoke-RestMethod -Uri $url -TimeoutSec 5 -UseBasicParsing)).Trim()
                if (Test-IPv4 $text) { return $text }
            } catch { }
        }
        return ''
    }

    function Resolve-IPv4([string]$Name) {
        try {
            return @([System.Net.Dns]::GetHostAddresses($Name) |
                    Where-Object { $_.AddressFamily -eq [System.Net.Sockets.AddressFamily]::InterNetwork } |
                    ForEach-Object { $_.ToString() } | Sort-Object -Unique)
        } catch {
            return @()
        }
    }

    # Port déjà ouvert par un autre programme ?
    function Test-PortBusy([int]$Number, [string]$Proto = 'tcp') {
        $props = [System.Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties()
        if ($Proto -eq 'udp') { $listeners = $props.GetActiveUdpListeners() } else { $listeners = $props.GetActiveTcpListeners() }
        return [bool](@($listeners) | Where-Object { $_.Port -eq $Number })
    }

    function Get-FreePort([int]$From) {
        for ($p = $From; $p -lt $From + 100; $p++) { if (-not (Test-PortBusy $p)) { return $p } }
        Fail 'aucun port libre trouvé à partir de {0}.' $From
    }

    function Test-Port([string]$Value) {
        if ($Value -notmatch '^\d{1,5}\z') { return $false }
        return ([int]$Value -ge 1 -and [int]$Value -le 65535)
    }

    function Test-Url([string]$Url) {
        try {
            $response = Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec 5
            return ($response.StatusCode -eq 200)
        } catch {
            return $false
        }
    }

    # --- Domaine -------------------------------------------------------------------------------

    # En minuscules, sans https:// ni / final (on accepte une adresse collée depuis le navigateur).
    function ConvertTo-DomainName([string]$Text) {
        $name = ($Text -replace '\s', '').ToLowerInvariant()
        $name = $name -replace '^https?://', ''
        $name = ($name -split '/')[0]
        return $name.TrimEnd('.')
    }

    function Test-DomainName([string]$Name) {
        if ($Name.Length -gt 253) { return $false }
        return ($Name -cmatch '^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+([a-z]{2,63}|xn--[a-z0-9-]{1,59})\z')
    }

    function Get-SslipName([string]$Ip) { return ('{0}.sslip.io' -f ($Ip -replace '\.', '-')) }

    # Prévient si le domaine ne pointe pas (encore) sur cette machine.
    function Test-DomainDns {
        $resolved = @(Resolve-IPv4 $K.Domain)
        $external = Get-ExternalIPv4
        if ($resolved.Count -eq 0) {
            $hint = ''
            if ($external) { $hint = ' ({0})' -f $external }
            Warn '{0} ne pointe vers aucune adresse pour l''instant. Chez ton fournisseur de domaine, crée un enregistrement A vers l''adresse IP publique de cette machine{1}. Le HTTPS se mettra en route tout seul dès que ce sera fait.' $K.Domain $hint
            return
        }
        $mine = @(Get-LocalIPv4) + @($external)
        foreach ($ip in $resolved) { if ($mine -contains $ip) { return } }
        if ($external) {
            Warn '{0} pointe vers {1}, mais l''adresse publique de cette machine est {2}. Corrige l''enregistrement A chez ton fournisseur de domaine : le HTTPS ne marchera qu''ensuite.' $K.Domain ($resolved -join ' ') $external
        } else {
            Warn '{0} pointe vers {1} ; je n''ai pas pu vérifier que c''est bien cette machine.' $K.Domain ($resolved -join ' ')
        }
    }

    # --- Fichiers ------------------------------------------------------------------------------

    function Get-EnvValue([string]$Key) {
        $value = ''
        foreach ($line in [System.IO.File]::ReadAllLines($K.EnvFile)) {
            if ($line.StartsWith($Key + '=')) { $value = $line.Substring($Key.Length + 1).Trim() }
        }
        if ($value.Length -ge 2 -and (($value.StartsWith('"') -and $value.EndsWith('"')) -or
                ($value.StartsWith("'") -and $value.EndsWith("'")))) {
            $value = $value.Substring(1, $value.Length - 2)
        }
        return $value
    }

    function Test-SamePath([string]$A, [string]$B) {
        return ([System.IO.Path]::GetFullPath($A).TrimEnd('\') -eq [System.IO.Path]::GetFullPath($B).TrimEnd('\'))
    }

    # Une commande prête à copier-coller dans PowerShell.
    function Get-ComposeHint {
        return ("cd '{0}'; docker compose" -f ($K.Dir -replace "'", "''"))
    }

    # Mot de passe aléatoire de la base : 40 lettres et chiffres, sans biais.
    function New-Password {
        $chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'
        $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
        $byte = New-Object byte[] 1
        $builder = New-Object System.Text.StringBuilder
        try {
            while ($builder.Length -lt 40) {
                $rng.GetBytes($byte)
                if ($byte[0] -lt 248) { [void]$builder.Append($chars[$byte[0] % 62]) }
            }
        } finally {
            $rng.Dispose()
        }
        return $builder.ToString()
    }

    # docker-compose.yml, deploy\Caddyfile et .env.example : du clone, sinon de GitHub.
    function Install-Files {
        [void](New-Item -ItemType Directory -Force -Path (Join-Path $K.Dir 'deploy'))
        [void](New-Item -ItemType Directory -Force -Path (Join-Path $K.Dir 'backups'))
        if ($K.Src -and (Test-SamePath $K.Src $K.Dir)) { return }
        foreach ($name in @('docker-compose.yml', 'deploy/Caddyfile', '.env.example')) {
            $target = Join-Path $K.Dir ($name -replace '/', '\')
            $new = $target + '.new'
            if ($K.Src) {
                Copy-Item -LiteralPath (Join-Path $K.Src ($name -replace '/', '\')) -Destination $new -Force
            } else {
                $url = '{0}/{1}/{2}' -f $RawBase, $K.Ref, $name
                try {
                    Invoke-WebRequest -Uri $url -OutFile $new -UseBasicParsing
                } catch {
                    Remove-Item -LiteralPath $new -Force -ErrorAction SilentlyContinue
                    Fail 'impossible de télécharger {0} ({1}). Vérifie ta connexion à Internet, et que le dépôt KAYO74/Kraken est public.' $name $url
                }
            }
            if ((Get-Item -LiteralPath $new).Length -eq 0) { Fail '{0} téléchargé est vide.' $name }
            if (Test-Path -LiteralPath $target) {
                if ((Get-FileHash -LiteralPath $new).Hash -ne (Get-FileHash -LiteralPath $target).Hash) {
                    Copy-Item -LiteralPath $target -Destination ($target + '.old') -Force
                    Say '{0} mis à jour (l''ancienne version reste dans {0}.old).' $name
                }
            }
            Move-Item -LiteralPath $new -Destination $target -Force
        }
    }

    # Écrit .env à la première installation seulement (jamais d'écrasement).
    function Write-EnvFile {
        $values = [ordered]@{
            KRAKEN_HTTP_PORT  = [string]$K.HttpPort
            KRAKEN_HTTPS_PORT = [string]$K.HttpsPort
            POSTGRES_PASSWORD = New-Password
        }
        if ($K.Mode -eq 'local') { $values['KRAKEN_PUBLIC_URL'] = $K.PublicUrl } else { $values['KRAKEN_DOMAIN'] = $K.Domain }
        if ($K.ImageExplicit) { $values['KRAKEN_IMAGE'] = $K.Image }
        $template = Join-Path $K.Dir '.env.example'
        $lines = @()
        if (Test-Path -LiteralPath $template) { $lines = [System.IO.File]::ReadAllLines($template) }
        $out = New-Object System.Collections.Generic.List[string]
        $out.Add((Format-Text '# Réglages de cette instance KRAKEN, écrits par install.ps1 le {0}.' (Get-Date -Format 'yyyy-MM-dd')))
        $out.Add((Repair-Text '# À garder secret. Relancer l''installation ne touche jamais ce fichier.'))
        $out.Add('')
        $done = @{}
        foreach ($line in $lines) {
            $replaced = $false
            foreach ($key in $values.Keys) {
                if ($line.StartsWith($key + '=')) {
                    $out.Add($key + '=' + $values[$key])
                    $done[$key] = $true
                    $replaced = $true
                    break
                }
            }
            if (-not $replaced) { $out.Add($line) }
        }
        foreach ($key in $values.Keys) { if (-not $done.ContainsKey($key)) { $out.Add($key + '=' + $values[$key]) } }
        # CreateNew : échoue plutôt que d'écraser un .env existant.
        $stream = New-Object System.IO.FileStream($K.EnvFile, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write)
        try {
            $writer = New-Object System.IO.StreamWriter($stream, (New-Object System.Text.UTF8Encoding($false)))
            $writer.NewLine = "`n"
            foreach ($line in $out) { $writer.WriteLine($line) }
            $writer.Flush()
        } finally {
            $stream.Dispose()
        }
        # Lisible par toi seul (équivalent de chmod 600).
        try {
            $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
            [void](Invoke-Tool 'icacls.exe' @($K.EnvFile, '/inheritance:r', '/grant:r', ('*{0}:(F)' -f $sid)) -Quiet)
        } catch { }
    }

    # --- Docker --------------------------------------------------------------------------------

    function Add-DockerToPath {
        $bin = Join-Path $env:ProgramFiles 'Docker\Docker\resources\bin'
        if ((Test-Path -LiteralPath $bin) -and -not (($env:Path -split ';') -contains $bin)) { $env:Path = $env:Path + ';' + $bin }
    }

    function Test-DockerReady { return ((Invoke-Tool 'docker' @('info') -Quiet) -eq 0) }

    function Wait-Docker([int]$Seconds) {
        $deadline = (Get-Date).AddSeconds($Seconds)
        while ((Get-Date) -lt $deadline) {
            if (Test-DockerReady) { return $true }
            Start-Sleep -Seconds 3
        }
        return $false
    }

    function Initialize-Docker {
        Add-DockerToPath
        if (-not (Test-Command 'docker')) {
            if (-not (Test-Command 'winget')) {
                Fail 'il faut Docker Desktop. Télécharge-le sur https://www.docker.com/products/docker-desktop/ , installe-le et ouvre-le une fois, puis relance cette commande.'
            }
            Step 'Installation de Docker Desktop (winget)'
            Say 'Windows va sans doute te demander une autorisation : accepte-la.'
            $code = Invoke-Tool 'winget' @('install', '--id', 'Docker.DockerDesktop', '-e', '--accept-package-agreements', '--accept-source-agreements')
            Add-DockerToPath
            if (-not (Test-Command 'docker')) {
                Fail 'l''installation de Docker Desktop n''a pas abouti (code {0}). Installe-le depuis https://www.docker.com/products/docker-desktop/ , redémarre l''ordinateur si on te le demande, puis relance cette commande.' $code
            }
        }
        if (-not (Test-DockerReady)) {
            $desktop = Join-Path $env:ProgramFiles 'Docker\Docker\Docker Desktop.exe'
            if (Test-Path -LiteralPath $desktop) {
                Say 'Je démarre Docker Desktop. Si une fenêtre s''ouvre, accepte ses conditions d''utilisation.'
                Start-Process -FilePath $desktop
            }
            Say 'J''attends que Docker soit prêt (jusqu''à 5 minutes)...'
            if (-not (Wait-Docker 300)) {
                Fail 'Docker Desktop ne répond pas. La première fois, il lui faut souvent WSL2 et un redémarrage : 1) ouvre PowerShell en administrateur et tape : wsl --install  2) redémarre l''ordinateur  3) ouvre Docker Desktop et accepte ses conditions  4) relance cette commande.'
            }
        }
        if ((Get-ToolOutput 'docker' @('info', '--format', '{{.OSType}}')) -eq 'windows') {
            $cli = Join-Path $env:ProgramFiles 'Docker\Docker\DockerCli.exe'
            if (Test-Path -LiteralPath $cli) {
                Say 'Docker utilise les conteneurs Windows : je le passe sur les conteneurs Linux.'
                [void](Invoke-Tool $cli @('-SwitchLinuxEngine') -Quiet)
                Start-Sleep -Seconds 5
                [void](Wait-Docker 120)
            }
            if ((Get-ToolOutput 'docker' @('info', '--format', '{{.OSType}}')) -ne 'linux') {
                Fail 'Docker doit utiliser les conteneurs Linux : clic droit sur l''icône de Docker (près de l''horloge), « Switch to Linux containers », puis relance cette commande.'
            }
        }
        if ((Invoke-Tool 'docker' @('compose', 'version') -Quiet) -ne 0) {
            Fail 'docker compose est introuvable : mets Docker Desktop à jour, puis relance cette commande.'
        }
    }

    function Invoke-Compose([string[]]$ArgumentList, [switch]$Quiet) {
        Push-Location -LiteralPath $K.Dir
        try {
            return (Invoke-Tool 'docker' (@('compose') + $ArgumentList) -Quiet:$Quiet)
        } finally {
            Pop-Location
        }
    }

    function Initialize-Image {
        Step 'Téléchargement de KRAKEN ({0})' $K.Image
        if ((Invoke-Tool 'docker' @('pull', $K.Image)) -eq 0) { return }
        $present = ((Invoke-Tool 'docker' @('image', 'inspect', $K.Image) -Quiet) -eq 0)
        if ($K.ImageExplicit -and $present) {
            Say 'Téléchargement impossible : j''utilise l''image {0} déjà présente sur cette machine.' $K.Image
            return
        }
        if ($K.Src) {
            Step 'L''image n''est pas disponible en ligne : je la construis depuis le code source'
            Say 'Ça prend de 10 à 30 minutes la première fois.'
            if ((Invoke-Tool 'docker' @('build', '-t', $K.Image, $K.Src)) -ne 0) {
                Fail 'la construction de l''image a échoué (voir les messages au-dessus).'
            }
            return
        }
        if ($present) {
            Warn 'impossible de télécharger la nouvelle version de KRAKEN : je garde celle déjà installée.'
            return
        }
        Fail 'impossible de télécharger l''image de KRAKEN ({0}). Soit elle n''est pas encore publiée, soit elle est privée. Si tu as accès au dépôt : docker login ghcr.io, puis relance cette commande. Sinon, installe depuis le code source : git clone https://github.com/KAYO74/Kraken, puis lance install.ps1 dans ce dossier.' $K.Image
    }

    # Une ancienne base sans son .env aurait un autre mot de passe : KRAKEN ne pourrait pas s'y connecter.
    function Test-OldVolume {
        $project = ((Split-Path -Leaf $K.Dir).ToLowerInvariant() -replace '[^a-z0-9_-]', '') -replace '^[-_]+', ''
        $volume = $project + '_db'
        if ((Invoke-Tool 'docker' @('volume', 'inspect', $volume) -Quiet) -eq 0) {
            Fail 'une base KRAKEN existe déjà sur cette machine (volume Docker {0}), mais son fichier .env a disparu, et avec lui son mot de passe. Remets l''ancien .env dans {1}, ou efface cette base (toutes ses données !) avec : docker volume rm {0}' $volume $K.Dir
        }
    }

    function Start-Kraken {
        Step 'Démarrage de KRAKEN'
        if ((Invoke-Compose @('pull', '--quiet', 'db', 'caddy', 'backup')) -ne 0) {
            Warn 'certaines images n''ont pas pu être téléchargées ; j''utilise celles déjà présentes.'
        }
        if ((Invoke-Compose @('up', '-d', '--remove-orphans')) -ne 0) {
            Fail 'docker compose n''a pas pu démarrer KRAKEN. Pour voir pourquoi : {0} logs --tail 100' (Get-ComposeHint)
        }
        # Mise à jour : Caddy ne relit son Caddyfile qu'en redémarrant.
        if ($K.Update) { [void](Invoke-Compose @('restart', 'caddy') -Quiet) }
    }

    function Wait-Health {
        Step 'Vérification (jusqu''à 4 minutes)'
        $deadline = (Get-Date).AddSeconds(240)
        while ((Invoke-Compose @('exec', '-T', 'caddy', 'wget', '-q', '-T', '3', '-O', '/dev/null', 'http://kraken:8080/api/v1/health') -Quiet) -ne 0) {
            if ((Get-Date) -gt $deadline) {
                Fail 'KRAKEN ne répond pas après 4 minutes. Pour voir ce qui se passe : {0} logs --tail 100 kraken db' (Get-ComposeHint)
            }
            Start-Sleep -Seconds 3
        }
        if ($K.Mode -eq 'local') {
            $url = 'http://127.0.0.1:{0}/api/v1/health' -f $K.HttpPort
            $deadline = (Get-Date).AddSeconds(60)
            while (-not (Test-Url $url)) {
                if ((Get-Date) -gt $deadline) {
                    Fail 'KRAKEN tourne mais ne répond pas sur le port {0}. Pour voir pourquoi : {1} logs --tail 100 caddy' $K.HttpPort (Get-ComposeHint)
                }
                Start-Sleep -Seconds 3
            }
        } else {
            Say 'Obtention du certificat HTTPS pour {0}...' $K.Domain
            $url = 'https://{0}:{1}/api/v1/health' -f $K.Domain, $K.HttpsPort
            $deadline = (Get-Date).AddSeconds(180)
            while (-not (Test-Url $url)) {
                if ((Get-Date) -gt $deadline) {
                    $K.HttpsPending = $true
                    break
                }
                Start-Sleep -Seconds 5
            }
        }
    }

    # --- Étapes --------------------------------------------------------------------------------

    function Show-Help {
        Say 'Installe KRAKEN sur ce PC Windows, ou le met à jour s''il est déjà installé.'
        Say ''
        Say '  irm {0} | iex' $InstallUrl
        Say '  powershell -ExecutionPolicy Bypass -File install.ps1 -Domain chat.exemple.org'
        Say ''
        Say 'Options, et la variable d''environnement qui fait pareil :'
        Say '  -Yes            aucune question, choix par défaut            KRAKEN_YES=1'
        Say '  -Domain NOM     ton nom de domaine, pour le HTTPS            KRAKEN_DOMAIN=NOM'
        Say '  -Local          instance pour ton réseau local, en HTTP      KRAKEN_LOCAL=1'
        Say '  -Dir DOSSIER    dossier d''installation (défaut : %USERPROFILE%\kraken)   KRAKEN_DIR=DOSSIER'
        Say '  -Port PORT      port HTTP en mode local (défaut 80, ou 8080 s''il est pris)   KRAKEN_HTTP_PORT=PORT'
        Say '  -DryRun         montre ce qui serait fait, sans rien installer   KRAKEN_DRY_RUN=1'
        Say '  -Help           cette aide'
        Say ''
        Say 'Avec irm | iex, passe les options par les variables, par exemple :'
        Say '  $env:KRAKEN_DOMAIN = ''chat.exemple.org''; irm {0} | iex' $InstallUrl
        Say 'Autres variables : KRAKEN_REF (branche des fichiers, défaut main), KRAKEN_IMAGE, KRAKEN_HTTPS_PORT.'
    }

    # Première installation : la seule question (le domaine), puis le mode et les ports.
    function Set-Configuration {
        if ($K.Local -and $K.Domain) { Fail 'choisis -Domain ou -Local, pas les deux.' }
        $public = ''
        if (-not $K.Local) { $public = Get-PublicIPv4 }
        $lan = Get-LanIPv4
        if (-not $K.Local -and -not $K.Domain -and $K.Interactive) {
            Say ''
            Say 'Si tu as un nom de domaine (ex. chat.exemple.org), KRAKEN l''utilisera avec le HTTPS.'
            if ($public) {
                Say 'Sans domaine, KRAKEN sera à l''adresse https://{0} (HTTPS compris).' (Get-SslipName $public)
            } else {
                $where = $lan
                if (-not $where) { $where = Repair-Text '<adresse de ce PC>' }
                Say 'Sans domaine, KRAKEN marchera pour les personnes connectées au même réseau que ce PC (Wi-Fi, box), à l''adresse http://{0}' $where
            }
            while ($true) {
                $answer = Read-Host -Prompt (Repair-Text 'Ton nom de domaine, ou Entrée pour t''en passer')
                $K.Domain = ConvertTo-DomainName ([string]$answer)
                if (-not $K.Domain -or (Test-DomainName $K.Domain)) { break }
                Say '« {0} » n''est pas un nom de domaine valable (exemple : chat.exemple.org). Réessaie.' $answer
            }
        }
        if ($K.Domain) {
            $K.Domain = ConvertTo-DomainName $K.Domain
            if (-not (Test-DomainName $K.Domain)) { Fail 'nom de domaine invalide : « {0} » (exemple : chat.exemple.org).' $K.Domain }
            $K.Mode = 'domain'
        } elseif ($public) {
            $K.Mode = 'sslip'
            $K.Domain = Get-SslipName $public
        } else {
            $K.Mode = 'local'
        }

        if ($K.HttpPort -and -not (Test-Port $K.HttpPort)) { Fail 'port HTTP invalide : {0}' $K.HttpPort }
        if ($K.HttpsPort -and -not (Test-Port $K.HttpsPort)) { Fail 'port HTTPS invalide : {0}' $K.HttpsPort }
        if ($K.Mode -eq 'local') {
            if (-not $K.HttpPort) {
                if (Test-PortBusy 80) {
                    $K.HttpPort = [string](Get-FreePort 8080)
                    Say 'Le port 80 est déjà pris sur ce PC : KRAKEN utilisera le port {0}.' $K.HttpPort
                } else {
                    $K.HttpPort = '80'
                }
            } elseif (Test-PortBusy ([int]$K.HttpPort)) {
                Fail 'le port {0} est déjà pris par un autre programme. Choisis-en un autre avec -Port.' $K.HttpPort
            }
            # Le HTTPS ne sert pas en mode local : on laisse le port 443 libre.
            if (-not $K.HttpsPort) { $K.HttpsPort = [string](Get-FreePort 8443) }
            $hostName = $lan
            if (-not $hostName) { $hostName = 'localhost' }
            $K.PublicUrl = 'http://' + $hostName
            if ($K.HttpPort -ne '80') { $K.PublicUrl = $K.PublicUrl + ':' + $K.HttpPort }
            if (-not $lan) { Warn 'je n''ai pas trouvé l''adresse de ce PC sur ton réseau : KRAKEN ne sera ouvert qu''ici, sur {0}.' $K.PublicUrl }
        } else {
            if (-not $K.HttpPort) { $K.HttpPort = '80' }
            if (-not $K.HttpsPort) { $K.HttpsPort = '443' }
            foreach ($p in @($K.HttpPort, $K.HttpsPort)) {
                if (Test-PortBusy ([int]$p)) {
                    Fail 'le port {0} est déjà pris par un autre programme (un autre serveur web ?). Le HTTPS a besoin des ports 80 et 443 : arrête ce programme, ou installe KRAKEN pour ton réseau local seulement avec -Local.' $p
                }
            }
            $K.PublicUrl = 'https://' + $K.Domain
            if ($K.Mode -eq 'domain') { Test-DomainDns }
        }
    }

    # Mise à jour : les réglages viennent de .env, jamais des options.
    function Read-Settings {
        $stored = Get-EnvValue 'KRAKEN_DOMAIN'
        if (($K.Local -and $stored) -or ($K.Domain -and (ConvertTo-DomainName $K.Domain) -ne $stored)) {
            Warn 'KRAKEN est déjà installé dans {0} : je garde les réglages de son fichier .env (-Domain et -Local sont ignorés). Pour les changer, modifie {1} puis relance.' $K.Dir $K.EnvFile
        }
        $K.Domain = $stored
        $K.PublicUrl = Get-EnvValue 'KRAKEN_PUBLIC_URL'
        $K.HttpPort = Get-EnvValue 'KRAKEN_HTTP_PORT'
        if (-not $K.HttpPort) { $K.HttpPort = '80' }
        $K.HttpsPort = Get-EnvValue 'KRAKEN_HTTPS_PORT'
        if (-not $K.HttpsPort) { $K.HttpsPort = '443' }
        if ($K.Domain) {
            $K.Mode = 'domain'
            if (-not $K.PublicUrl) { $K.PublicUrl = 'https://' + $K.Domain }
        } else {
            $K.Mode = 'local'
            if (-not $K.PublicUrl) { Fail '{0} ne contient ni KRAKEN_DOMAIN ni KRAKEN_PUBLIC_URL : remplis l''un des deux, puis relance.' $K.EnvFile }
            $oldHost = (($K.PublicUrl -replace '^[a-z]+://', '') -split '[:/]')[0]
            $lan = Get-LanIPv4
            if ($lan -and (Test-IPv4 $oldHost) -and $oldHost -ne $lan) {
                Warn 'l''adresse de ce PC sur ton réseau a changé ({0}, maintenant {1}). Mets la nouvelle dans KRAKEN_PUBLIC_URL, dans {2}, puis relance.' $oldHost $lan $K.EnvFile
            }
        }
        $storedImage = Get-EnvValue 'KRAKEN_IMAGE'
        if (-not $K.ImageExplicit -and $storedImage) {
            $K.Image = $storedImage
            $K.ImageExplicit = $true
        }
    }

    function Show-Plan {
        Say ''
        Say 'Essai à blanc (-DryRun) : voici ce que je ferais, sans rien installer ni modifier.'
        if (Test-Command 'docker') { $docker = Repair-Text 'déjà là, démarré si besoin' } elseif (Test-Command 'winget') { $docker = 'winget install --id Docker.DockerDesktop' } else { $docker = Repair-Text 'à installer à la main (https://www.docker.com/products/docker-desktop/)' }
        Say '  Docker Desktop : {0}' $docker
        Say '  Dossier : {0}' $K.Dir
        if ($K.Update) {
            Say '  Déjà installé : mise à jour, .env gardé tel quel'
        } else {
            Say '  Mode : {0} ; adresse : {1} ; ports {2} (HTTP) et {3} (HTTPS)' $K.Mode $K.PublicUrl $K.HttpPort $K.HttpsPort
        }
        if ($K.Src) {
            Say '  Fichiers : ceux du clone {0}' $K.Src
        } else {
            Say '  Fichiers : docker-compose.yml, deploy/Caddyfile et .env.example depuis {0}/{1}/' $RawBase $K.Ref
        }
        Say '  Image : docker pull {0}' $K.Image
        if (-not $K.Update) { Say '  .env : écrit avec un mot de passe de base aléatoire, lisible par toi seul' }
        Say '  Démarrage : docker compose up -d, puis attente de /api/v1/health'
    }

    function Show-Summary {
        if ($K.Src) {
            $update = "cd '{0}'; git pull; powershell -ExecutionPolicy Bypass -File install.ps1" -f ($K.Src -replace "'", "''")
            if (-not (Test-SamePath $K.Src $K.Dir)) { $update = $update + (" -Dir '{0}'" -f ($K.Dir -replace "'", "''")) }
        } else {
            $update = 'irm {0} | iex' -f ($InstallUrl -replace '/main/', ('/' + $K.Ref + '/'))
            if (-not (Test-SamePath $K.Dir $K.DefaultDir)) { $update = ("`$env:KRAKEN_DIR = '{0}'; " -f ($K.Dir -replace "'", "''")) + $update }
        }
        $line = '=================================================================='
        Say ''
        Say $line
        if ($K.HttpsPending) {
            Say 'KRAKEN tourne, mais son certificat HTTPS n''est pas encore prêt.'
            Say 'Vérifie que {0} pointe bien sur ce PC et que les ports 80 et 443 sont ouverts' $K.Domain
            Say '(box, pare-feu, hébergeur). Caddy réessaie tout seul ; pour suivre :'
            Say '  {0} logs -f caddy' (Get-ComposeHint)
            Say 'Ensuite, KRAKEN sera ici : {0}' $K.PublicUrl
        } else {
            Say 'KRAKEN est prêt : {0}' $K.PublicUrl
            if ($K.Mode -eq 'local') {
                Say 'Les personnes connectées au même réseau (Wi-Fi, box) l''ouvrent avec cette adresse.'
                Say 'Si elles n''y arrivent pas, autorise Docker dans le pare-feu Windows (réseaux privés).'
            }
            Say 'Ouvre-la dans ton navigateur et crée ton compte.'
        }
        Say ''
        Say 'Mettre à jour : relance la même commande (réglages et données sont gardés) :'
        Say '  {0}' $update
        Say 'Fichiers : {0} (réglages dans .env, à garder secret)' $K.Dir
        Say 'Sauvegardes : {0} (une par jour, gardées 14 jours)' (Join-Path $K.Dir 'backups')
        Say 'Journaux : {0} logs -f' (Get-ComposeHint)
        Say 'Arrêter : {0} down    Relancer : {0} up -d' (Get-ComposeHint)
        Say $line
    }

    # --- Programme principal -------------------------------------------------------------------

    function Test-Flag([object]$Value) { return ([string]$Value -match '^(1|true|yes|oui)\z') }
    function Get-Setting([string]$Value, [string]$EnvName) {
        if ($Value) { return $Value }
        return [string][Environment]::GetEnvironmentVariable($EnvName, 'Process')
    }

    if ($Options.Help) { Show-Help; return }

    $K = @{
        Yes           = [bool]$Options.Yes -or (Test-Flag $env:KRAKEN_YES)
        DryRun        = [bool]$Options.DryRun -or (Test-Flag $env:KRAKEN_DRY_RUN)
        Local         = [bool]$Options.Local -or (Test-Flag $env:KRAKEN_LOCAL)
        Domain        = Get-Setting $Options.Domain 'KRAKEN_DOMAIN'
        Dir           = Get-Setting $Options.Dir 'KRAKEN_DIR'
        HttpPort      = Get-Setting $Options.Port 'KRAKEN_HTTP_PORT'
        HttpsPort     = Get-Setting '' 'KRAKEN_HTTPS_PORT'
        Ref           = Get-Setting '' 'KRAKEN_REF'
        Image         = Get-Setting '' 'KRAKEN_IMAGE'
        ImageExplicit = $false
        Src           = ''
        Mode          = ''
        PublicUrl     = ''
        Update        = $false
        HttpsPending  = $false
    }
    if (-not $K.Ref) { $K.Ref = 'main' }
    if ($K.Ref -notmatch '^[A-Za-z0-9._/-]+\z') { Fail 'KRAKEN_REF invalide : {0}' $K.Ref }
    if ($K.Image) {
        if ($K.Image -notmatch '^[A-Za-z0-9._/:@-]+\z') { Fail 'KRAKEN_IMAGE invalide : {0}' $K.Image }
        $K.ImageExplicit = $true
    } else {
        $K.Image = $DefaultImage
    }
    $K.Interactive = (-not $K.Yes) -and (-not $K.DryRun) -and [Environment]::UserInteractive -and (-not [Console]::IsInputRedirected)

    # Système : Windows 64 bits seulement (Linux et macOS : install.sh).
    if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) {
        Fail 'ce script est pour Windows. Sous Linux ou macOS : curl -fsSL https://raw.githubusercontent.com/KAYO74/Kraken/main/install.sh | sh'
    }
    if (-not [Environment]::Is64BitOperatingSystem) { Fail 'KRAKEN a besoin d''un Windows 64 bits, et celui-ci est en 32 bits.' }
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }

    # Lancé depuis un clone du dépôt ? On utilise alors ses fichiers, et son code si besoin.
    if ($Options.ScriptFile) {
        $folder = Split-Path -Parent $Options.ScriptFile
        $isCheckout = $true
        foreach ($name in @('docker-compose.yml', 'deploy\Caddyfile', 'Dockerfile', 'Cargo.toml')) {
            if (-not (Test-Path -LiteralPath (Join-Path $folder $name))) { $isCheckout = $false }
        }
        if ($isCheckout) { $K.Src = [System.IO.Path]::GetFullPath($folder) }
    }
    if ($K.Src) {
        $K.DefaultDir = $K.Src
    } else {
        if (-not $env:USERPROFILE) { Fail 'USERPROFILE n''est pas défini : indique un dossier avec -Dir.' }
        $K.DefaultDir = Join-Path $env:USERPROFILE 'kraken'
    }
    if (-not $K.Dir) { $K.Dir = $K.DefaultDir }
    $K.Dir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($K.Dir)
    $K.EnvFile = Join-Path $K.Dir '.env'
    $K.Update = Test-Path -LiteralPath $K.EnvFile

    # .env est la seule source des réglages : rien de l'environnement ne doit passer devant.
    $composeVars = @('KRAKEN_DOMAIN', 'KRAKEN_PUBLIC_URL', 'KRAKEN_HTTP_PORT', 'KRAKEN_HTTPS_PORT', 'KRAKEN_IMAGE',
        'POSTGRES_PASSWORD', 'KRAKEN_SMTP_URL', 'KRAKEN_SMTP_FROM', 'KRAKEN_AUTH_PER_MIN', 'KRAKEN_CONTACT',
        'KRAKEN_PROCESSORS', 'KRAKEN_SOURCE_URL', 'KRAKEN_SEED', 'RUST_LOG',
        'COMPOSE_FILE', 'COMPOSE_PROJECT_NAME', 'COMPOSE_PROFILES', 'COMPOSE_ENV_FILES')
    $savedEnv = @{}
    foreach ($name in $composeVars) { $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        foreach ($name in $composeVars) { [Environment]::SetEnvironmentVariable($name, $null, 'Process') }

        Say 'KRAKEN : installation dans {0} (Windows, {1}, PowerShell {2})' $K.Dir $arch $PSVersionTable.PSVersion
        if ($K.Update) {
            Say 'Déjà installé : mise à jour, en gardant les réglages et les données.'
            Read-Settings
        } else {
            Set-Configuration
        }
        if ($K.DryRun) {
            Show-Plan
            return
        }
        $env:KRAKEN_IMAGE = $K.Image

        Initialize-Docker
        [void](New-Item -ItemType Directory -Force -Path $K.Dir)
        Install-Files
        # L'image d'abord : si elle manque, rien n'est écrit et la prochaine fois repose la question.
        Initialize-Image
        if (-not $K.Update) {
            Test-OldVolume
            Write-EnvFile
        }
        Start-Kraken
        Wait-Health
        Show-Summary
    } finally {
        foreach ($name in $composeVars) { [Environment]::SetEnvironmentVariable($name, $savedEnv[$name], 'Process') }
    }
}

$KrakenOptions = @{
    Yes        = [bool]$Yes
    Domain     = $Domain
    Local      = [bool]$Local
    Dir        = $Dir
    Port       = $Port
    DryRun     = [bool]$DryRun
    Help       = [bool]$Help
    ScriptFile = $PSCommandPath
}
try {
    Install-Kraken -Options $KrakenOptions
} catch {
    Write-Host ''
    Write-Host ('Erreur : ' + $_.Exception.Message) -ForegroundColor Red
    # Lancé comme fichier : code de sortie 1. Avec irm | iex, on ne ferme pas la fenêtre.
    if ($PSCommandPath) { exit 1 }
}
