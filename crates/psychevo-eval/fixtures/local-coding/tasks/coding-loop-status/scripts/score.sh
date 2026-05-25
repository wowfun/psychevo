set -eu

if grep -qx 'status=done' status.txt && grep -qx 'owner=agent' status.txt; then
    cat <<'JSON'
{"schema_version":1,"passed":true,"score":1.0,"message":"release status completed","details":{"scorer":"status-file","required":["status=done","owner=agent"]}}
JSON
else
    cat <<'JSON'
{"schema_version":1,"passed":false,"score":0.0,"message":"release status was not completed","details":{"scorer":"status-file","required":["status=done","owner=agent"]}}
JSON
fi
