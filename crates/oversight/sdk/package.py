"""Packs the built engine into an SDK folder and archive.

    cargo build --release -p oversight
    python crates/oversight/sdk/package.py --out dist

Makes dist/oversight-sdk-<version>-<platform>/ with the library, headers,
the CMake package and the OverSight.Net NuGet package (needs the .NET 8
SDK; --no-csharp skips it), plus a .zip (Windows) or .tar.gz (Linux) of it.
A game needs only this folder: no Rust, no repository checkout.
"""

import argparse
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path

SDK = Path(__file__).resolve().parent
CRATE = SDK.parent
ROOT = CRATE.parent.parent

# What each platform ships: (file in the cargo output, folder in the SDK)
LIBRARIES = {
    "windows": [("oversight.dll", "bin"), ("oversight.dll.lib", "lib")],
    "linux": [("liboversight.so", "lib")],
    "macos": [("liboversight.dylib", "lib")],
}

VERSION_FILE = """\
# Any 0.x release is compatible only with itself, later majors keep
# compatibility within the major
set(PACKAGE_VERSION "{version}")
if(PACKAGE_FIND_VERSION VERSION_GREATER PACKAGE_VERSION)
    set(PACKAGE_VERSION_COMPATIBLE FALSE)
elseif(NOT PACKAGE_FIND_VERSION_MAJOR EQUAL {major})
    set(PACKAGE_VERSION_COMPATIBLE FALSE)
elseif({major} EQUAL 0 AND NOT PACKAGE_FIND_VERSION_MINOR EQUAL {minor})
    set(PACKAGE_VERSION_COMPATIBLE FALSE)
else()
    set(PACKAGE_VERSION_COMPATIBLE TRUE)
    if(PACKAGE_FIND_VERSION STREQUAL PACKAGE_VERSION)
        set(PACKAGE_VERSION_EXACT TRUE)
    endif()
endif()
"""


def crate_version():
    text = (CRATE / "Cargo.toml").read_text(encoding="utf-8")
    return re.search(r'^version\s*=\s*"([^"]+)"', text, re.M).group(1)


def host_platform():
    system = platform.system()
    if system == "Windows":
        return "windows"
    if system == "Darwin":
        return "macos"
    return "linux"


def pack_csharp(lib_dir, dest, version):
    """OverSight.Net.<version>.nupkg with this platform's native library."""
    project = ROOT / "bindings" / "csharp" / "OverSight" / "OverSight.csproj"
    subprocess.run(
        [
            "dotnet", "pack", str(project), "-c", "Release", "-nologo",
            "-o", str(dest / "nuget"),
            f"-p:Version={version}",
            # Trailing slash: the project joins it with the file name
            f"-p:OverSightLibDir={lib_dir.resolve()}/",
        ],
        check=True,
    )


def package(lib_dir, out, system, arch, csharp=True):
    version = crate_version()
    name = f"oversight-sdk-{version}-{system}-{arch}"
    dest = out / name
    if dest.exists():
        shutil.rmtree(dest)

    for file, folder in LIBRARIES[system]:
        src = lib_dir / file
        if not src.is_file():
            sys.exit(f"{src} is missing: run cargo build --release -p oversight first")
        (dest / folder).mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dest / folder / file)

    shutil.copytree(CRATE / "include", dest / "include")
    shutil.copytree(SDK / "cmake", dest / "cmake")
    major, minor = version.split(".")[:2]
    (dest / "cmake" / "oversight-config-version.cmake").write_text(
        VERSION_FILE.format(version=version, major=major, minor=minor), encoding="utf-8"
    )
    if csharp:
        pack_csharp(lib_dir, dest, version)
    shutil.copytree(SDK / "templates", dest / "templates")
    for script in ("new-game.cmd", "new-game.ps1", "new-game.sh"):
        shutil.copy2(SDK / script, dest / script)
    (dest / "VERSION").write_text(version + "\n", encoding="utf-8")
    shutil.copy2(SDK / "README.md", dest / "README.md")
    shutil.copy2(ROOT / "LICENSE", dest / "LICENSE")

    if system == "windows":
        archive = out / f"{name}.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
            for f in sorted(dest.rglob("*")):
                z.write(f, Path(name) / f.relative_to(dest))
    else:
        archive = out / f"{name}.tar.gz"
        def executable(info):
            if info.name.endswith(".sh"):
                info.mode |= 0o755
            return info

        with tarfile.open(archive, "w:gz") as t:
            t.add(dest, arcname=name, filter=executable)
    print(f"{dest}\n{archive}")
    return dest


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--lib-dir", type=Path, default=ROOT / "target" / "release")
    p.add_argument("--out", type=Path, default=ROOT / "dist")
    p.add_argument("--platform", choices=sorted(LIBRARIES), default=host_platform())
    p.add_argument("--arch", default="x64")
    p.add_argument("--no-csharp", action="store_true", help="skip the NuGet package")
    a = p.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    package(a.lib_dir, a.out.resolve(), a.platform, a.arch, csharp=not a.no_csharp)


if __name__ == "__main__":
    main()
