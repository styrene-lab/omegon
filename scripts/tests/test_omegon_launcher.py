#!/usr/bin/env python3
import os
import stat
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LAUNCHER = ROOT / "scripts" / "omegon-launcher.sh"


def make_bin(path: Path, label: str):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f"#!/usr/bin/env bash\nif [[ \"${{1:-}}\" == \"--version\" ]]; then echo 'omegon test {label}'; else echo 'run {label} $*'; fi\n")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def target_from(stdout: str) -> Path:
    for line in stdout.splitlines():
        if line.startswith("target: "):
            return Path(line.removeprefix("target: "))
    raise AssertionError(f"no target line in {stdout!r}")


def same_target(stdout: str, expected: Path) -> bool:
    return os.path.samefile(target_from(stdout), expected)


def fix_root_from(stdout: str) -> Path:
    for line in stdout.splitlines():
        if line.startswith("fix: cd ") and line.endswith(" && just link"):
            return Path(line.removeprefix("fix: cd ").removesuffix(" && just link"))
    raise AssertionError(f"no fix line in {stdout!r}")


def run(args, cwd, home, env=None):
    e = os.environ.copy()
    e.pop("OMEGON_BIN", None)
    e.pop("OMEGON_DEV_ROOT", None)
    e.pop("OMEGON_CHANNEL", None)
    e["HOME"] = str(home)
    if env:
        e.update(env)
    return subprocess.run([str(LAUNCHER), *args], cwd=cwd, env=e, text=True, capture_output=True)


def test_env_omegon_bin_wins():
    with tempfile.TemporaryDirectory() as td:
        base = Path(td); home = base / "home"; cwd = base / "work"; cwd.mkdir(); home.mkdir()
        exact = base / "exact" / "omegon"; make_bin(exact, "exact")
        res = run(["--which"], cwd, home, {"OMEGON_BIN": str(exact)})
        assert res.returncode == 0, res.stderr
        assert "reason: env:OMEGON_BIN" in res.stdout
        assert same_target(res.stdout, exact)
        assert "omegon test exact" in res.stdout


def test_env_dev_root_wins_over_nearest_checkout():
    with tempfile.TemporaryDirectory() as td:
        base = Path(td); home = base / "home"; home.mkdir()
        local = base / "local"; (local / "core/crates/omegon").mkdir(parents=True); (local / "Cargo.toml").write_text("[workspace]\n")
        other = base / "other"; (other / "core/crates/omegon").mkdir(parents=True); (other / "Cargo.toml").write_text("[workspace]\n")
        make_bin(local / "target/release/omegon", "local")
        make_bin(other / "target/release/omegon", "other")
        res = run(["--which"], local, home, {"OMEGON_DEV_ROOT": str(other)})
        assert res.returncode == 0, res.stderr
        assert "reason: env:OMEGON_DEV_ROOT" in res.stdout
        assert same_target(res.stdout, other / "target/release/omegon")


def test_nearest_checkout_precedes_channel():
    with tempfile.TemporaryDirectory() as td:
        base = Path(td); home = base / "home"; home.mkdir()
        checkout = base / "checkout"; nested = checkout / "a/b"; nested.mkdir(parents=True)
        (checkout / "core/crates/omegon").mkdir(parents=True); (checkout / "Cargo.toml").write_text("[workspace]\n")
        channel = base / "channel"; (channel / "core/crates/omegon").mkdir(parents=True); (channel / "Cargo.toml").write_text("[workspace]\n")
        make_bin(checkout / "target/release/omegon", "checkout")
        make_bin(channel / "target/release/omegon", "channel")
        (home / ".omegon/channels").mkdir(parents=True)
        (home / ".omegon/channels/default").write_text(str(channel) + "\n")
        res = run(["--which"], nested, home)
        assert res.returncode == 0, res.stderr
        assert "reason: nearest-checkout" in res.stdout
        assert same_target(res.stdout, checkout / "target/release/omegon")


def test_named_channel_used_outside_checkout():
    with tempfile.TemporaryDirectory() as td:
        base = Path(td); home = base / "home"; cwd = base / "outside"; cwd.mkdir(); home.mkdir()
        channel = base / "release"; (channel / "core/crates/omegon").mkdir(parents=True); (channel / "Cargo.toml").write_text("[workspace]\n")
        make_bin(channel / "target/release/omegon", "release")
        (home / ".omegon/channels").mkdir(parents=True)
        (home / ".omegon/channels/release").write_text(str(channel) + "\n")
        res = run(["--which"], cwd, home, {"OMEGON_CHANNEL": "release"})
        assert res.returncode == 0, res.stderr
        assert "reason: channel:release" in res.stdout
        assert same_target(res.stdout, channel / "target/release/omegon")


