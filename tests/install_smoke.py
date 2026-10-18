#!/usr/bin/env python3
"""Exercise the real shell installer using local fake curl releases, no network."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def main():
    with tempfile.TemporaryDirectory(prefix="d-hub-install-") as temporary:
        root = Path(temporary)
        fake_bin = root / "tools"
        fake_bin.mkdir()
        fixture = root / "release"
        fixture.mkdir()
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
        arch = {"x86_64": "x86_64", "aarch64": "aarch64"}[os.uname().machine]
        name = f"d-hub-{arch}-unknown-linux-gnu"
        shutil.copy2(ROOT / "target/debug/d-hub", fixture / name)
        digest = hashlib.sha256((fixture / name).read_bytes()).hexdigest()
        (fixture / "SHA256SUMS").write_text(f"{digest}  {name}\n")
        curl = fake_bin / "curl"
        curl.write_text('''#!/usr/bin/env python3
import os,shutil,sys
from pathlib import Path
args=sys.argv[1:]
if args[-1].endswith('/latest'):
    print('https://github.com/dastari/d-hub/releases/tag/v'+os.environ['FAKE_VERSION'],end='')
else:
    url=next(a for a in args if a.startswith('https://'))
    dest=args[args.index('-o')+1]
    shutil.copyfile(Path(os.environ['FAKE_RELEASE'])/url.rsplit('/',1)[-1],dest)
''')
        curl.chmod(0o755)
        home = root / "home with spaces"
        home.mkdir()
        env = os.environ | {"HOME": str(home), "XDG_DATA_HOME": str(home / "data"), "PATH": f"{fake_bin}:{os.environ['PATH']}", "FAKE_RELEASE": str(fixture), "FAKE_VERSION": version}

        def install():
            return subprocess.run(["sh", str(ROOT / "scripts/install.sh")], env=env, capture_output=True, text=True)

        result = install()
        assert result.returncode == 0, result.stderr
        binary = home / "data/d-hub/bin/d-hub"
        link = home / ".local/bin/d-hub"
        assert link.is_symlink() and link.resolve() == binary
        assert binary.parent.stat().st_mode & 0o077 == 0
        assert subprocess.check_output([str(link), "--version"], text=True).strip() == f"d-hub {version}"
        assert install().returncode == 0  # replace owned install
        assert (binary.parent / "d-hub.previous").exists()
        original = binary.read_bytes()
        (fixture / name).write_bytes(b"corrupt download")
        assert install().returncode != 0
        assert binary.read_bytes() == original
        link.unlink()
        link.write_text("unrelated executable")
        assert install().returncode != 0
        assert link.read_text() == "unrelated executable"
    print("Installer smoke passed: spaces in paths, managed symlink, version/checksum, reinstall/backup, corrupt download and existing executable preservation; no HTTP.")


if __name__ == "__main__":
    main()
