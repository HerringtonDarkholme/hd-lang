"""Prepare a local workerd control bundle; copies only public repo sources."""
from pathlib import Path
import shutil
import sys

repo = Path.cwd()
source = Path(__file__).resolve().parent
output = Path(sys.argv[1]).resolve()
output.relative_to(repo.resolve())
output.mkdir(parents=True, exist_ok=True)
modules = [
    "host/core.mjs", "host/checked-report.mjs", "host/float.mjs",
    "host/gen/abi.mjs", "host/journal.mjs", "host/wire.mjs",
]
for name in modules:
    target = output / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(repo / name, target)
(output / "fetch-control.mjs").write_text(
    (source / "fetch-control.mjs").read_text().replace(
        "../../../../host/core.mjs", "./host/core.mjs"
    )
)
shutil.copy2(source / "app/build/debug/muse-http-prototype.wasm", output / "handler.wasm")
(output / "main.mjs").write_text("""import wasm from './handler.wasm';
import {invoke} from './fetch-control.mjs';
export default {async fetch(request) {
 const got=await invoke(wasm,request);
 return got.response ?? new Response(JSON.stringify(got),{status:500});
}};
""")
entries = [
    f'(name = "./{name}", esModule = embed "./{name}")'
    for name in ["main.mjs", "fetch-control.mjs"] + modules
]
entries.append('(name = "./handler.wasm", wasm = embed "./handler.wasm")')
(output / "server.capnp").write_text(
    'using Workerd = import "/workerd/workerd.capnp";\n'
    'const config :Workerd.Config = (\n'
    'services = [(name = "main", worker = (compatibilityDate = "2026-10-23", modules = [\n'
    + ',\n'.join(entries)
    + '\n], globalOutbound = "loopback")), '
    '(name = "loopback", external = (address = "127.0.0.1:18768", http = ()))],\n'
    'sockets = [(name = "http", address = "127.0.0.1:18767", http = (), service = "main")]\n);\n'
)
(output / "feature.wasm").write_bytes(bytes.fromhex(
    "0061736d010000000105016000017f020701016d0166000003020100070501016600010a0601040010000b"
))
shutil.copy2(source / "workerd-features.js", output / "features.mjs")
(output / "features.capnp").write_text(
    'using Workerd = import "/workerd/workerd.capnp";\n'
    'const config :Workerd.Config = (\n'
    'services = [(name = "main", worker = (compatibilityDate = "2026-10-23", modules = [\n'
    '(name = "./main.js", esModule = embed "./features.mjs"),\n'
    '(name = "./feature.wasm", wasm = embed "./feature.wasm")]))],\n'
    'sockets = [(name = "http", address = "127.0.0.1:18766", http = (), service = "main")]\n);\n'
)
print(output / "server.capnp")
