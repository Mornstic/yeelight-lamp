//! 命令行验证工具：不启动界面，直接和真实的灯对话。
//!
//! 用法：
//!   lampctl info
//!   lampctl status
//!   lampctl on | off | toggle
//!   lampctl bright <1-100>
//!   lampctl temp <2700-6500>
//!   lampctl preset <office|focus|read|relax>
//!   lampctl autofill <on|off>
//!   lampctl scan
//!   lampctl watch <秒数>

use std::process::ExitCode;
use std::time::Duration;

use serde_json::json;
use yeelight_lamp::device::{preset_by_id, Lamp, Status, PRESETS};
use yeelight_lamp::miio;


#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("status");

    let result = match command {
        "info" => info(),
        "scan" => scan().await,
        "watch" => watch(args.get(1).and_then(|text| text.parse().ok())).await,
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => run_lamp(other, &args[1..]).await,
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("错误：{message}");
            ExitCode::FAILURE
        }
    }
}

fn info() -> Result<(), String> {
    let lamp = Lamp::load().map_err(|err| err.to_string())?;
    let config = lamp.config();
    println!("名称      {}", config.name);
    println!("型号      {}", config.model);
    println!("设备 ID   {}", config.did);
    println!("MAC       {}", config.mac.as_deref().unwrap_or("未记录"));
    println!("IP        {}", config.ip.map_or("未记录".into(), |ip| ip.to_string()));
    println!("配置      {}", config.path().display());
    println!("token     已读取（长度 32，不在终端输出）");
    println!("预设      {}", PRESETS.iter().map(|p| p.label).collect::<Vec<_>>().join(" / "));
    Ok(())
}

async fn scan() -> Result<(), String> {
    println!("广播寻找 miIO 设备（1.5 秒）…");
    let found = miio::scan(Duration::from_millis(1500)).await;
    if found.is_empty() {
        println!("没有收到任何应答");
        return Ok(());
    }
    for device in found {
        println!(
            "  {}  设备 ID {}  时间戳 {}",
            device.ip, device.device_id, device.stamp
        );
    }
    Ok(())
}

async fn watch(seconds: Option<u64>) -> Result<(), String> {
    let mut lamp = Lamp::load().map_err(|err| err.to_string())?;
    println!("IP  {}", lamp.connect().await.map_err(|err| err.to_string())?);
    for _ in 0..seconds.unwrap_or(10) {
        match lamp.status_with_latency().await {
            Ok((status, latency)) => println!("{}  {} ms", format_status(status), latency),
            Err(err) => println!("读取失败：{err}"),
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}

async fn run_lamp(command: &str, rest: &[String]) -> Result<(), String> {
    let mut lamp = Lamp::load().map_err(|err| err.to_string())?;

    match command {
        "status" => {
            let (status, latency) = lamp
                .status_with_latency()
                .await
                .map_err(|err| err.to_string())?;
            println!("IP  {}  ·  {} ms", lamp.ip().unwrap(), latency);
            println!("{}", format_status(status));
        }
        "on" | "off" | "toggle" => {
            let target = if command == "toggle" {
                !lamp.status().await.map_err(|err| err.to_string())?.power
            } else {
                command == "on"
            };
            lamp.set_power(target).await.map_err(|err| err.to_string())?;
            println!("{}", if target { "已开灯" } else { "已关灯" });
        }
        "bright" | "brightness" => {
            let value: u8 = parse_arg(rest, command)?.parse().map_err(|_| "亮度需要 1-100 的整数")?;
            lamp.set_brightness(value)
                .await
                .map_err(|err| err.to_string())?;
            println!("亮度 → {}", value.clamp(1, 100));
        }
        "temp" | "color-temp" => {
            let value: u16 = parse_arg(rest, command)?.parse().map_err(|_| "色温需要 2700-6500 的整数")?;
            lamp.set_color_temp(value)
                .await
                .map_err(|err| err.to_string())?;
            println!("色温 → {}K", value.clamp(2700, 6500));
        }
        "preset" => {
            let name = parse_arg(rest, command)?;
            let preset = preset_by_id(name)
                .ok_or_else(|| format!("未知预设 {name}，可选：office/focus/read/relax"))?;
            lamp.apply_preset(preset).await.map_err(|err| err.to_string())?;
            println!(
                "预设 → {}（{}% / {}K）",
                preset.label, preset.brightness, preset.color_temp
            );
        }
        "autofill" => {
            let on = parse_arg(rest, command)? == "on";
            lamp.set_auto_fill(on).await.map_err(|err| err.to_string())?;
            println!("自动补光 → {}", if on { "开" } else { "关" });
        }
        "light" => {
            // lampctl light <brightness> <kelvin>：先开灯再写入，验证唤醒逻辑
            let brightness: u8 = rest
                .first()
                .ok_or("缺少亮度")?
                .parse()
                .map_err(|_| "亮度需要 1-100 的整数")?;
            let color_temp: u16 = rest
                .get(1)
                .ok_or("缺少色温")?
                .parse()
                .map_err(|_| "色温需要 2700-6500 的整数")?;
            lamp.apply_lighting(brightness, color_temp)
                .await
                .map_err(|err| err.to_string())?;
            println!(
                "已设为 {}% / {}K",
                brightness.clamp(1, 100),
                color_temp.clamp(2700, 6500)
            );
        }
        "raw" => {
            // 排障用：直接发一条 miIO 指令并把结果原样打印
            let method = parse_arg(rest, "raw")?;
            let params: serde_json::Value = rest
                .get(1)
                .map(|text| serde_json::from_str(text).map_err(|_| "参数需要是 JSON"))
                .transpose()?
                .unwrap_or_else(|| json!([]));
            let value = lamp.raw_command(method, params).await.map_err(|err| err.to_string())?;
            println!("{value:#}");
        }
        other => {
            print_help();
            return Err(format!("未知命令 {other}"));
        }
    }

    Ok(())
}

fn parse_arg<'a>(rest: &'a [String], command: &str) -> Result<&'a str, String> {
    rest.first()
        .map(String::as_str)
        .ok_or_else(|| format!("{command} 缺少参数"))
}

fn format_status(status: Status) -> String {
    format!(
        "{}  亮度 {:>3}%  色温 {:>4}K  模式 {}{}",
        if status.power { "开灯" } else { "关灯" },
        status.brightness,
        status.color_temp,
        status.mode,
        if status.auto_fill { "  自动补光 开" } else { "" }
    )
}

fn print_help() {
    println!(
        "yeelight-lamp 验证工具\n\
         \n\
         用法：lampctl <命令>\n\
         \n\
         命令：\n\
           info                     显示配置摘要（不输出 token）\n\
           status                   读取开关/亮度/色温/模式/补光\n\
           on | off | toggle        开灯、关灯、切换\n\
           bright <1-100>           设置亮度\n\
           temp <2700-6500>         设置色温\n\
           preset <名称>            办公 / 专注 / 阅读 / 休闲\n\
           autofill <on|off>        自动补光\n\
           light <亮度> <色温>      先开灯再设置亮度与色温\n\
           raw <方法> [JSON参数]    直接发 miIO 指令\n\
           scan                     广播寻找局域网里的 miIO 设备\n\
           watch [秒数]             每秒读一次状态"
    );
}
