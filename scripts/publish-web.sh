#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
python3 scripts/package.py --web-only --output artifacts/web-bundle
# The daemon belongs to a separate unit, so Web redeploy cannot kill shared documents.
runtime="${XDG_RUNTIME_DIR:-$HOME/.cache}/miocad"
service_hash="$(sha256sum artifacts/web-bundle/bin/cad-service artifacts/web-bundle/bin/cad-server | sha256sum | cut -c1-16)"
service_release="$HOME/.local/share/miocad/service-$service_hash"
mkdir -p "$service_release/bin" "$HOME/.config/systemd/user"
if [[ ! -x "$service_release/bin/cad-service" ]]; then
    install -m 700 artifacts/web-bundle/bin/cad-service "$service_release/bin/cad-service"
    install -m 700 artifacts/web-bundle/bin/cad-server "$service_release/bin/cad-server"
fi
unit="$HOME/.config/systemd/user/miocad-cad-service.service"
# Keep an existing daemon/unit on its existing kernel version. Upgrading it is a separate,
# explicit operation because the current document store is in memory.
if [[ ! -e "$unit" ]]; then
    cat > "$unit" <<UNIT
[Unit]
Description=AI-CAD shared document service
After=network-online.target
[Service]
ExecStart="$service_release/bin/cad-service" --runtime-dir "$runtime" --worker "$service_release/bin/cad-server"
Restart=on-failure
RestartSec=3
NoNewPrivileges=yes
UMask=0077
[Install]
WantedBy=default.target
UNIT
    systemctl --user daemon-reload
fi
systemctl --user enable miocad-cad-service.service
# Read-only health check; never print credentials or start a competing daemon to check it.
if ! python3 - "$runtime" <<'PY'
import json,socket,sys
from pathlib import Path
try:
    e=json.loads((Path(sys.argv[1])/'endpoint.json').read_bytes())
    host,port=e['address'].rsplit(':',1)
    with socket.create_connection((host,int(port)),timeout=2) as connection:
        connection.sendall((json.dumps({'protocol':e['protocol'],'service_id':e['service_id'],'token':e['token'],'request':{'method':'ping'}})+'\n').encode())
        reply=json.loads(connection.makefile('rb').readline())
        assert reply.get('result',{}).get('ready') is True
except Exception:sys.exit(1)
PY
then
    systemctl --user start miocad-cad-service.service
fi
# Upgrade GPU/viewport handling in an independent relay, preserving the document owner,
# its service/worker IDs, counters and Undo/Redo. SIGTERM restores the upstream listener.
cat > "$HOME/.config/systemd/user/miocad-render-service.service" <<UNIT
[Unit]
Description=AI-CAD shared GPU viewport relay
After=network-online.target miocad-cad-service.service
[Service]
ExecStart="$service_release/bin/cad-service" --relay-runtime-dir "$runtime"
Restart=on-failure
RestartSec=3
TimeoutStopSec=30
NoNewPrivileges=yes
UMask=0077
[Install]
WantedBy=default.target
UNIT
systemctl --user daemon-reload
systemctl --user enable miocad-render-service.service
systemctl --user restart miocad-render-service.service
# Wait for an actual rendering listener, not just a systemd process or an old log line.
python3 - "$runtime" <<'PYREADY'
import json,socket,sys,time
from pathlib import Path
root=Path(sys.argv[1])
for attempt in range(80):
    try:
        endpoint=json.loads((root/'endpoint.json').read_bytes());upstream=json.loads((root/'render-upstream.json').read_bytes())
        assert endpoint['address']!=upstream['address'] and endpoint['service_id']==upstream['service_id']
        def call(request):
            host,port=endpoint['address'].rsplit(':',1)
            with socket.create_connection((host,int(port)),timeout=2) as connection:
                connection.sendall((json.dumps({'protocol':endpoint['protocol'],'service_id':endpoint['service_id'],'token':endpoint['token'],'request':request})+'\n').encode())
                reply=json.loads(connection.makefile('rb').readline());assert 'result' in reply;return reply['result']
        session=call({'method':'ping'})['default_session'];assert isinstance(call({'method':'render_frames','session':session}),list)
        break
    except Exception:
        if attempt==79:raise
        time.sleep(.1)
PYREADY
systemctl --user is-active --quiet miocad-render-service.service
# The portal's base64 validator currently overflows its stack on large ELF/WASM files.
# Use its documented persistent-local-service + proxy mode instead of uploading binaries.
web_hash="$(find artifacts/web-bundle -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -c1-16)"
web_release="$HOME/.local/share/miocad/web-$web_hash"
if [[ ! -f "$web_release/.complete" ]]; then
    mkdir -p "$web_release"
    cp -a artifacts/web-bundle/. "$web_release/"
    chmod 700 "$web_release/bin/cad-web"
    touch "$web_release/.complete"
fi
web_port="${CAD_WEB_PORT:-19060}"
# Thin local adapter; provider credentials remain in the installed Agent's own store.
agent_pi="${CAD_AGENT_PI:-$(command -v pi || true)}"
agent_backend="${CAD_AGENT_BACKEND:-disabled}"
if [[ -n "$agent_pi" && -z "${CAD_AGENT_BACKEND:-}" ]]; then agent_backend=pi; fi
cat > "$HOME/.config/systemd/user/miocad-web.service" <<UNIT
[Unit]
Description=AI-CAD egui Web gateway
After=network-online.target miocad-render-service.service
[Service]
ExecStart="$web_release/bin/cad-web" --runtime-dir "$runtime"
Environment=HOST=127.0.0.1
Environment=PORT=$web_port
Environment=BASE_PATH=/apps/miocad/
Environment=CAD_WEB_ATTACH_ONLY=1
Environment=CAD_WEB_REQUIRE_RELAY=1
Environment=CAD_WEB_STRIP_PREFIX=1
# Trusted Tailscale deployment: keep browser scoping, omit the manual access code.
Environment=CAD_WEB_TRUSTED_ACCESS=1
Environment=CAD_AGENT_BACKEND=$agent_backend
Environment=CAD_AGENT_PI="$agent_pi"
Restart=on-failure
RestartSec=3
NoNewPrivileges=yes
UMask=0077
[Install]
WantedBy=default.target
UNIT
systemctl --user daemon-reload
systemctl --user enable --now miocad-web.service
systemctl --user restart miocad-web.service
for attempt in {1..40}; do
    if curl --noproxy '*' --fail --silent "http://127.0.0.1:$web_port/health" >/dev/null; then break; fi
    sleep 0.25
done
curl --noproxy '*' --fail --silent "http://127.0.0.1:$web_port/health" >/dev/null
systemctl --user is-active --quiet miocad-web.service
~/.local/bin/portalctl proxy --slug miocad --name 'AI-CAD' \
    --description 'Rust egui 精确 CAD 工作区，与桌面和 Agent 默认共享会话' \
    --target "http://127.0.0.1:$web_port" --strip-prefix true
