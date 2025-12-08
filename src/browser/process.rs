#![allow(dead_code)]
use crate::info;
use anyhow::{Context, Result, anyhow};
use rand::distr::Alphanumeric;
use rand::{Rng, rng};
use regex::Regex;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use which::which;

pub struct CustomTempDir {
    pub path: PathBuf,
}

impl CustomTempDir {
    pub fn new(base: PathBuf, prefix: &str) -> Result<Self> {
        std::fs::create_dir_all(&base)?;
        let name = format!(
            "{}_{}_{}",
            prefix,
            chrono::Local::now().format("%Y%m%d_%H%M%S"),
            rng()
                .sample_iter(&Alphanumeric)
                .take(6)
                .map(char::from)
                .collect::<String>()
        );
        let path = base.join(name);
        std::fs::create_dir(&path)?;
        Ok(Self { path })
    }
}

impl Drop for CustomTempDir {
    fn drop(&mut self) {
        for i in 0..10 {
            if std::fs::remove_dir_all(&self.path).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100 * (i as u64 + 1).min(3)));
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub struct BrowserProcess {
    pub child: Child,
    pub _temp: CustomTempDir,
}

impl Drop for BrowserProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub async fn launch(headless: bool) -> Result<(BrowserProcess, String)> {
    let temp = CustomTempDir::new(
        std::env::current_dir()?.join("data").join("temp_browser"),
        "cdp",
    )?;
    let exe = find_chrome()?;
    let port = (8000..9000)
        .find(|&p| std::net::TcpListener::bind(("127.0.0.1", p)).is_ok())
        .ok_or(anyhow!("No available port"))?;

    let mut args = vec![
        format!("--remote-debugging-port={}", port),
        format!("--user-data-dir={}", temp.path.display()),
        "--no-sandbox".into(), "--no-zygote".into(), "--in-process-gpu".into(),
        "--disable-dev-shm-usage".into(), "--disable-background-networking".into(),
        "--disable-default-apps".into(), "--disable-extensions".into(),
        "--disable-sync".into(), "--disable-translate".into(),
        "--metrics-recording-only".into(), "--safebrowsing-disable-auto-update".into(),
        "--mute-audio".into(), "--no-first-run".into(), "--hide-scrollbars".into(),
        "--window-size=1200,1600".into(),
        "--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36".into()
    ];
    if headless {
        args.push("--headless=new".into());
    }

    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new(&exe);
        c.creation_flags(0x08000000);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = Command::new(&exe);

    info!(target: "Browser", "启动浏览器进程: {:?}", exe);
    let mut child = cmd.args(args).stderr(Stdio::piped()).spawn()?;
    let stderr = child.stderr.take().context("No stderr")?;
    let ws_url = wait_for_ws(stderr).await?;

    Ok((BrowserProcess { child, _temp: temp }, ws_url))
}

fn find_chrome() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("CHROME") {
        return Ok(p.into());
    }
    let apps = [
        "google-chrome-stable",
        "chromium",
        "chrome",
        "msedge",
        "microsoft-edge",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ];
    for app in apps {
        if let Ok(p) = which(app) {
            return Ok(p);
        }
    }

    #[cfg(windows)]
    {
        let paths = [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        ];
        for p in paths {
            if Path::new(p).exists() {
                return Ok(p.into());
            }
        }
    }
    Err(anyhow!("Chrome/Edge not found. Set CHROME env var."))
}

async fn wait_for_ws(stderr: std::process::ChildStderr) -> Result<String> {
    let reader = BufReader::new(stderr);
    let re = Regex::new(r"listening on (.*/devtools/browser/.*)$")?;
    tokio::task::spawn_blocking(move || {
        for line in reader.lines() {
            let l = line?;
            if let Some(cap) = re.captures(&l) {
                return Ok(cap[1].to_string());
            }
        }
        Err(anyhow!("WS URL not found in stderr"))
    })
    .await?
}
