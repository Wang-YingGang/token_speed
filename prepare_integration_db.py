from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parent
SOURCE = Path.home() / ".cc-switch" / "cc-switch.db"
DEST_DIR = ROOT / "integration"
DEST = DEST_DIR / "cc-switch.db"
FIELDS = (
    "request_id, app_type, model, input_tokens, output_tokens, latency_ms, "
    "first_token_ms, duration_ms, status_code, is_streaming, data_source, created_at"
)

if not SOURCE.exists():
    raise SystemExit(f"未找到数据库：{SOURCE}")
DEST_DIR.mkdir(exist_ok=True)
if DEST.exists():
    DEST.unlink()
for suffix in ("-wal", "-shm"):
    sidecar = Path(str(DEST) + suffix)
    if sidecar.exists():
        sidecar.unlink()

source = sqlite3.connect(f"file:{SOURCE.as_posix()}?mode=ro", uri=True)
source.execute("PRAGMA query_only = ON")
columns = source.execute("PRAGMA table_info(proxy_request_logs)").fetchall()
print("真实表字段：", ", ".join(f"{column[1]}:{column[2]}" for column in columns))
rows = []
for variants in (("claude", "claude-desktop", "claude_code"), ("codex",)):
    placeholders = ",".join("?" * len(variants))
    rows.extend(source.execute(
        f"SELECT {FIELDS} FROM proxy_request_logs "
        f"WHERE app_type IN ({placeholders}) "
        "AND status_code >= 200 AND status_code < 300 AND output_tokens > 0 "
        "ORDER BY created_at DESC, rowid DESC LIMIT 120",
        variants,
    ).fetchall())
print(f"只读复制成功用量记录：{len(rows)} 条")

dest = sqlite3.connect(DEST)
dest.execute(
    "CREATE TABLE proxy_request_logs ("
    "request_id TEXT, app_type TEXT, model TEXT, input_tokens INTEGER, "
    "output_tokens INTEGER, latency_ms INTEGER, first_token_ms INTEGER, "
    "duration_ms INTEGER, status_code INTEGER, is_streaming INTEGER, "
    "data_source TEXT, created_at INTEGER)"
)
dest.executemany(f"INSERT INTO proxy_request_logs ({FIELDS}) VALUES ({','.join('?' * 12)})", rows)
dest.commit()

for normalized, variants in (("Claude", ("claude", "claude-desktop", "claude_code")), ("Codex", ("codex",))):
    placeholders = ",".join("?" * len(variants))
    record = dest.execute(
        "SELECT model, output_tokens, latency_ms, first_token_ms, duration_ms, created_at "
        f"FROM proxy_request_logs WHERE app_type IN ({placeholders}) "
        "AND status_code >= 200 AND status_code < 300 AND output_tokens > 0 "
        "ORDER BY created_at DESC, rowid DESC LIMIT 1",
        variants,
    ).fetchone()
    if not record:
        print(f"{normalized}：暂无成功记录")
        continue
    model, output, latency, first_token, duration, created_at = record
    if first_token is not None and first_token >= 0 and latency > first_token:
        generation = latency - first_token
    elif duration is not None and duration > 0:
        generation = duration
    else:
        generation = max(latency, 0)
    speed = output * 1000 / generation if generation > 0 else 0
    print(
        f"{normalized}：model={model}, output={output}, generation={generation} ms, "
        f"speed={speed:.2f} t/s, created_at={created_at}"
    )

print(f"联调副本：{DEST}")
dest.close()
source.close()
