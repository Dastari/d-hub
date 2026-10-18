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

        def install(*arguments):
            return subprocess.run(["sh", str(ROOT / "scripts/install.sh"), *arguments], env=env, capture_output=True, text=True)

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
        refused = install()
        assert refused.returncode != 0 and "--replace" in refused.stderr
        assert link.read_text() == "unrelated executable"
        # A failed download cannot move the previous launcher or create a backup.
        assert install("--replace").returncode != 0
        assert link.read_text() == "unrelated executable"
        assert not list(link.parent.glob("d-hub-backup.*"))
        (fixture / name).write_bytes(original)
        env["FAKE_VERSION"] = "999.0.0"
        assert install("--replace").returncode != 0
        assert link.read_text() == "unrelated executable"
        assert not list(link.parent.glob("d-hub-backup.*"))
        env["FAKE_VERSION"] = version
        link.chmod(0o755)
        migrated = install("--replace")
        assert migrated.returncode == 0, migrated.stderr
        backups = set(link.parent.glob("d-hub-backup.*"))
        assert len(backups) == 1
        backup = next(iter(backups))
        assert backup.stat().st_mode & 0o077 == 0
        assert (backup / "d-hub").read_text() == "unrelated executable"
        assert (backup / "d-hub").stat().st_mode & 0o777 == 0o755
        assert str(backup / "d-hub") in migrated.stdout
        assert link.is_symlink() and link.resolve() == binary
        # Preserve a source-build symlink as a link; never modify its target.
        target = home / "dev/build"
        target.parent.mkdir()
        target.write_text("source build stays unchanged")
        link.unlink()
        link.symlink_to("../../dev/build")
        assert install("--replace").returncode == 0
        added = set(link.parent.glob("d-hub-backup.*")) - backups
        assert len(added) == 1
        assert os.readlink(next(iter(added)) / "d-hub") == "../../dev/build"
        assert target.read_text() == "source build stays unchanged"
        backups |= added
        # Dangling development links are preserved too.
        link.unlink()
        missing = home / "missing-build"
        link.symlink_to(missing)
        assert install("--replace").returncode == 0
        added = set(link.parent.glob("d-hub-backup.*")) - backups
        assert len(added) == 1
        assert os.readlink(next(iter(added)) / "d-hub") == str(missing)
        assert not missing.exists()
        # Replacing a symlink to a directory must not install inside that directory.
        backups |= added
        target_directory = home / "linked directory"
        target_directory.mkdir()
        (target_directory / "keep").write_text("retained")
        link.unlink()
        link.symlink_to(target_directory)
        assert install("--replace").returncode == 0
        added = set(link.parent.glob("d-hub-backup.*")) - backups
        assert len(added) == 1
        assert os.readlink(next(iter(added)) / "d-hub") == str(target_directory)
        assert list(target_directory.iterdir()) == [target_directory / "keep"]
        # A directory at the launcher path is always refused, even with --replace.
        link.unlink()
        link.mkdir()
        (link / "keep").write_text("retained")
        assert install("--replace").returncode != 0
        assert (link / "keep").read_text() == "retained"
        assert install("--help").returncode == 0
        assert install("--unknown").returncode != 0
    print("Installer smoke passed: checksums/version, managed reinstall, explicit file/relative-symlink/dangling-link migration with private backups, preserved targets, validation failure and directory protection; no HTTP.")


if __name__ == "__main__":
    main()
