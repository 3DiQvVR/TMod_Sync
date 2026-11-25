use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env,
    net::IpAddr,
    path::PathBuf,
    process::{Output, Stdio},
    time::Duration,
};
use tokio::{process::Command, time::timeout};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Tailscale,
    Zerotier,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub online: bool,
    pub ip: Option<String>,
    pub detail: String,
}

impl Status {
    fn offline(detail: impl Into<String>) -> Self {
        Self {
            online: false,
            ip: None,
            detail: detail.into(),
        }
    }
}

impl Provider {
    pub fn download_url(self) -> &'static str {
        match self {
            Self::Tailscale => "https://tailscale.com/download",
            Self::Zerotier => "https://www.zerotier.com/download/",
        }
    }
}

fn search_dirs(provider: Provider) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    for var in [
        "ProgramW6432",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
    ] {
        if let Some(p) = env::var_os(var) {
            dirs.push(PathBuf::from(p).join(match provider {
                Provider::Tailscale => "Tailscale",
                Provider::Zerotier => "ZeroTier/One",
            }));
        }
    }
    #[cfg(unix)]
    dirs.extend(
        [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
            "/usr/sbin",
            "/usr/local/sbin",
        ]
        .map(PathBuf::from),
    );
    if let Some(path) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&path).filter(|p| p.is_absolute()));
    }
    #[cfg(windows)]
    {
        use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
        let service = match provider {
            Provider::Tailscale => "Tailscale",
            Provider::Zerotier => "ZeroTierOneService",
        };
        if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(format!(r"System\CurrentControlSet\Services\{service}"))
        {
            if let Ok(image) = key.get_value::<String, _>("ImagePath") {
                if let Some(i) = image.to_ascii_lowercase().find(".exe") {
                    let exe = PathBuf::from(image[..i + 4].trim_start_matches('"'));
                    if exe.is_absolute() {
                        if let Some(dir) = exe.parent() {
                            dirs.push(dir.to_owned());
                        }
                    }
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    if provider == Provider::Tailscale {
        dirs.push("/Applications/Tailscale.app/Contents/MacOS".into());
    }
    #[cfg(target_os = "linux")]
    let _ = provider;
    dirs
}

fn find_cli(provider: Provider) -> Option<PathBuf> {
    let cli = match provider {
        Provider::Tailscale => "tailscale",
        Provider::Zerotier => "zerotier-cli",
    };
    let mut names = vec![format!("{cli}{}", env::consts::EXE_SUFFIX)];
    #[cfg(windows)]
    if provider == Provider::Zerotier {
        names.extend(
            [
                "zerotier-one_x64.exe",
                "zerotier-one_x86.exe",
                "zerotier-one_arm64.exe",
                "zerotier-cli.bat",
            ]
            .map(str::to_owned),
        );
    }
    #[cfg(target_os = "macos")]
    if provider == Provider::Tailscale {
        names.push("Tailscale".into());
    }
    search_dirs(provider)
        .iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|p| p.is_file())
}

fn gui(provider: Provider) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let p = PathBuf::from(match provider {
            Provider::Tailscale => "/Applications/Tailscale.app",
            Provider::Zerotier => "/Applications/ZeroTier.app",
        });
        if p.is_dir() {
            return Some(p);
        }
    }
    let name = match provider {
        Provider::Tailscale => "tailscale-ipn",
        Provider::Zerotier => "zerotier_desktop_ui",
    };
    search_dirs(provider)
        .iter()
        .map(|d| d.join(format!("{name}{}", env::consts::EXE_SUFFIX)))
        .find(|p| p.is_file())
}

#[derive(Serialize)]
pub struct Inventory {
    pub tailscale: bool,
    pub zerotier: bool,
}
impl Inventory {
    pub fn preferred(&self) -> Provider {
        if !self.tailscale && self.zerotier {
            Provider::Zerotier
        } else {
            Provider::Tailscale
        }
    }
    fn has(&self, p: Provider) -> bool {
        match p {
            Provider::Tailscale => self.tailscale,
            Provider::Zerotier => self.zerotier,
        }
    }
}
pub fn inventory() -> Inventory {
    let installed = |p| find_cli(p).is_some() || gui(p).is_some();
    Inventory {
        tailscale: installed(Provider::Tailscale),
        zerotier: installed(Provider::Zerotier),
    }
}

