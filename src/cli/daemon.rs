//! Daemon management: install/uninstall/status as OS-native background service.
//! Supports Linux (systemd user), macOS (launchd), Windows (registry autostart).

use anyhow::Result;

/// Run the daemon subcommand.
pub fn run(action: &str, port: u16, binary: &str) -> Result<()> {
    match action {
        "install" => install(port, binary),
        "uninstall" => uninstall(),
        "status" => status(),
        _ => {
            println!("Usage: agentrete daemon <install|uninstall|status>");
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
fn install(port: u16, binary: &str) -> Result<()> {
    // Check systemd user service support
    let has_systemd = std::process::Command::new("systemctl")
        .arg("--user")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_systemd {
        anyhow::bail!(
            "systemd user services not available. Requires systemd (most Linux distros have it)."
        );
    }

    let home = std::env::var("HOME")?;
    let svc_dir = format!("{}/.config/systemd/user", home);
    std::fs::create_dir_all(&svc_dir)?;

    // Copy binary to a stable location (~/.local/bin) to survive npx cache cleanup
    let bin_dir = format!("{}/.local/bin", home);
    std::fs::create_dir_all(&bin_dir)?;
    let stable_bin = format!("{}/agentrete", bin_dir);
    std::fs::copy(binary, &stable_bin)?;
    // Ensure executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&stable_bin)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&stable_bin, perms)?;
    }

    let service_content = format!(
        r#"[Unit]
Description=Agentrete Memory Server (MCP)
After=network.target

[Service]
ExecStart={stable_bin} mcp --port {port}
Restart=on-failure
RestartSec=2
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
"#,
        stable_bin = stable_bin,
        port = port
    );

    let svc_path = format!("{}/agentrete.service", svc_dir);
    std::fs::write(&svc_path, &service_content)?;

    // Enable & start
    std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()?;
    std::process::Command::new("systemctl")
        .args(["--user", "enable", "agentrete.service"])
        .status()?;
    std::process::Command::new("systemctl")
        .args(["--user", "start", "agentrete.service"])
        .status()?;

    println!("Agentreate service installed (systemd user).");
    println!("  Status: systemctl --user status agentrete");
    println!("  Logs:   journalctl --user -u agentrete -f");
    Ok(())
}

#[cfg(target_os = "macos")]
fn install(port: u16, binary: &str) -> Result<()> {
    let home = std::env::var("HOME")?;
    let agent_dir = format!("{}/Library/LaunchAgents", home);
    std::fs::create_dir_all(&agent_dir)?;

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>io.agentrete.server</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>mcp</string>
        <string>--port</string>
        <string>{port}</string>
    </array>
    <key>KeepAlive</key>
    <true/>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/agentrete.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/agentrete.log</string>
</dict>
</plist>
"#,
        binary = binary,
        port = port
    );

    let plist_path = format!("{}/io.agentrete.server.plist", agent_dir);
    std::fs::write(&plist_path, &plist_content)?;

    // Load it
    std::process::Command::new("launchctl")
        .args(["load", &plist_path])
        .status()?;

    println!("Agentreate service installed (launchd user agent).");
    println!("  Status: launchctl list | grep agentrete");
    println!("  Logs:   tail -f /tmp/agentrete.log");
    Ok(())
}

#[cfg(target_os = "windows")]
fn install(port: u16, binary: &str) -> Result<()> {
    // Windows: registry Run key for auto-start on login
    let reg_cmd = format!(
        r#"powershell -Command "New-ItemProperty -Path HKCU:\Software\Microsoft\Windows\CurrentVersion\Run -Name Agentrete -Value '{} mcp --port {}' -PropertyType String -Force""#,
        binary.replace('\\', "\\\\"),
        port
    );

    std::process::Command::new("powershell")
        .args(["-Command", &reg_cmd])
        .status()?;

    // Start now
    std::process::Command::new("powershell")
        .args([
            "-Command",
            &format!(
                "Start-Process -WindowStyle Hidden '{}' -ArgumentList 'mcp','--port','{}'",
                binary, port
            ),
        ])
        .status()?;

    println!("Agentreate service installed (Windows registry autostart).");
    println!("  Runs on login and started now.");
    println!("  Uninstall: agentrete daemon uninstall");
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall() -> Result<()> {
    std::process::Command::new("systemctl")
        .args(["--user", "stop", "agentrete.service"])
        .status()
        .ok();
    std::process::Command::new("systemctl")
        .args(["--user", "disable", "agentrete.service"])
        .status()
        .ok();
    let home = std::env::var("HOME")?;
    let svc_path = format!("{}/.config/systemd/user/agentrete.service", home);
    let _ = std::fs::remove_file(&svc_path);
    std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .ok();
    println!("Agentreate service uninstalled.");
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall() -> Result<()> {
    let home = std::env::var("HOME")?;
    let plist_path = format!("{}/Library/LaunchAgents/io.agentrete.server.plist", home);
    std::process::Command::new("launchctl")
        .args(["unload", &plist_path])
        .status()
        .ok();
    let _ = std::fs::remove_file(&plist_path);
    println!("Agentreate service uninstalled.");
    Ok(())
}

#[cfg(target_os = "windows")]
fn uninstall() -> Result<()> {
    std::process::Command::new("powershell")
        .args(["-Command", "Remove-ItemProperty -Path HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run -Name Agentrete -ErrorAction SilentlyContinue"])
        .status().ok();
    std::process::Command::new("powershell")
        .args([
            "-Command",
            "Get-Process agentrete -ErrorAction SilentlyContinue | Stop-Process -Force",
        ])
        .status()
        .ok();
    println!("Agentreate service uninstalled.");
    Ok(())
}

fn status() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("systemctl")
            .args(["--user", "is-active", "agentrete.service"])
            .output()?;
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if out == "active" {
            println!("Status: running (systemd user service)");
        } else {
            println!("Status: not running");
        }
    }

    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("launchctl")
            .args(["list"])
            .output()?;
        let out = String::from_utf8_lossy(&output.stdout);
        if out.contains("io.agentrete.server") {
            println!("Status: running (launchd user agent)");
        } else {
            println!("Status: not running");
        }
    }

    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("powershell")
            .args(["-Command", "Get-Process agentrete -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id"])
            .output()?;
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !out.is_empty() {
            println!("Status: running (PID: {})", out);
        } else {
            println!("Status: not running");
        }
    }

    Ok(())
}
