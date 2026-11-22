# Puts a "VideoEdit" shortcut on the desktop and in the Start menu that opens
# the last build through launch.bat, so the fork's settings are always loaded.
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
$shell = New-Object -ComObject WScript.Shell
$places = @([Environment]::GetFolderPath("Desktop"), [Environment]::GetFolderPath("Programs"))
foreach ($place in $places) {
    $link = $shell.CreateShortcut((Join-Path $place "VideoEdit.lnk"))
    $link.TargetPath = Join-Path $root "launch.bat"
    $link.WorkingDirectory = $root
    $link.IconLocation = (Join-Path $root "assets\icons\videoedit.ico") + ",0"
    $link.WindowStyle = 7  # minimized: the console only flashes in the taskbar
    $link.Description = "VideoEdit (Concat fork)"
    $link.Save()
    "shortcut: " + $link.FullName
}