fn command(provider: Provider) -> Result<Command> {
    let program = find_cli(provider).context("未找到组网命令行组件，请打开本机组网客户端")?;
    let mut cmd = Command::new(&program);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
        if program
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("zerotier-one_"))
        {
            cmd.arg("-q");
        }
    }
    cmd.stdin(Stdio::null()).kill_on_drop(true);
    Ok(cmd)
}

async fn run(provider: Provider, args: &[&str], secs: u64) -> Result<Output> {
    let mut cmd = command(provider)?;
    timeout(Duration::from_secs(secs), cmd.args(args).output())
        .await
        .context("组网服务响应超时，请检查本机客户端")?
        .context("无法执行组网 CLI")
}

fn check_output(out: &Output) -> Result<()> {
    if !out.status.success() {
        let error = if out.stderr.is_empty() {
            &out.stdout
        } else {
            &out.stderr
        };
        let detail: String = String::from_utf8_lossy(error)
            .trim()
            .chars()
            .take(350)
            .collect();
        let reason = detail.to_ascii_lowercase();
        if reason.contains("authtoken")
            || reason.contains("permission denied")
            || reason.contains("access is denied")
        {
            bail!("组网服务需要本机授权。点击连接，在客户端中完成授权。");
        }
        if reason.contains("failed to connect")
            || reason.contains("cannot connect")
            || reason.contains("connection refused")
        {
            bail!("本机组网服务尚未响应。点击连接以唤起客户端。");
        }
        bail!("组网服务返回错误 ({}) · {detail}", out.status);
    }
    Ok(())
}

fn preferred_ip(ips: &Value) -> Option<String> {
    let candidates = ips
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|s| s.split('/').next()?.parse::<IpAddr>().ok())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified() && !ip.is_multicast());
    candidates
        .min_by_key(|ip| !ip.is_ipv4())
        .map(|ip| ip.to_string())
}

fn tailscale_status(value: &Value) -> Status {
    let state = value["BackendState"].as_str().unwrap_or("未知状态");
    if state != "Running" || value["Self"]["Online"].as_bool() != Some(true) {
        return Status::offline(match state {
            "NeedsLogin" => "需要登录 Tailscale",
            "NeedsMachineAuth" => "设备正在等待管理员授权",
            "Stopped" => "Tailscale 已断开",
            "Running" => "本机 Tailscale 节点离线",
            _ => state,
        });
    }
    if let Some(ip) = preferred_ip(&value["TailscaleIPs"])
        .or_else(|| preferred_ip(&value["Self"]["TailscaleIPs"]))
        .or_else(|| adapter_ip(Provider::Tailscale, "", None))
    {
        Status {
            online: true,
            ip: Some(ip),
            detail: "已接入 Tailscale".into(),
        }
    } else {
        Status::offline("Tailscale 尚未分配虚拟 IP")
    }
}

fn valid_network_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn adapter_candidate(
    provider: Provider,
    id: &str,
    device: Option<&str>,
    name: &str,
    ip: IpAddr,
) -> bool {
    let IpAddr::V4(ip) = ip else { return false };
    let alias = name.to_ascii_lowercase();
    let exact = device.is_some_and(|d| !d.is_empty() && d.eq_ignore_ascii_case(name));
    match provider {
        Provider::Tailscale => {
            alias.contains("tailscale")
                && ip.octets()[0] == 100
                && (64..=127).contains(&ip.octets()[1])
        }
        Provider::Zerotier => {
            let zt_device = alias.len() == 10
                && alias.starts_with("zt")
                && alias[2..].bytes().all(|b| b.is_ascii_alphanumeric());
            ip.is_private()
                && (exact
                    || ((alias.contains("zerotier") || zt_device)
                        && (id.is_empty() || alias.contains(&id.to_ascii_lowercase()))))
        }
    }
}

fn adapter_ip(provider: Provider, id: &str, device: Option<&str>) -> Option<String> {
    let adapters = if_addrs::get_if_addrs().ok()?;
    let mut addresses = adapters
        .iter()
        .filter(|a| a.is_oper_up() && adapter_candidate(provider, id, device, &a.name, a.ip()))
        .map(|a| a.ip());
    let first = addresses.next()?;
    addresses.all(|ip| ip == first).then(|| first.to_string())
}

