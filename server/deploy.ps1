# Deploys the sync server to rr over SSH (Windows OpenSSH, host alias `rr`).
#   powershell -ExecutionPolicy Bypass -File server\deploy.ps1 [-Sounds C:\path\to\thu_vien_am]
# Copies the code, makes the venv on first run, keeps the token it already has
# (a new one on first run), restarts the user service. With -Sounds, also
# rebuilds the sound library from the old project's thu_vien_am on the server.
param([string]$Sounds = "")
$ErrorActionPreference = "Stop"
$ssh = "C:\Windows\System32\OpenSSH\ssh.exe"
$scp = "C:\Windows\System32\OpenSSH\scp.exe"
$here = $PSScriptRoot

& $ssh rr 'mkdir -p ~/concat-sync ~/concat-sync-data ~/.config/systemd/user'
& $scp -q "$here\sync_server.py" "$here\make_sound_manifest.py" "$here\requirements.txt" rr:concat-sync/
& $scp -q "$here\concat-sync.service" rr:.config/systemd/user/
& $ssh rr @'
set -e
cd ~/concat-sync
# uv, because rr's Python has no ensurepip (python3-venv is not installed).
[ -x venv/bin/uvicorn ] || { rm -rf venv && ~/.local/bin/uv venv -q venv && ~/.local/bin/uv pip install -q --python venv/bin/python -r requirements.txt; }
[ -s token ] || { python3 -c 'import secrets; print(secrets.token_urlsafe(32))' > token; chmod 600 token; }
systemctl --user daemon-reload
systemctl --user enable --now concat-sync >/dev/null 2>&1
systemctl --user restart concat-sync
sleep 2
systemctl --user is-active concat-sync
'@
if ($LASTEXITCODE -ne 0) { throw "the service did not start; see: ssh rr journalctl --user -u concat-sync -n 50" }

if ($Sounds) {
    & $ssh rr 'rm -rf ~/concat-sync/thu_vien_am && mkdir -p ~/concat-sync/thu_vien_am'
    & $scp -q -r "$Sounds\*" rr:concat-sync/thu_vien_am/
    & $ssh rr 'cd ~/concat-sync && venv/bin/python make_sound_manifest.py --src thu_vien_am --out ~/concat-sync-data/audio && rm -rf thu_vien_am'
}
Write-Host "deployed: http://100.114.205.62:8700 (token in rr:~/concat-sync/token)"
