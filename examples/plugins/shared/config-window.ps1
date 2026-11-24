Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$form = New-Object System.Windows.Forms.Form
$form.Text = 'GameNode plugin preferences'
$form.Width = 420
$form.Height = 150
$form.StartPosition = 'CenterScreen'
$form.KeyPreview = $true
$label = New-Object System.Windows.Forms.Label
$label.Text = 'Preferred catalog label'
$label.AutoSize = $true
$label.Location = New-Object System.Drawing.Point(12, 15)
$text = New-Object System.Windows.Forms.TextBox
$text.Text = $args[0]
$text.Width = 370
$text.Location = New-Object System.Drawing.Point(12, 40)
$save = New-Object System.Windows.Forms.Button
$save.Text = 'Save'
$save.DialogResult = [System.Windows.Forms.DialogResult]::OK
$save.Location = New-Object System.Drawing.Point(220, 75)
$cancel = New-Object System.Windows.Forms.Button
$cancel.Text = 'Cancel'
$cancel.DialogResult = [System.Windows.Forms.DialogResult]::Cancel
$cancel.Location = New-Object System.Drawing.Point(305, 75)
$form.Controls.AddRange(@($label, $text, $save, $cancel))
$form.AcceptButton = $save
$form.CancelButton = $cancel
if ($form.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    [Console]::Out.Write($text.Text)
    exit 0
}
exit 1
