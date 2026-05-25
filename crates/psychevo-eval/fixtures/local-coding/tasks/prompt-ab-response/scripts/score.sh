set -eu

if grep -qx 'concise' response_policy.txt && grep -qx 'concrete' response_policy.txt && grep -qx 'local' response_policy.txt; then
    cat <<'JSON'
{"schema_version":1,"passed":true,"score":1.0,"message":"prompt policy matched oracle","details":{"scorer":"prompt-policy","required":["concise","concrete","local"]}}
JSON
else
    cat <<'JSON'
{"schema_version":1,"passed":false,"score":0.0,"message":"prompt policy did not match oracle","details":{"scorer":"prompt-policy","required":["concise","concrete","local"]}}
JSON
fi