fn zerotier_status(nets: &Value, id: &str) -> Result<Status> {
    let list = nets.as_array().context("ZeroTier 网络列表格式异常")?;
    let net = if id.is_empty() {
        match list.as_slice() {
            [] => return Ok(Status::offline("尚未加入 ZeroTier 网络")),
            [one] => one,
            _ => return Ok(Status::offline("已加入多个网络，请在设置中指定房间网络 ID")),
        }
    } else {
        ensure!(valid_network_id(id), "ZeroTier 网络 ID 应为 16 位十六进制");
        let Some(net) = list.iter().find(|n| {
            n["id"]
                .as_str()
                .or_else(|| n["nwid"].as_str())
                .is_some_and(|s| s.eq_ignore_ascii_case(id))
        }) else {
            return Ok(Status::offline("尚未加入指定的 ZeroTier 网络"));
        };
        net
    };
    let status = net["status"].as_str().unwrap_or("未知状态");
    if status != "OK" {
        return Ok(Status::offline(match status {
            "ACCESS_DENIED" => "请房主在 ZeroTier Central 授权此设备".into(),
            "REQUESTING_CONFIGURATION" => "正在向 ZeroTier 控制器请求配置".into(),
            _ => format!("ZeroTier 网络状态 · {status}"),
        }));
    }
    let Some(ip) = preferred_ip(&net["assignedAddresses"]).or_else(|| {
        adapter_ip(
            Provider::Zerotier,
            if list.len() == 1 { "" } else { id },
            net["portDeviceName"].as_str(),
        )
    }) else {
        return Ok(Status::offline("ZeroTier 网络尚未分配虚拟 IP"));
    };
    Ok(Status {
        online: true,
        ip: Some(ip),
        detail: format!(
            "已接入 {}",
            net["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("ZeroTier")
        ),
    })
}

async fn probe(provider: Provider, network_id: &str) -> Result<Status> {
    let query = match provider {
        Provider::Tailscale => &["status", "--json", "--peers=false"][..],
        Provider::Zerotier => &["-j", "info"][..],
    };
    let out = run(provider, query, 4).await?;
    // Some Tailscale releases exit nonzero during login but still return structured status.
    let json: Value = match serde_json::from_slice(&out.stdout) {
        Ok(v) => v,
        Err(e) => {
            check_output(&out)?;
            return Err(e).context("网络状态 JSON 无法读取");
        }
    };
    if provider == Provider::Tailscale {
        let status = tailscale_status(&json);
        if status.online || json.get("BackendState").is_none() {
            check_output(&out)?;
        }
        return Ok(status);
    }
    check_output(&out)?;
    if json["online"].as_bool() != Some(true) {
        return Ok(Status::offline("ZeroTier 节点离线"));
    }
    let nets = run(provider, &["-j", "listnetworks"], 4).await?;
    check_output(&nets)?;
    zerotier_status(&serde_json::from_slice(&nets.stdout)?, network_id.trim())
}

pub async fn status(provider: Provider, network_id: &str) -> Status {
    probe(provider, network_id)
        .await
        .unwrap_or_else(|error| Status::offline(format!("{error:#}")))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    installed: Inventory,
    status: Status,
    adapter_ip: Option<String>,
}
pub async fn overview(provider: Provider, id: &str) -> Overview {
    let installed = inventory();
    let status = if installed.has(provider) {
        status(provider, id).await
    } else {
        Status::offline("尚未检测到所选客户端")
    };
    let adapter_ip = if installed.has(provider) && !status.online {
        adapter_ip(provider, id, None)
    } else {
        None
    };
    Overview {
        installed,
        status,
        adapter_ip,
    }
}

pub async fn connect(provider: Provider, network_id: &str) -> Result<String> {
    if provider == Provider::Zerotier && !network_id.is_empty() {
        ensure!(
            valid_network_id(network_id.trim()),
            "网络 ID 应为 16 位十六进制"
        );
    }
    match cli_connect(provider, network_id).await {
        Ok(message) => Ok(message),
        Err(error) => {
            let Some(app) = gui(provider) else {
                return Err(error);
            };
            #[cfg(target_os = "macos")]
            if app.extension().is_some_and(|s| s == "app") {
                open::that(app)?;
                return Ok("已打开组网客户端，请在其中完成连接。".into());
            }
            std::process::Command::new(app)
                .spawn()
                .map(|mut child| {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                })
                .context("无法打开本机组网客户端")?;
            Ok("已唤起组网客户端，请在窗口或系统托盘中完成连接。".into())
        }
    }
}

async fn cli_connect(provider: Provider, network_id: &str) -> Result<String> {
    match provider {
        Provider::Zerotier => {
            let id = network_id.trim();
            ensure!(
                valid_network_id(id),
                "请在网络设置中填写房主提供的 16 位 ZeroTier 网络 ID"
            );
            check_output(&run(provider, &["join", id], 8).await?)?;
            Ok("已提交入网请求；私有网络需房主授权".into())
        }
        Provider::Tailscale => {
            // The daemon owns authentication. The finite CLI wait also works on machines
            // with no browser session; the UI keeps polling independently.
            let out = run(provider, &["up", "--json", "--timeout=4s"], 8).await?;
            let mut auth_url = None;
            for block in serde_json::Deserializer::from_slice(&out.stdout).into_iter::<Value>() {
                let block = block.context("无法读取 Tailscale 接入结果")?;
                if block["BackendState"] == "Running" {
                    return Ok("已接入 Tailscale".into());
                }
                if block["BackendState"] == "NeedsMachineAuth" {
                    return Ok("请让 tailnet 管理员授权本机，授权后将自动刷新".into());
                }
                if let Some(url) = block["AuthURL"].as_str() {
                    let url = tauri::Url::parse(url).context("Tailscale 登录链接无效")?;
                    ensure!(
                        url.scheme() == "https" && url.host_str().is_some(),
                        "Tailscale 登录需要 HTTPS 地址"
                    );
                    auth_url = Some(url);
                }
            }
            if let Some(url) = auth_url {
                open::that(url.as_str())
                    .context("无法打开 Tailscale 登录页，请使用本机 Tailscale 客户端登录")?;
                return Ok("已打开登录页，完成授权后会自动刷新状态".into());
            }
            check_output(&out)?;
            Ok("已提交 Tailscale 连接请求".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn adapter_address_must_belong_to_the_requested_tunnel() {
        let ts = "100.100.2.9".parse().unwrap();
        let zt = "192.168.9.12".parse().unwrap();
        assert!(adapter_candidate(
            Provider::Tailscale,
            "",
            None,
            "tailscale0",
            ts
        ));
        assert!(!adapter_candidate(
            Provider::Tailscale,
            "",
            None,
            "Wi-Fi",
            ts
        ));
        assert!(!adapter_candidate(
            Provider::Tailscale,
            "",
            None,
            "Tailscale",
            "100.2.3.4".parse().unwrap()
        ));
        assert!(adapter_candidate(
            Provider::Zerotier,
            "",
            None,
            "ZeroTier One",
            zt
        ));
        assert!(!adapter_candidate(
            Provider::Zerotier,
            "",
            None,
            "Ethernet",
            zt
        ));
        assert!(!adapter_candidate(
            Provider::Zerotier,
            "aaaaaaaaaaaaaaaa",
            None,
            "ZeroTier One [bbbbbbbbbbbbbbbb]",
            zt
        ));
        assert!(adapter_candidate(
            Provider::Zerotier,
            "aaaaaaaaaaaaaaaa",
            Some("zt12345678"),
            "zt12345678",
            zt
        ));
    }

    #[test]
    fn tailscale_requires_running_online_node_and_prefers_ipv4() {
        let mut data = json!({"BackendState":"Running","Self":{"Online":true},"TailscaleIPs":["fd7a:115c:a1e0::1","100.77.8.2"]});
        assert_eq!(tailscale_status(&data).ip.as_deref(), Some("100.77.8.2"));
        data["Self"]["Online"] = json!(false);
        assert!(!tailscale_status(&data).online);
        data["Self"]["Online"] = json!(true);
        data["BackendState"] = json!("NeedsLogin");
        assert!(!tailscale_status(&data).online);
    }

    #[test]
    fn zerotier_matches_network_and_requires_authorization_and_address() {
        let mut nets = json!([
            {"id":"ffffffffffffffff","status":"OK","assignedAddresses":["10.0.0.2/24"]},
            {"nwid":"abcdef0123456789","status":"ACCESS_DENIED","assignedAddresses":["10.1.0.2/24"]}
        ]);
        assert!(!zerotier_status(&nets, "").unwrap().online);
        assert!(!zerotier_status(&nets, "abcdef0123456789").unwrap().online);
        assert!(!zerotier_status(&nets, "0000000000000001").unwrap().online);
        nets[1]["status"] = json!("OK");
        assert_eq!(
            zerotier_status(&nets, "ABCDEF0123456789")
                .unwrap()
                .ip
                .as_deref(),
            Some("10.1.0.2")
        );
        nets[1]["assignedAddresses"] = json!([]);
        assert!(!zerotier_status(&nets, "abcdef0123456789").unwrap().online);
        assert!(zerotier_status(&nets, "not-a-network-id").is_err());
    }
}
