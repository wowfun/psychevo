#!/usr/bin/env python3
import json
import sys
import urllib.parse
import urllib.request


def respond(request, result):
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)


def fetch_json(url, method="GET", token=None):
    headers = {}
    data = None
    if method == "POST":
        data = b'{"get_updates_buf":"","base_info":{"channel_version":"1.0.2"}}'
        headers["Content-Type"] = "application/json"
        headers["Authorization"] = "Bearer " + token
    with urllib.request.urlopen(
        urllib.request.Request(url, data=data, headers=headers, method=method)
    ) as response:
        return json.load(response)


for line in sys.stdin:
    request = json.loads(line)
    method = request["method"]
    params = request.get("params", {})
    if method == "initialize":
        respond(request, {
            "protocol": "psychevo-extension/1",
            "extensionId": "psychevo.channel.wechat",
            "capabilities": {
                "channels": [{
                    "channel": "wechat",
                    "deliveryCapabilities": ["poll", "text", "qr_setup"]
                }]
            }
        })
    elif method == "channel/wechat/qr/start":
        base_url = params["baseUrl"].rstrip("/")
        body = fetch_json(base_url + "/ilink/bot/get_bot_qrcode?bot_type=3")
        qrcode = body["qrcode"]
        qr_url = body.get("qrcode_img_content") or qrcode
        image = qr_url if qr_url.lower().startswith("data:image/") else None
        svg = None if image else "<svg data-test=\"wechat-qr\"></svg>"
        respond(request, {
            "qrcode": qrcode,
            "qrUrl": qr_url,
            "qrImage": image,
            "qrSvg": svg,
            "baseUrl": base_url
        })
    elif method == "channel/wechat/qr/poll":
        base_url = params["baseUrl"].rstrip("/")
        qrcode = urllib.parse.quote(params["qrcode"], safe="")
        body = fetch_json(base_url + "/ilink/bot/get_qrcode_status?qrcode=" + qrcode)
        if body["status"] == "confirmed":
            respond(request, {
                "status": "confirmed",
                "accountId": body["ilink_bot_id"],
                "token": body["bot_token"],
                "baseUrl": body.get("baseurl") or base_url,
                "userId": body.get("ilink_user_id")
            })
        else:
            respond(request, {
                "status": body["status"],
                "message": "waiting",
                "baseUrl": body.get("baseurl") or base_url
            })
    elif method == "channel/wechat/health":
        base_url = params["baseUrl"].rstrip("/")
        body = fetch_json(base_url + "/ilink/bot/getupdates", "POST", params["credential"])
        respond(request, {
            "ok": body.get("ret", 0) == 0 and body.get("errcode", 0) == 0,
            "reason": "polling_empty",
            "message": body.get("errmsg"),
            "msgCount": len(body.get("msgs", []))
        })
    elif method == "shutdown":
        respond(request, {})
        break
    else:
        respond(request, {})
