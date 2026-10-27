import argparse
import json
import os
import sys
from pathlib import Path

import requests
from micloud import MiCloud
from miio.cloud import CloudInterface
from playwright.sync_api import sync_playwright

APP_DIR = Path(os.environ["APPDATA"]) / "yeelight-lamp"
CONFIG = APP_DIR / "config.json"
SESSION = APP_DIR / "session.json"
PROFILE = APP_DIR / "browser_profile"
MODEL = "yeelink.light.lamp43"
UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0"
)


def mask(value: str, keep: int = 4) -> str:
    if not value:
        return "<empty>"
    return value if len(value) <= keep * 2 else f"{value[:keep]}...{value[-keep:]}"


def _cookies_to_session(context) -> dict:
    jar = {
        c["name"]: c["value"]
        for c in context.cookies()
        if "xiaomi" in c["domain"]
    }
    return {k: jar[k] for k in ("userId", "passToken", "cUserId", "deviceId") if k in jar}


def load_session(login: bool = False) -> dict:
    if SESSION.exists() and not login:
        return json.loads(SESSION.read_text(encoding="utf-8"))

    with sync_playwright() as p:
        context = p.chromium.launch_persistent_context(
            user_data_dir=str(PROFILE),
            channel="msedge",
            headless=not login,
            args=["--no-first-run", "--no-default-browser-check"],
        )
        if login:
            page = context.pages[0] if context.pages else context.new_page()
            page.goto("https://account.xiaomi.com/", timeout=60_000)
            print("请在弹出的 Edge 窗口里登录小米账号（手机号/邮箱 + 密码）。")
            print("登录完成后回到这个窗口按回车继续...")
            input()
        session = _cookies_to_session(context)
        context.close()

    missing = [k for k in ("userId", "passToken") if k not in session]
    if missing:
        raise RuntimeError(f"浏览器配置里缺少 {missing}，用 --login 重新登录")
    SESSION.write_text(json.dumps(session, ensure_ascii=False, indent=2), encoding="utf-8")
    return session


def exchange(session: dict):
    http = requests.Session()
    http.headers.update({"User-Agent": UA})
    for name, value in session.items():
        http.cookies.set(name, value, domain=".xiaomi.com")

    resp = http.get(
        "https://account.xiaomi.com/pass/serviceLogin?sid=xiaomiio&_json=true", timeout=25
    )
    data = json.loads(resp.text.replace("&&&START&&&", ""))
    if data.get("code") != 0 or not data.get("ssecurity"):
        raise RuntimeError(f"换票失败: code={data.get('code')} desc={data.get('desc')}")
    location = data.get("location")
    if not location:
        raise RuntimeError("换票响应里没有 location")

    http.get(location, timeout=25)
    service_token = next((c.value for c in http.cookies if c.name == "serviceToken"), None)
    if not service_token:
        raise RuntimeError("没有拿到 serviceToken")
    return data["ssecurity"], service_token, str(data.get("userId") or session["userId"])


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--login", action="store_true", help="打开浏览器手动登录后再取 token")
    args = parser.parse_args()
    APP_DIR.mkdir(parents=True, exist_ok=True)
    try:
        session = load_session(login=args.login)
        ssecurity, service_token, user_id = exchange(session)
    except Exception as exc:
        print(f"失败: {type(exc).__name__}: {exc}")
        print("若是换票失败，试试 python get_token.py --login 重新登录小米账号。")
        return 1

    client = MiCloud(username="", password="")
    client.user_id = user_id
    client.ssecurity = ssecurity
    client.cuser_id = session.get("cUserId", "")
    client.pass_token = session["passToken"]
    client.service_token = service_token

    cloud = CloudInterface(username="", password="")
    cloud._micloud = client
    devices = cloud.get_devices(locale="cn")
    matches = [d for d in devices.values() if d.model == MODEL]
    if not matches:
        print("国区没找到，尝试其它区域...")
        devices = cloud.get_devices()
        matches = [d for d in devices.values() if d.model == MODEL]
    if not matches:
        print(f"账号下没有 {MODEL}。现有设备：")
        for d in devices.values():
            if not d.parent_id:
                print(f"  - {d.name}  model={d.model}  did={d.did}")
        return 1

    dev = matches[0]
    config = {
        "name": dev.name,
        "model": dev.model,
        "did": dev.did,
        "token": dev.token,
        "ip": dev.ip,
        "mac": dev.mac,
    }
    CONFIG.write_text(json.dumps(config, ensure_ascii=False, indent=2), encoding="utf-8")
    print("已写入", CONFIG)
    print("设备:", dev.name)
    print("model:", dev.model)
    print("did:", dev.did, " ip:", dev.ip, " mac:", dev.mac)
    print("token:", mask(dev.token), f"(len={len(dev.token)})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
