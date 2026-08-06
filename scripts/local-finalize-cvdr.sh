#!/usr/bin/env bash
# Local-only: capture /cvdr_live then call permissionless finalize_cvdr backstop.
set -euo pipefail
RECEIPT_HEX="${1:?usage: $0 <64-hex-receipt-id>}"
LUI=$(dfx canister id local_user_index --network local)
LIVE_URL="http://${LUI}.raw.localhost:8080/cvdr_live/${RECEIPT_HEX}"
echo "GET $LIVE_URL"
LIVE=$(curl -sf "$LIVE_URL")
CERT_HEX=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["certificate"])' <<<"$LIVE")
WIT_HEX=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["witness_cbor"])' <<<"$LIVE")
python3 - <<PY
import json, pathlib
receipt=bytes.fromhex("$RECEIPT_HEX")
cert=bytes.fromhex("$CERT_HEX")
wit=bytes.fromhex("$WIT_HEX")
# candid: record { receipt_id: blob; certificate: blob; witness: blob }
# Use dfx with raw blob encoding via python candid? easier with didc or ic-utils
print("receipt", receipt.hex()[:16], "cert", len(cert), "wit", len(wit))
pathlib.Path("/tmp/cvdr_finalize_args.json").write_text(json.dumps({
  "receipt_id": list(receipt),
  "certificate": list(cert),
  "witness": list(wit),
}))
PY
# Build candid IDL from hex using didc if available; else msgpack via agent is hard.
# Fallback: use `dfx canister call` with blob vectors
CERT_CANDID=$(python3 -c 'c=bytes.fromhex("'"$CERT_HEX"'"); print("blob \\"" + "".join(f"\\{b:02x}" for b in c) + "\\"")')
WIT_CANDID=$(python3 -c 'c=bytes.fromhex("'"$WIT_HEX"'"); print("blob \\"" + "".join(f"\\{b:02x}" for b in c) + "\\"")')
RID_CANDID=$(python3 -c 'c=bytes.fromhex("'"$RECEIPT_HEX"'"); print("blob \\"" + "".join(f"\\{b:02x}" for b in c) + "\\"")')
dfx canister call local_user_index finalize_cvdr "(record { receipt_id = $RID_CANDID; certificate = $CERT_CANDID; witness = $WIT_CANDID })" --network local
echo "Poll:"
curl -s "http://${LUI}.raw.localhost:8080/cvdr/${RECEIPT_HEX}" | head -c 200; echo