def test_fallback_used_without_checkout_or_channel():
    with tempfile.TemporaryDirectory() as td:
        base = Path(td); home = base / "home"; cwd = base / "outside"; cwd.mkdir(); home.mkdir()
        fallback = home / ".omegon/bin/omegon"; make_bin(fallback, "fallback")
        res = run(["--which"], cwd, home)
        assert res.returncode == 0, res.stderr
        assert "reason: fallback-installed" in res.stdout
        assert same_target(res.stdout, fallback)


def test_stale_channel_fails_loudly():
    with tempfile.TemporaryDirectory() as td:
        base = Path(td); home = base / "home"; cwd = base / "outside"; cwd.mkdir(); home.mkdir()
        (home / ".omegon/channels").mkdir(parents=True)
        (home / ".omegon/channels/default").write_text(str(base / "missing") + "\n")
        res = run(["--which"], cwd, home)
        assert res.returncode != 0
        assert "channel default has no runnable binary" in res.stderr


def test_paths_with_spaces_work_for_dev_root_and_channel():
    with tempfile.TemporaryDirectory() as td:
        base = Path(td); home = base / "home dir"; cwd = base / "outside dir"; cwd.mkdir(); home.mkdir()
        dev = base / "checkout with spaces"; (dev / "core/crates/omegon").mkdir(parents=True); (dev / "Cargo.toml").write_text("[workspace]\n")
        channel = base / "channel with spaces"; (channel / "core/crates/omegon").mkdir(parents=True); (channel / "Cargo.toml").write_text("[workspace]\n")
        make_bin(dev / "target/release/omegon", "dev spaces")
        make_bin(channel / "target/release/omegon", "channel spaces")

        res = run(["--which"], cwd, home, {"OMEGON_DEV_ROOT": str(dev)})
        assert res.returncode == 0, res.stderr
        assert "reason: env:OMEGON_DEV_ROOT" in res.stdout
        assert same_target(res.stdout, dev / "target/release/omegon")

        (home / ".omegon/channels").mkdir(parents=True)
        (home / ".omegon/channels/default").write_text(str(channel) + "\n")
        res = run(["--which"], cwd, home)
        assert res.returncode == 0, res.stderr
        assert "reason: channel:default" in res.stdout
        assert same_target(res.stdout, channel / "target/release/omegon")


def test_dev_release_fallback_under_checkout():
    with tempfile.TemporaryDirectory() as td:
        base = Path(td); home = base / "home"; home.mkdir()
        checkout = base / "checkout"; nested = checkout / "nested"; nested.mkdir(parents=True)
        (checkout / "core/crates/omegon").mkdir(parents=True); (checkout / "Cargo.toml").write_text("[workspace]\n")
        make_bin(checkout / "target/dev-release/omegon", "dev-release")
        res = run(["--which"], nested, home)
        assert res.returncode == 0, res.stderr
        assert "reason: nearest-checkout" in res.stdout
        assert same_target(res.stdout, checkout / "target/dev-release/omegon")


def test_launcher_rejects_self_as_omegon_bin():
    with tempfile.TemporaryDirectory() as td:
        base = Path(td); home = base / "home"; cwd = base / "outside"; cwd.mkdir(); home.mkdir()
        res = run(["--which"], cwd, home, {"OMEGON_BIN": str(LAUNCHER)})
        assert res.returncode != 0
        assert "OMEGON_BIN is not executable or points to launcher" in res.stderr


def test_which_reports_stale_checkout_build():
    with tempfile.TemporaryDirectory() as td:
        base = Path(td); home = base / "home"; home.mkdir()
        checkout = base / "checkout"; checkout.mkdir()
        subprocess.run(["git", "init"], cwd=checkout, check=True, capture_output=True)
        subprocess.run(["git", "config", "user.email", "test@example.com"], cwd=checkout, check=True)
        subprocess.run(["git", "config", "user.name", "Test"], cwd=checkout, check=True)
        (checkout / "core/crates/omegon").mkdir(parents=True)
        (checkout / "Cargo.toml").write_text("[workspace]\n")
        (checkout / "file").write_text("one\n")
        subprocess.run(["git", "add", "."], cwd=checkout, check=True)
        subprocess.run(["git", "commit", "-m", "init"], cwd=checkout, check=True, capture_output=True)
        head = subprocess.check_output(["git", "rev-parse", "--short", "HEAD"], cwd=checkout, text=True).strip()
        make_bin(checkout / "target/release/omegon", "oldhash")

        res = run(["--which"], checkout, home)
        assert res.returncode == 0, res.stderr
        assert f"checkout-head: {head}" in res.stdout
        assert "stale: yes" in res.stdout
        assert os.path.samefile(fix_root_from(res.stdout), checkout)


if __name__ == "__main__":
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for test in tests:
        test()
        print(f"ok {test.__name__}")
