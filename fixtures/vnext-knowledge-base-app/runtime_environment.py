"""Environment for the source-deleted reference Host."""

from pathlib import Path


def build_runtime_environment(
    root: Path, database_url: str, signing_secret: str, token_pepper: str
) -> dict[str, str]:
    home = root / "runtime-home"
    temporary = root / "runtime-tmp"
    home.mkdir(mode=0o700)
    temporary.mkdir(mode=0o700)
    return {
        "PATH": str(root / "no-tools"),
        "HOME": str(home),
        "TMPDIR": str(temporary),
        "LENSO_REFERENCE_DATABASE_URL": database_url,
        "LENSO_AUTH_SIGNING_SECRET": signing_secret,
        "LENSO_AUTH_TOKEN_PEPPER": token_pepper,
    }
