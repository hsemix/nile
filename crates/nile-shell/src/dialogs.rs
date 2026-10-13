//! The OS file chooser, through whatever the system ships (no extra dependencies): AppleScript
//! on macOS, a PowerShell `OpenFileDialog` on Windows, `zenity` or `kdialog` elsewhere.
//! Blocks until the user answers.

use std::process::Command;

/// Extensions (`png`, `jpg`) named in an `accept` attribute like `".png,.jpg,image/*"`.
fn extensions(accept: &str) -> Vec<String> {
    accept
        .split(',')
        .filter_map(|t| t.trim().strip_prefix('.'))
        .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(str::to_string)
        .collect()
}

pub fn pick_files(multiple: bool, accept: &str) -> Vec<String> {
    let exts = extensions(accept);
    let output = if cfg!(target_os = "macos") {
        let kinds = if exts.is_empty() { String::new() } else { format!(" of type {{{}}}", exts.iter().map(|e| format!("\"{e}\"")).collect::<Vec<_>>().join(", ")) };
        let script = format!(
            "set fs to choose file{kinds} with multiple selections allowed {multiple}\nset out to \"\"\nif class of fs is not list then set fs to {{fs}}\nrepeat with f in fs\nset out to out & POSIX path of f & linefeed\nend repeat\nout"
        );
        Command::new("osascript").args(["-e", &script]).output()
    } else if cfg!(windows) {
        let filter = if exts.is_empty() { "All files|*.*".to_string() } else { format!("Files|{}", exts.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";")) };
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; $d.Multiselect = ${multiple}; $d.Filter = '{filter}'; if ($d.ShowDialog() -eq 'OK') {{ $d.FileNames -join \"`n\" }}"
        );
        Command::new("powershell").args(["-NoProfile", "-Command", &script]).output()
    } else {
        let mut zenity = Command::new("zenity");
        zenity.arg("--file-selection").arg("--separator=\n");
        if multiple {
            zenity.arg("--multiple");
        }
        if !exts.is_empty() {
            zenity.arg(format!("--file-filter={}", exts.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(" ")));
        }
        zenity.output().or_else(|_| {
            let mut k = Command::new("kdialog");
            k.arg("--getopenfilename");
            if multiple {
                k.args(["--multiple", "--separate-output"]);
            }
            k.output()
        })
    };
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim_end_matches('\r').to_string()).filter(|l| !l.is_empty()).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_extensions_from_accept() {
        assert_eq!(super::extensions(".png, .JPG,image/*,.,.a-b"), vec!["png", "JPG"]);
    }
}
